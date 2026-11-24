//! rekordbox 6/7 support: locate and open `master.db` read-only.

pub mod history;
mod source;
pub mod sqlcipher;

pub use source::RekordboxDbSource;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Where the database key is stored for the user (never shipped with djcast).
pub fn default_key_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("djcast").join("rekordbox-db.key"))
}

/// Load the key from an explicit value, or from [`default_key_path`].
pub fn load_key(explicit: Option<&str>) -> Result<String> {
    if let Some(k) = explicit.map(str::trim).filter(|k| !k.is_empty()) {
        return Ok(k.to_string());
    }
    let path = default_key_path().context("no config directory")?;
    let key = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "rekordbox database key not found at {}. See docs/rekordbox-key.md for how to obtain it, \
             then run `djcast rekordbox set-key <KEY>`",
            path.display()
        )
    })?;
    Ok(key.trim().to_string())
}

pub fn save_key(key: &str) -> Result<PathBuf> {
    let path = default_key_path().context("no config directory")?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, key.trim())?;
    Ok(path)
}

/// Find `master.db`. rekordbox records its location in the agent's `options.json`
/// (`db-path`); we fall back to the default location.
///
/// We deliberately do not read `rekordbox3.settings`, which also holds account tokens.
pub fn find_master_db() -> Result<PathBuf> {
    let config = dirs::config_dir().context("no config directory")?;
    let pioneer = if cfg!(target_os = "macos") {
        dirs::home_dir().context("no home directory")?.join("Library/Pioneer")
    } else {
        config.join("Pioneer")
    };
    let options = if cfg!(target_os = "macos") {
        dirs::home_dir().unwrap().join("Library/Application Support/Pioneer/rekordboxAgent/storage/options.json")
    } else {
        pioneer.join("rekordboxAgent/storage/options.json")
    };
    if let Some(path) = db_path_from_options(&options) {
        if path.exists() {
            return Ok(path);
        }
        log::warn!("options.json points to missing {}", path.display());
    }
    let fallback = pioneer.join("rekordbox/master.db");
    if fallback.exists() {
        return Ok(fallback);
    }
    bail!("rekordbox master.db not found (is rekordbox 6/7 installed?)")
}

fn db_path_from_options(options: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(options).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("options")?
        .as_array()?
        .iter()
        .filter_map(|o| o.as_array())
        .find(|o| o.first().and_then(|k| k.as_str()) == Some("db-path"))
        .and_then(|o| o.get(1)?.as_str().map(PathBuf::from))
}

/// Opens `master.db` by decrypting it into memory. Reuses derived keys across reloads
/// and only re-decrypts when the files changed.
pub struct MasterDb {
    path: PathBuf,
    passphrase: String,
    keys: Option<sqlcipher::Keys>,
    stamp: Option<(Option<SystemTime>, Option<SystemTime>, u64)>,
}

impl MasterDb {
    pub fn new(path: PathBuf, passphrase: String) -> Self {
        Self { path, passphrase, keys: None, stamp: None }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn wal_path(&self) -> PathBuf {
        let mut p = self.path.clone().into_os_string();
        p.push("-wal");
        PathBuf::from(p)
    }

    /// True if master.db or its WAL changed since the last successful [`open`](Self::open).
    pub fn changed(&self) -> bool {
        self.stamp.is_none() || self.current_stamp() != self.stamp
    }

    fn current_stamp(&self) -> Option<(Option<SystemTime>, Option<SystemTime>, u64)> {
        let db = std::fs::metadata(&self.path).ok();
        let wal = std::fs::metadata(self.wal_path()).ok();
        Some((
            db.as_ref().and_then(|m| m.modified().ok()),
            wal.as_ref().and_then(|m| m.modified().ok()),
            wal.map(|m| m.len()).unwrap_or(0),
        ))
    }

    /// Decrypt the current state into an in-memory read-only connection.
    /// Retries a few times because rekordbox may be writing while we read.
    pub fn open(&mut self) -> Result<Connection> {
        let mut last_err = None;
        for attempt in 0..4 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            match self.try_open() {
                Ok(c) => return Ok(c),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap())
    }

    fn try_open(&mut self) -> Result<Connection> {
        let stamp = self.current_stamp();
        let db = std::fs::read(&self.path).with_context(|| format!("reading {}", self.path.display()))?;
        let wal = std::fs::read(self.wal_path()).ok();
        let salt = sqlcipher::read_salt(&db)?;
        if self.keys.as_ref().map(|k| k.salt()) != Some(salt) {
            self.keys = Some(sqlcipher::Keys::derive(&self.passphrase, salt));
        }
        let image = sqlcipher::decrypt(self.keys.as_ref().unwrap(), &db, wal.as_deref())?;
        let mut conn = Connection::open_in_memory()?;
        conn.deserialize_read_exact(rusqlite::MAIN_DB, image.as_slice(), image.len(), true)?;
        self.stamp = stamp;
        Ok(conn)
    }
}
