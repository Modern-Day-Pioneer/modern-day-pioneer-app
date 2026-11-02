//! Keeping the Journey's files and its catalog in step.
//!
//! - **Check at start and after folder changes** ([`Journey::check_files`]):
//!   a working file moved or renamed inside the Journey folder is found again
//!   by size and checksum and put back in its working place. Files that
//!   cannot be found are reported.
//! - **After each action** ([`Journey::sync_files`]): when an action removes a
//!   file's catalog row (deleting the Journey's copy, or undoing an import),
//!   the file is moved to `managed-content/removed/` and moved back if undo
//!   brings the row back. Undo lasts for the session, so removed copies are
//!   deleted for good the next time the Journey is opened. Nothing outside
//!   the Journey folder is ever touched.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use serde_json::Value;

use crate::folder::{self, CACHE_DIR, CONTENT_DIR, SNAPSHOT_DIR};
use crate::import::{checksum, set_readonly, HOLDING_DIR};
use crate::jobs::JobContext;
use crate::{Error, Journey, Result};

/// Where removed copies wait before they are deleted for good.
pub const REMOVED_DIR: &str = "removed";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileCheck {
    /// Working paths that were found elsewhere in the Journey folder and put back.
    pub relinked: Vec<String>,
    /// Working paths whose file could not be found.
    pub missing: Vec<String>,
}

fn walk(dir: &Path, skip: &HashSet<PathBuf>, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if skip.contains(&p) {
            continue;
        }
        // Links are never followed or taken: a link could lead outside the
        // Journey folder, and nothing outside it may be moved in.
        let Ok(t) = e.file_type() else { continue };
        if t.is_symlink() {
            continue;
        }
        if t.is_dir() {
            walk(&p, skip, out);
        } else if t.is_file() {
            out.push(p);
        }
    }
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// Move a file, clearing its read-only mark first and setting it again after.
fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = set_readonly(from, false);
    std::fs::rename(from, to)?;
    set_readonly(to, true)
}

impl Journey {
    /// Find working files that were moved or renamed inside the Journey
    /// folder and put them back. Run when the Journey opens and when folder
    /// watching sees a change. Also clears leftovers of an interrupted import.
    pub fn check_files(&mut self, ctx: &JobContext) -> Result<FileCheck> {
        let root = self.root().to_path_buf();
        let content = root.join(CONTENT_DIR);
        let holding = content.join(HOLDING_DIR);
        if holding.is_dir() {
            crate::import::clear_readonly_tree(&holding);
            let _ = std::fs::remove_dir_all(&holding);
        }

        let mut rows: Vec<(String, i64, String)> = Vec::new();
        {
            let mut st = self.conn().prepare("SELECT path, size, sha256 FROM content_file ORDER BY path")?;
            for r in st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))? {
                rows.push(r?);
            }
        }
        let mut report = FileCheck::default();
        let lost: Vec<&(String, i64, String)> = rows.iter().filter(|(p, _, _)| !folder::absolute(&root, p).is_file()).collect();
        if lost.is_empty() {
            return Ok(report);
        }
        // Candidates: every file in the Journey folder that no row points at,
        // apart from the app's own files.
        let known: HashSet<String> = rows.iter().map(|r| r.0.clone()).collect();
        let skip: HashSet<PathBuf> = [SNAPSHOT_DIR, CACHE_DIR].iter().map(|d| root.join(d)).chain([content.join(REMOVED_DIR)]).collect();
        let mut all = Vec::new();
        walk(&root, &skip, &mut all);
        let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        for p in all {
            let rel = rel_of(&root, &p);
            if known.contains(&rel) || is_app_file(&rel) || rel.ends_with("READ-ME.txt") {
                continue;
            }
            if let Ok(m) = std::fs::metadata(&p) {
                by_size.entry(m.len()).or_default().push(p);
            }
        }
        ctx.set_total(lost.len() as u64);
        let mut sums: HashMap<PathBuf, String> = HashMap::new();
        for (path, size, sha) in lost {
            ctx.check()?;
            let found = by_size.get(&(*size as u64)).and_then(|cands| {
                cands.iter().find(|c| {
                    let s = sums.entry((*c).clone()).or_insert_with(|| checksum(c).map(|x| x.0).unwrap_or_default());
                    s == sha
                })
            });
            match found.cloned() {
                Some(from) => {
                    move_file(&from, &folder::absolute(&root, path))?;
                    if let Some(c) = by_size.get_mut(&(*size as u64)) {
                        c.retain(|p| p != &from);
                    }
                    report.relinked.push(path.clone());
                }
                None => report.missing.push(path.clone()),
            }
            ctx.advance(format!("Checked {path}"));
        }
        Ok(report)
    }

    /// Bring the files in line with what one action did to `content_file`.
    pub(crate) fn sync_files(&mut self, action_id: &str) -> Result<()> {
        let root = self.root().to_path_buf();
        let removed_dir = root.join(CONTENT_DIR).join(REMOVED_DIR);
        let entries: Vec<_> = self.history_of(action_id)?.into_iter().filter(|e| e.table_name == "content_file" && e.field.is_none()).collect();
        // Content that got a new file in this action: a new version of a
        // transcript. Its earlier files stay where they are (sprint 5 default 9).
        let replaced: HashSet<String> = entries
            .iter()
            .filter_map(|e| match (&e.old_value, &e.new_value) {
                (None, Some(Value::Object(new))) => new.get("content_id").and_then(|v| v.as_str()).map(str::to_string),
                _ => None,
            })
            .collect();
        for e in entries {
            match (&e.old_value, &e.new_value) {
                // Row removed: put the file aside, unless another row still uses it.
                (Some(Value::Object(old)), None) => {
                    let Some(path) = old.get("path").and_then(|v| v.as_str()) else { continue };
                    if old.get("content_id").and_then(|v| v.as_str()).is_some_and(|c| replaced.contains(c)) {
                        continue;
                    }
                    let still: bool = self.conn().query_row("SELECT EXISTS(SELECT 1 FROM content_file WHERE path = ?1)", [path], |r| r.get(0))?;
                    let from = folder::absolute(&root, path);
                    if !still && from.is_file() {
                        move_file(&from, &folder::absolute(&removed_dir, path))?;
                    }
                }
                // Row back (undo): bring the file back from the removed folder.
                (None, Some(Value::Object(new))) => {
                    let Some(path) = new.get("path").and_then(|v| v.as_str()) else { continue };
                    let to = folder::absolute(&root, path);
                    let from = folder::absolute(&removed_dir, path);
                    if !to.is_file() && from.is_file() {
                        move_file(&from, &to)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Move waiting content to set aside, or set-aside content back to
    /// waiting, as one action. "In a memory" is set by adding the
    /// content to a memory, not here.
    pub fn set_content_state(&mut self, ids: &[String], state: &str) -> Result<()> {
        if !matches!(state, "waiting" | "set aside") {
            return Err(Error::Invalid(format!("Content can't be moved to \"{state}\" here.")));
        }
        self.act(|a| {
            for id in ids {
                let now: String = a.conn().query_row("SELECT state FROM content WHERE id = ?1", [id], |r| r.get(0))?;
                if now == "in a memory" {
                    return Err(Error::Invalid("This item is in a memory; take it out of the memory first.".into()));
                }
                a.update("content", crate::id_key(id), "state", state.into())?;
            }
            Ok(())
        })
    }

    /// Delete the Journey's copy of raw content (waiting or set aside), after
    /// the screen has asked. The original on the device is never touched; the
    /// copy waits in `managed-content/removed/` for 30 days and comes back on undo.
    pub fn delete_content(&mut self, ids: &[String]) -> Result<()> {
        // A transcript goes with its recording.
        let mut all: Vec<String> = ids.to_vec();
        for id in ids {
            if let Some(t) = crate::voice::transcript_id(self.conn(), id)? {
                all.push(t);
            }
        }
        let ids = &all;
        self.act(|a| {
            for id in ids {
                delete_one(a, id)?;
            }
            Ok(())
        })
    }

    /// Delete a recording made or kept in the app, with its transcript, even
    /// when it is in a memory (taken out first). One action, so Ctrl+Z brings
    /// it all back. Used by "Delete it" when finishing a recording.
    pub fn delete_recording(&mut self, id: &str) -> Result<()> {
        let kind: Option<String> = self.conn().query_row("SELECT kind FROM content WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        match kind.as_deref() {
            None => return Err(Error::Invalid("That recording is no longer in the Journey.".into())),
            Some("voice recording") => {}
            Some(_) => return Err(Error::Invalid("Only recordings can be deleted this way.".into())),
        }
        let transcript = crate::voice::transcript_id(self.conn(), id)?;
        let memories: Vec<String> = {
            let mut st = self.conn().prepare("SELECT memory_id FROM memory_content WHERE content_id = ?1")?;
            let v = st.query_map([id], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
            v
        };
        self.act(|a| {
            for m in &memories {
                crate::memories::take_out(a, m, id)?;
                crate::memories::refresh_status(a, m)?;
            }
            if let Some(t) = &transcript {
                delete_one(a, t)?;
            }
            delete_one(a, id)
        })
    }

    /// Delete a recording's transcript; its files wait in `removed/` like any
    /// deleted copy, and Ctrl+Z brings it back.
    pub fn delete_transcript(&mut self, recording: &str) -> Result<()> {
        let t = crate::voice::transcript_id(self.conn(), recording)?.ok_or_else(|| Error::Invalid("This recording has no transcript.".into()))?;
        self.act(|a| delete_one(a, &t))
    }
}

/// Delete one content item's rows and files (not in a memory).
fn delete_one(a: &mut crate::history::Action, id: &str) -> Result<()> {
    let exists: bool = a.conn().query_row("SELECT EXISTS(SELECT 1 FROM content WHERE id = ?1)", [id], |r| r.get(0))?;
    if !exists {
        return Ok(());
    }
    let in_memory: bool = a.conn().query_row(
        "SELECT state = 'in a memory' OR EXISTS(SELECT 1 FROM memory_content WHERE content_id = ?1) FROM content WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if in_memory {
        return Err(Error::Invalid("This item is in a memory; take it out of the memory first.".into()));
    }
    let files: Vec<String> = {
        let mut st = a.conn().prepare("SELECT path FROM content_file WHERE content_id = ?1")?;
        let v = st.query_map([id], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
        v
    };
    for path in files {
        let mut key = crate::Row::new();
        key.insert("content_id".into(), id.into());
        key.insert("path".into(), path.into());
        a.delete("content_file", key)?;
    }
    let rels: Vec<(String, String, String)> = {
        let mut st = a.conn().prepare("SELECT from_id, to_id, kind FROM content_relationship WHERE from_id = ?1 OR to_id = ?1")?;
        let v = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<std::result::Result<_, _>>()?;
        v
    };
    for (f, t, k) in rels {
        let mut key = crate::Row::new();
        key.insert("from_id".into(), f.into());
        key.insert("to_id".into(), t.into());
        key.insert("kind".into(), k.into());
        a.delete("content_relationship", key)?;
    }
    a.delete_item("content", id)
}

/// The app's own files at the top of the Journey folder.
fn is_app_file(rel: &str) -> bool {
    !rel.contains('/') && (rel.starts_with("catalog.") || rel == "context.jsonld" || rel == "settings.json")
}

/// Delete the copies removed in an earlier session (called when the Journey opens).
pub(crate) fn purge_removed(root: &Path) {
    let dir = root.join(CONTENT_DIR).join(REMOVED_DIR);
    if dir.is_dir() {
        crate::import::clear_readonly_tree(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
