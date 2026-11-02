//! The rebuildable cache: `cache/` holds only what the app can make
//! again from the catalog. Today that is the name and title search index;
//! thumbnails join it in sprint 2. Deleting the folder loses nothing.

use rusqlite::{params, Connection};

use crate::folder::CACHE_DIR;
use crate::Result;

pub const SEARCH_FILE: &str = "search.sqlite";

/// Item tables searched by name or title, with the kind shown to people.
const SEARCHED: &[(&str, &str)] = &[
    ("individual", "person"),
    ("family", "family"),
    ("place", "place"),
    ("story", "story"),
    ("memory", "memory"),
    ("event", "event"),
];

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Hit {
    pub id: String,
    pub kind: String,
    pub name: String,
}

pub fn open(root: &std::path::Path) -> Result<Connection> {
    let dir = root.join(CACHE_DIR);
    std::fs::create_dir_all(&dir)?;
    let conn = Connection::open(dir.join(SEARCH_FILE))?;
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS names USING fts5(id UNINDEXED, kind UNINDEXED, name, tokenize = 'unicode61 remove_diacritics 2');",
    )?;
    Ok(conn)
}

/// Rebuild the whole index from the catalog.
pub fn rebuild(catalog: &Connection, cache: &Connection) -> Result<()> {
    let tx = cache.unchecked_transaction()?;
    tx.execute("DELETE FROM names", [])?;
    for (table, kind) in SEARCHED {
        let mut stmt = catalog.prepare(&format!("SELECT id, name, {} FROM {table}", kind_expr(table)))?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        for row in rows {
            let (id, name, k) = row?;
            let k = if k.is_empty() { kind.to_string() } else { k };
            tx.execute("INSERT INTO names (id, kind, name) VALUES (?1, ?2, ?3)", params![id, k, name])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn kind_expr(table: &str) -> &'static str {
    match table {
        "individual" => "CASE kind WHEN 'Pet' THEN 'pet' ELSE 'person' END",
        _ => "''",
    }
}

/// Every name of these kinds, A to Z, for a list shown before anything is
/// typed (sprint 5 feedback round 2).
pub fn browse(cache: &Connection, kinds: &[String], limit: usize) -> Result<Vec<Hit>> {
    let mut stmt = cache.prepare("SELECT id, kind, name FROM names WHERE kind IN (SELECT value FROM json_each(?1)) ORDER BY name COLLATE NOCASE LIMIT ?2")?;
    let hits = stmt.query_map(params![serde_json::to_string(kinds).map_err(|e| crate::Error::Invalid(e.to_string()))?, limit as i64], |r| {
        Ok(Hit { id: r.get(0)?, kind: r.get(1)?, name: r.get(2)? })
    })?;
    Ok(hits.collect::<std::result::Result<_, _>>()?)
}

/// Names and titles that start with each word typed, best matches first.
pub fn search(cache: &Connection, text: &str, limit: usize) -> Result<Vec<Hit>> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    if words.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt = cache.prepare("SELECT id, kind, name FROM names WHERE names MATCH ?1 ORDER BY rank LIMIT ?2")?;
    let hits = stmt.query_map(params![words.join(" "), limit as i64], |r| {
        Ok(Hit { id: r.get(0)?, kind: r.get(1)?, name: r.get(2)? })
    })?;
    Ok(hits.collect::<std::result::Result<_, _>>()?)
}
