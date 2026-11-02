//! Daily catalog snapshots: the first change of each day saves a copy
//! of the catalog in `snapshots/`, and the last 30 days are kept.

use rusqlite::Connection;

use crate::folder::SNAPSHOT_DIR;
use crate::Result;

pub const KEEP_DAYS: usize = 30;

/// Take today's snapshot if there is none yet. Returns true when one was taken.
pub fn daily(root: &std::path::Path, conn: &Connection, today: &str) -> Result<bool> {
    let dir = root.join(SNAPSHOT_DIR);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("catalog-{today}.sqlite"));
    if path.exists() {
        return Ok(false);
    }
    take(conn, &path)?;
    prune(&dir)?;
    Ok(true)
}

/// Write a consistent copy of the catalog to `path`.
pub fn take(conn: &Connection, path: &std::path::Path) -> Result<()> {
    let tmp = path.with_extension("sqlite.tmp");
    let _ = std::fs::remove_file(&tmp);
    conn.execute("VACUUM INTO ?1", [tmp.to_string_lossy()])?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn prune(dir: &std::path::Path) -> Result<()> {
    let mut snaps: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("catalog-") && n.ends_with(".sqlite"))
        .collect();
    snaps.sort();
    while snaps.len() > KEEP_DAYS {
        std::fs::remove_file(dir.join(snaps.remove(0)))?;
    }
    Ok(())
}
