//! Now-playing source that follows a text file written by another program, e.g. the live
//! tracklist of VirtualDJ or any tool that keeps "Artist - Title" in a file.
//!
//! The last non-empty line is the current track. A leading time ("21:05", "[21:05:33]",
//! "21:05 : ") is ignored, and the line is split at the first " - " into artist and title.

use crate::{Source, Track};
use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Only the end of the file is read: a tracklist grows for the whole night.
const TAIL: u64 = 64 * 1024;
/// A file last written longer ago than this when we start belongs to an earlier set.
const STALE_AT_START: Duration = Duration::from_secs(30 * 60);

pub struct TextFileSource {
    path: PathBuf,
    /// Modification time and size at the last read.
    seen: Option<(SystemTime, u64)>,
    current: Option<Track>,
}

impl TextFileSource {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            bail!("no text file chosen for now playing");
        }
        Ok(Self { path, seen: None, current: None })
    }
}

impl Source for TextFileSource {
    fn name(&self) -> &'static str {
        "text-file"
    }

    fn poll(&mut self) -> Result<Option<Track>> {
        let meta = std::fs::metadata(&self.path).with_context(|| format!("reading {}", self.path.display()))?;
        let stamp = (meta.modified()?, meta.len());
        if self.seen == Some(stamp) {
            return Ok(self.current.clone());
        }
        // On the first look, a file untouched for a long time is left over from an earlier
        // set (a tracklist from last night): wait until it changes.
        if self.seen.is_none() && stamp.0.elapsed().is_ok_and(|age| age > STALE_AT_START) {
            self.seen = Some(stamp);
            return Ok(None);
        }
        let mut file = File::open(&self.path).with_context(|| format!("opening {}", self.path.display()))?;
        let start = meta.len().saturating_sub(TAIL);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        // A cut in the middle of the file leaves a partial first line; it is never the last one.
        let text = decode(&bytes, start == 0);
        self.seen = Some(stamp);
        self.current = text.lines().rev().find_map(parse_line);
        Ok(self.current.clone())
    }
}

/// UTF-8 or UTF-16 (with a byte order mark, as Windows programs often write it).
fn decode(bytes: &[u8], from_start: bool) -> String {
    let utf16 = |b: &[u8], le: bool| {
        let units: Vec<u16> =
            b.as_chunks::<2>().0.iter().map(|&c| if le { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) }).collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] if from_start => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] if from_start => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] if from_start => String::from_utf8_lossy(rest).into_owned(),
        // The tail of a UTF-16 file: many zero bytes. Keep the reading aligned to whole units.
        _ if bytes.iter().filter(|b| **b == 0).count() > bytes.len() / 4 => {
            let le = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count() > bytes.len() / 4;
            utf16(&bytes[bytes.len() % 2..], le)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// "Artist - Title" from one line, without a leading time; None for a blank line.
fn parse_line(line: &str) -> Option<Track> {
    let rest = strip_time(line.trim());
    if rest.is_empty() {
        return None;
    }
    let (artist, title) = match rest.split_once(" - ") {
        Some((a, t)) => (a.trim(), t.trim()),
        None => ("", rest),
    };
    Some(Track { artist: artist.into(), title: title.into(), album: None, deck: None })
}

/// Remove "21:05", "21:05:33", "[21:05]" or "(21:05)" at the start, with a following
/// ":", "-" or "|" separator.
fn strip_time(s: &str) -> &str {
    let inner = s.strip_prefix(['[', '(']).unwrap_or(s);
    let digits_and_colons = inner.find(|c: char| !(c.is_ascii_digit() || c == ':')).unwrap_or(inner.len());
    let stamp = &inner[..digits_and_colons];
    let is_time = stamp.split(':').count() >= 2 && stamp.split(':').all(|p| (1..=2).contains(&p.len()));
    if !is_time {
        return s;
    }
    let after = inner[digits_and_colons..].strip_prefix([']', ')']).unwrap_or(&inner[digits_and_colons..]);
    after.trim_start().trim_start_matches([':', '-', '|']).trim_start()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(line: &str) -> Option<String> {
        parse_line(line).map(|t| format!("{} / {}", t.artist, t.title))
    }

    #[test]
    fn lines_with_and_without_times() {
        assert_eq!(shown("Daft Punk - One More Time").as_deref(), Some("Daft Punk / One More Time"));
        assert_eq!(shown("21:05 : Daft Punk - One More Time").as_deref(), Some("Daft Punk / One More Time"));
        assert_eq!(shown("[21:05:33] Daft Punk - One More Time").as_deref(), Some("Daft Punk / One More Time"));
        assert_eq!(shown("(1:05) - Daft Punk - One More Time").as_deref(), Some("Daft Punk / One More Time"));
        assert_eq!(shown("Just A Title").as_deref(), Some(" / Just A Title"));
        // A title that starts with digits is not a time.
        assert_eq!(shown("2000 Watts - 99 Luftballons").as_deref(), Some("2000 Watts / 99 Luftballons"));
        assert_eq!(shown("   "), None);
    }

    #[test]
    fn follows_the_last_line_and_only_rereads_on_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tracklist.txt");
        std::fs::write(&path, "21:00 : A - First\r\n21:04 : B - Second\r\n\r\n").unwrap();
        let mut src = TextFileSource::new(&path).unwrap();
        assert_eq!(src.poll().unwrap().unwrap().title, "Second");
        std::fs::write(&path, "21:00 : A - First\r\n21:04 : B - Second\r\n21:08 : C - Third\r\n").unwrap();
        assert_eq!(src.poll().unwrap().unwrap().title, "Third");
    }

    #[test]
    fn an_old_file_waits_for_the_next_track() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tracklist.txt");
        std::fs::write(&path, "A - Last night\n").unwrap();
        let hours_ago = SystemTime::now() - Duration::from_secs(5 * 3600);
        File::options().write(true).open(&path).unwrap().set_modified(hours_ago).unwrap();
        let mut src = TextFileSource::new(&path).unwrap();
        assert!(src.poll().unwrap().is_none());
        std::fs::write(&path, "A - Last night\nB - Tonight\n").unwrap();
        assert_eq!(src.poll().unwrap().unwrap().title, "Tonight");
    }

    #[test]
    fn reads_utf16_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("np.txt");
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("Beyoncé - Halo\r\n".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&path, bytes).unwrap();
        let t = TextFileSource::new(&path).unwrap().poll().unwrap().unwrap();
        assert_eq!((t.artist.as_str(), t.title.as_str()), ("Beyoncé", "Halo"));
    }

    #[test]
    fn missing_file_is_an_error() {
        let mut src = TextFileSource::new("does-not-exist.txt").unwrap();
        assert!(src.poll().is_err());
    }
}
