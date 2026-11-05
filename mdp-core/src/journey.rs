//! Create and open a Journey.
//!
//! A Journey is one project folder. Every path inside it is relative, so the
//! folder can be moved to another drive and opened again.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

use crate::{format_user_version, format_version_text, Error, Result, FORMAT_MAJOR, FORMAT_MINOR};

pub const CATALOG_FILE: &str = "catalog.sqlite";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    ReadWrite,
    /// The catalog was written by a newer app with a higher major version.
    ReadOnly,
}

pub struct Journey {
    root: PathBuf,
    conn: Connection,
    id: String,
    mode: OpenMode,
    author: String,
    cache: Option<Connection>,
    /// Actions done since the Journey was opened, newest last (session undo).
    pub(crate) undo_stack: Vec<String>,
    /// Write the plain-text copy after a pause instead of after every action.
    defer_copy: bool,
    /// When the plain-text copy fell behind the catalog, if it has.
    copy_behind: Option<std::time::Instant>,
}

/// Marks a plain-text copy that lags the catalog, so the next open rewrites it
/// even if the app stopped before it could.
pub const COPY_BEHIND_FILE: &str = "catalog.jsonl.behind";

/// The device's author name, until the settings page sets one.
pub fn default_author() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Unknown".into())
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Refuse a catalog whose files point anywhere but the Journey's own
/// managed-content folder. The app never writes such a path, so one means the
/// catalog was damaged or made by hand, and opening it could read or move
/// files that are not part of the Journey.
fn check_paths(conn: &Connection) -> Result<()> {
    let mut st = conn.prepare("SELECT path FROM content_file")?;
    let bad: Option<String> = st.query_map([], |r| r.get::<_, String>(0))?.filter_map(|p| p.ok()).find(|p| !crate::folder::is_working_path(p));
    match bad {
        Some(p) => Err(Error::Invalid(format!(
            "This Journey can't be opened: its catalog lists a file outside the Journey folder ({p}). \
             The catalog may be damaged or may not have been made by this app."
        ))),
        None => Ok(()),
    }
}

impl Journey {
    /// Create a new Journey in `folder`, which must not exist or be empty.
    pub fn create(folder: &Path, name: &str) -> Result<Journey> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("A Journey needs a name.".into()));
        }
        if folder.exists() && std::fs::read_dir(folder)?.next().is_some() {
            return Err(Error::Invalid(format!("{} is not empty.", folder.display())));
        }
        std::fs::create_dir_all(folder)?;
        let mut conn = Connection::open(folder.join(CATALOG_FILE))?;
        configure(&conn)?;
        let tx = conn.transaction()?;
        tx.execute_batch(include_str!("schema.sql"))?;
        tx.pragma_update(None, "user_version", format_user_version(FORMAT_MAJOR, FORMAT_MINOR))?;
        tx.commit()?;
        let mut j = Journey {
            root: folder.to_path_buf(),
            conn,
            id: String::new(),
            mode: OpenMode::ReadWrite,
            author: default_author(),
            cache: None,
            undo_stack: Vec::new(),
            defer_copy: false,
            copy_behind: None,
        };
        crate::folder::prepare(folder)?;
        let at = now();
        j.id = j.act(|a| {
            let mut row = crate::history::Row::new();
            row.insert("name".into(), name.into());
            row.insert("language".into(), "en".into());
            row.insert("format_version".into(), format_version_text().into());
            row.insert("date_modified".into(), at.into());
            a.create("journey", row)
        })?;
        j.undo_stack.clear(); // creating the Journey is not undoable
        Ok(j)
    }

    /// Open an existing Journey folder.
    pub fn open(folder: &Path) -> Result<Journey> {
        let path = folder.join(CATALOG_FILE);
        if !path.is_file() {
            return Err(Error::Invalid(format!("No Journey found in {}.", folder.display())));
        }
        let conn = Connection::open(&path)?;
        configure(&conn)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let major = (version / 100) as u32;
        let mode = if major > FORMAT_MAJOR {
            OpenMode::ReadOnly
        } else {
            OpenMode::ReadWrite
        };
        if mode == OpenMode::ReadOnly {
            conn.execute_batch("PRAGMA query_only = ON;")?;
        }
        let id: Option<String> =
            conn.query_row("SELECT id FROM journey LIMIT 1", [], |r| r.get(0)).optional()?;
        let id = id.ok_or_else(|| Error::Invalid("The catalog has no Journey.".into()))?;
        check_paths(&conn)?;
        let mut j = Journey { root: folder.to_path_buf(), conn, id, mode, author: default_author(), cache: None, undo_stack: Vec::new(), defer_copy: false, copy_behind: None };
        if mode == OpenMode::ReadWrite {
            crate::folder::prepare(folder)?;
            crate::files::purge_removed(folder);
            j.upgrade(version)?;
            if folder.join(COPY_BEHIND_FILE).exists() || !folder.join(crate::jsonl::JSONL_FILE).exists() {
                j.copy_behind = Some(std::time::Instant::now());
                j.write_copy()?;
            }
        }
        j.open_cache()?;
        Ok(j)
    }

    /// Bring an older catalog up to this app's format, one step at a time,
    /// after saving a snapshot ("Format version and upgrade rule").
    fn upgrade(&mut self, version: i64) -> Result<()> {
        let current = format_user_version(FORMAT_MAJOR, FORMAT_MINOR);
        if version >= current {
            return Ok(());
        }
        let snap = self.root.join(crate::folder::SNAPSHOT_DIR).join(format!("catalog-before-format-{}.sqlite", format_version_text()));
        if !snap.exists() {
            crate::snapshot::take(&self.conn, &snap)?;
        }
        // 1.6 rebuilds the event table, which needs the links unchecked while it is swapped.
        if version < 106 {
            self.conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
        }
        let tx = self.conn.transaction()?;
        if version < 101 {
            // 1.1: GPS and camera details on pictures and videos (sprint 2).
            tx.execute_batch(
                "ALTER TABLE picture ADD COLUMN latitude REAL; ALTER TABLE picture ADD COLUMN longitude REAL;
                 ALTER TABLE picture ADD COLUMN camera TEXT; ALTER TABLE video ADD COLUMN latitude REAL;
                 ALTER TABLE video ADD COLUMN longitude REAL; ALTER TABLE video ADD COLUMN camera TEXT;",
            )?;
        }
        if version < 102 {
            // 1.2: a portrait for each person and pet, and a picture's turn (sprint 8).
            for (table, col, ty) in [("individual", "image_id", "TEXT REFERENCES item(id)"), ("picture", "turn", "INTEGER")] {
                add_column(&tx, table, col, ty)?;
            }
        }
        if version < 103 {
            // 1.3: a memory marked finished stops asking for more (sprint 9).
            add_column(&tx, "memory", "finished", "TEXT")?;
        }
        if version < 104 {
            // 1.4: merged IDs kept as aliases, and a recording's quiet start (sprint 10).
            add_column(&tx, "voice_recording", "quiet_start", "REAL")?;
            add_column(&tx, "voice_recording", "play_full", "INTEGER NOT NULL DEFAULT 0 CHECK (play_full IN (0, 1))")?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS alias (old_id TEXT PRIMARY KEY, item_id TEXT NOT NULL REFERENCES item(id), updated TEXT NOT NULL);")?;
        }
        if version < 105 {
            // 1.5: "In memory of" pages, a story kind with a note per memory (sprint 13).
            add_column(&tx, "story", "genre", "TEXT NOT NULL DEFAULT 'story' CHECK (genre IN ('story', 'tribute'))")?;
            add_column(&tx, "story_memory", "note", "TEXT")?;
        }
        if version < 106 {
            // 1.6: kinds of event the family adds (sprint 19).
            // SQLite can't loosen a CHECK in place, so the table is copied.
            tx.execute_batch(
                "CREATE TABLE event_new (
                   id TEXT PRIMARY KEY REFERENCES item(id),
                   kind TEXT NOT NULL CHECK (kind <> ''),
                   name TEXT NOT NULL CHECK (name <> ''),
                   start_date TEXT, start_date_edtf TEXT, end_date TEXT, end_date_edtf TEXT,
                   place_id TEXT REFERENCES place(id),
                   description TEXT,
                   updated TEXT NOT NULL);
                 INSERT INTO event_new (id, kind, name, start_date, start_date_edtf, end_date, end_date_edtf, place_id, description, updated)
                   SELECT id, kind, name, start_date, start_date_edtf, end_date, end_date_edtf, place_id, description, updated FROM event;
                 DROP TABLE event;
                 ALTER TABLE event_new RENAME TO event;",
            )?;
            let broken: i64 = tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
            if broken > 0 {
                drop(tx);
                self.conn.execute_batch("PRAGMA foreign_keys = ON;")?;
                return Err(Error::Invalid("The catalog's events could not be brought up to date; nothing was changed.".into()));
            }
        }
        tx.pragma_update(None, "user_version", current)?;
        tx.commit()?;
        self.conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let jid = self.id.clone();
        self.act(|a| {
            a.update("journey", crate::id_key(&jid), "format_version", format_version_text().into())
        })?;
        self.undo_stack.clear(); // an upgrade is not undoable
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn mode(&self) -> OpenMode {
        self.mode
    }
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn name(&self) -> Result<String> {
        Ok(self.conn.query_row("SELECT name FROM journey WHERE id = ?1", [&self.id], |r| r.get(0))?)
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn set_author(&mut self, author: &str) {
        let a = author.trim();
        if !a.is_empty() {
            self.author = a.to_string();
        }
    }

    /// Housekeeping before every action: the first change of the day saves a snapshot.
    pub(crate) fn before_change(&mut self) -> Result<()> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        crate::snapshot::daily(&self.root, &self.conn, &today)?;
        Ok(())
    }

    /// Housekeeping after every committed action: the plain-text copy and the cache.
    pub(crate) fn after_change(&mut self) -> Result<()> {
        if self.copy_behind.is_none() {
            if self.defer_copy {
                std::fs::write(self.root.join(COPY_BEHIND_FILE), b"")?;
            }
            self.copy_behind = Some(std::time::Instant::now());
        }
        if !self.defer_copy {
            self.write_copy()?;
        }
        self.open_cache()?;
        if let Some(c) = &self.cache {
            crate::cache::rebuild(&self.conn, c)?;
        }
        Ok(())
    }

    /// Write the plain-text copy after a pause rather than after every action
    ///. The app turns this on; it then calls
    /// `copy_due` now and then, and `write_copy` before anything leaves the app.
    pub fn defer_copy(&mut self, on: bool) {
        self.defer_copy = on;
    }

    /// The copy lags the catalog and nothing has changed for `quiet`.
    pub fn copy_due(&self, quiet: std::time::Duration) -> bool {
        self.copy_behind.is_some_and(|t| t.elapsed() >= quiet)
    }

    /// Bring the plain-text copy up to date, if it lags the catalog.
    pub fn write_copy(&mut self) -> Result<()> {
        if self.copy_behind.is_none() || self.mode == OpenMode::ReadOnly {
            return Ok(());
        }
        crate::jsonl::write(self)?;
        self.copy_behind = None;
        let _ = std::fs::remove_file(self.root.join(COPY_BEHIND_FILE));
        Ok(())
    }

    /// Open the cache, rebuilding it from the catalog when it is missing.
    fn open_cache(&mut self) -> Result<()> {
        let file = self.root.join(crate::folder::CACHE_DIR).join(crate::cache::SEARCH_FILE);
        if self.cache.is_some() && file.exists() {
            return Ok(());
        }
        self.cache = None;
        if self.mode == OpenMode::ReadOnly && !file.exists() {
            let c = Connection::open_in_memory()?;
            c.execute_batch("CREATE VIRTUAL TABLE names USING fts5(id UNINDEXED, kind UNINDEXED, name, tokenize = 'unicode61 remove_diacritics 2');")?;
            crate::cache::rebuild(&self.conn, &c)?;
            self.cache = Some(c);
            return Ok(());
        }
        let c = crate::cache::open(&self.root)?;
        crate::cache::rebuild(&self.conn, &c)?;
        self.cache = Some(c);
        Ok(())
    }

    /// Find people, pets, families, places, stories, memories and events by name or title.
    pub fn search(&mut self, text: &str, limit: usize) -> Result<Vec<crate::cache::Hit>> {
        self.open_cache()?;
        match &self.cache {
            Some(c) => crate::cache::search(c, text, limit),
            None => Ok(vec![]),
        }
    }

    /// Every name of these kinds ("person", "pet", "family", "memory", ...), A to Z.
    pub fn browse(&mut self, kinds: &[String], limit: usize) -> Result<Vec<crate::cache::Hit>> {
        self.open_cache()?;
        match &self.cache {
            Some(c) => crate::cache::browse(c, kinds, limit),
            None => Ok(vec![]),
        }
    }

    /// Close the cache connection, for example before its folder is deleted.
    pub fn close_cache(&mut self) {
        self.cache = None;
    }
}

impl Drop for Journey {
    /// Closing a Journey always leaves a current plain-text copy.
    fn drop(&mut self) {
        let _ = self.write_copy();
    }
}

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
    Ok(())
}

/// Add a column during an upgrade unless it is already there.
fn add_column(tx: &rusqlite::Transaction, table: &str, col: &str, ty: &str) -> Result<()> {
    let has: bool = tx.query_row(&format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE name = ?1"), [col], |r| r.get::<_, i64>(0))? > 0;
    if !has {
        tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {ty};"))?;
    }
    Ok(())
}
