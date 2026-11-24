//! Developer probe: decrypt the local master.db and print schema info + recent history.
use djcast_nowplaying::rekordbox::{self, MasterDb, history};

fn main() -> anyhow::Result<()> {
    let path = rekordbox::find_master_db()?;
    println!("db: {}", path.display());
    let t = std::time::Instant::now();
    let mut db = MasterDb::new(path, rekordbox::load_key(None)?);
    let conn = db.open()?;
    println!("decrypted in {:?}", t.elapsed());
    for table in ["djmdSongHistory", "djmdHistory"] {
        let cols: Vec<String> = conn
            .prepare(&format!("PRAGMA table_info({table})"))?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<Result<_, _>>()?;
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        println!("{table} ({n} rows): {}", cols.join(", "));
    }
    for e in history::recent(&conn, 5)? {
        println!("{} | {} - {} | {:?}", e.created_at, e.artist, e.title, e.album);
    }
    Ok(())
}
