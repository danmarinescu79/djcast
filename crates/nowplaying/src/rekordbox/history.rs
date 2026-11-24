//! rekordbox play history (`djmdSongHistory`).

use anyhow::Result;
use rusqlite::Connection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: String,
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub created_at: String,
}

/// Most recent history entries, newest first.
pub fn recent(conn: &Connection, limit: usize) -> Result<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT h.ID, COALESCE(a.Name, ''), COALESCE(c.Title, ''), al.Name, h.created_at
         FROM djmdSongHistory h
         LEFT JOIN djmdContent c ON c.ID = h.ContentID
         LEFT JOIN djmdArtist a ON a.ID = c.ArtistID
         LEFT JOIN djmdAlbum al ON al.ID = c.AlbumID
         WHERE COALESCE(h.rb_local_deleted, 0) = 0
         ORDER BY h.created_at DESC, h.rowid DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |r| {
        Ok(HistoryEntry {
            id: r.get(0)?,
            artist: r.get(1)?,
            title: r.get(2)?,
            album: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
