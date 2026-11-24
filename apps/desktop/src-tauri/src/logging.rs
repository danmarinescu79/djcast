//! Log to a file next to the config (`logs/djcast.log`), rotated by size, and to stderr.
//! Release builds have no console, so the file is what users can send when reporting a problem.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const KEEP: usize = 3;

struct FileLogger {
    path: PathBuf,
    file: Mutex<Option<File>>,
}

impl FileLogger {
    fn open(path: &Path) -> Option<File> {
        OpenOptions::new().create(true).append(true).open(path).ok()
    }

    /// djcast.log → djcast.1.log → … → djcast.<KEEP>.log (oldest dropped).
    fn rotate(&self) {
        let rotated = |i: usize| self.path.with_file_name(format!("djcast.{i}.log"));
        let _ = fs::remove_file(rotated(KEEP));
        for i in (1..KEEP).rev() {
            let _ = fs::rename(rotated(i), rotated(i + 1));
        }
        let _ = fs::rename(&self.path, rotated(1));
    }
}

impl log::Log for FileLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info || (m.target().starts_with("djcast") && m.level() <= log::Level::Debug)
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("{} {:<5} {}: {}\n", timestamp(), record.level(), record.target(), record.args());
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        let mut guard = self.file.lock().unwrap();
        if guard.as_ref().and_then(|f| f.metadata().ok()).is_some_and(|m| m.len() > MAX_BYTES) {
            *guard = None;
            self.rotate();
        }
        if guard.is_none() {
            *guard = Self::open(&self.path);
        }
        if let Some(f) = guard.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().unwrap().as_mut() {
            let _ = f.flush();
        }
    }
}

/// UTC "YYYY-MM-DD HH:MM:SS".
fn timestamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

/// Install the logger; returns the logs folder.
pub fn init(config_dir: &Path) -> PathBuf {
    let dir = config_dir.join("logs");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("djcast.log");
    let file = FileLogger::open(&path);
    let logger = FileLogger { path, file: Mutex::new(file) };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    dir
}

#[cfg(test)]
mod tests {
    #[test]
    fn timestamp_format() {
        let t = super::timestamp();
        assert_eq!(t.len(), 19, "{t}");
        assert!(t.starts_with("20"), "{t}");
    }
}
