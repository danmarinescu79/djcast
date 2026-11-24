//! Side outputs for now-playing changes: text files for OBS and a timestamped tracklist.

use crate::Track;
use anyhow::Result;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct NowPlayingFiles {
    dir: PathBuf,
    tracklist: PathBuf,
    started: Instant,
}

impl NowPlayingFiles {
    /// Creates `dir` and a new tracklist file for this session.
    pub fn new(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let tracklist = dir.join(format!("tracklist-{ts}.txt"));
        let me = Self { dir, tracklist, started: Instant::now() };
        me.write_current(None)?;
        Ok(me)
    }

    /// Update `nowplaying.txt`, `artist.txt`, `title.txt` and append to the tracklist.
    pub fn update(&self, track: &Track) -> Result<()> {
        self.write_current(Some(track))?;
        let mut f = OpenOptions::new().create(true).append(true).open(&self.tracklist)?;
        writeln!(f, "{} {}", format_elapsed(self.started.elapsed()), track.display())?;
        Ok(())
    }

    pub fn tracklist_path(&self) -> &Path {
        &self.tracklist
    }

    fn write_current(&self, track: Option<&Track>) -> Result<()> {
        let (np, artist, title) = match track {
            Some(t) => (t.display(), t.artist.as_str(), t.title.as_str()),
            None => (String::new(), "", ""),
        };
        fs::write(self.dir.join("nowplaying.txt"), np)?;
        fs::write(self.dir.join("artist.txt"), artist)?;
        fs::write(self.dir.join("title.txt"), title)?;
        Ok(())
    }
}

fn format_elapsed(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_files_and_tracklist() {
        let dir = tempfile::tempdir().unwrap();
        let out = NowPlayingFiles::new(dir.path()).unwrap();
        let t = Track { artist: "A".into(), title: "T".into(), album: None, deck: None };
        out.update(&t).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("nowplaying.txt")).unwrap(), "A - T");
        assert_eq!(fs::read_to_string(dir.path().join("title.txt")).unwrap(), "T");
        let tl = fs::read_to_string(out.tracklist_path()).unwrap();
        assert_eq!(tl, "00:00:00 A - T\n");
    }

    #[test]
    fn elapsed_format() {
        assert_eq!(format_elapsed(Duration::from_secs(3723)), "01:02:03");
    }
}
