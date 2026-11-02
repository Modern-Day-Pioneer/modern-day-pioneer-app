//! Memories: title, date, caption, ordered content with a cover, typed
//! notes, and people, pets and families linked with a role.
//!
//! Content enters the Journey only through a memory: `memory_content`
//! is the only link, and adding content sets it "in a memory". A memory with
//! no content is a placeholder ("needs content", never exported). Every
//! change is one action in history, so undo reverses it whole.

use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::{json, Value};

use crate::dates::{self, DateAsKnown};
use crate::people::{date_value, name_of, row, text_value};
use crate::summary::Entry;
use crate::{folder, id_key, Action, Error, Journey, Result, Row};

/// Roles for each kind of content.
pub const ROLES: &[(&str, &[&str])] = &[
    ("picture", &["in the picture", "taken by", "present but not pictured", "remembered", "mentioned in the caption"]),
    ("voice recording", &["speaking", "interviewing or asking", "listening or present", "intended audience", "mentioned", "recorded by"]),
    ("video", &["on camera", "speaking", "filmed by", "present but off camera", "intended audience", "mentioned"]),
    ("text", &["written by", "written to", "mentioned", "transcribed by"]),
];

/// A caption longer than this offers to become a typed note.
pub const LONG_CAPTION: usize = 300;

/// The role a waiting item gets when it is assigned to someone.
pub fn default_role(kind: &str) -> &'static str {
    match kind {
        "picture" => "in the picture",
        "voice recording" => "speaking",
        "video" => "on camera",
        _ => "written by",
    }
}

/// The combined role list for these kinds of content, plus "other".
/// With no content yet, every list is offered.
pub fn roles_for(kinds: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for (k, roles) in ROLES {
        if kinds.is_empty() || kinds.iter().any(|x| x == k) {
            for r in *roles {
                if !out.iter().any(|o| o == r) {
                    out.push(r.to_string());
                }
            }
        }
    }
    out.push("other".into());
    out
}

/// A person or pet with a few words that tell them apart from others with
/// the same name ("born 1912", "1912 to 1980"), for the assign box.
#[derive(Debug, Clone, Serialize)]
pub struct NameHint {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub hint: String,
}

fn year(edtf: &Option<String>) -> Option<String> {
    edtf.as_deref().map(|e| e.trim_start_matches(['~', '?'])).filter(|e| e.len() >= 4 && e[..4].chars().all(|c| c.is_ascii_digit())).map(|e| e[..4].to_string())
}

/// Everyone's names with their hints.
pub(crate) fn name_hints(c: &rusqlite::Connection) -> Result<Vec<NameHint>> {
    let mut st = c.prepare(
        "SELECT i.id, i.name, i.kind, i.birth_date_edtf, i.death_date_edtf, i.description FROM individual i ORDER BY i.name COLLATE NOCASE, i.birth_date_edtf",
    )?;
    let rows = st.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name, kind, born, died, desc) = row?;
        let hint = match (year(&born), year(&died)) {
            (Some(b), Some(d)) => format!("{b} to {d}"),
            (Some(b), None) => format!("born {b}"),
            (None, Some(d)) => format!("died {d}"),
            (None, None) => desc
                .map(|d| d.split_whitespace().take(6).collect::<Vec<_>>().join(" "))
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| "no dates yet".into()),
        };
        out.push(NameHint { id, name, kind: kind.to_lowercase(), hint });
    }
    let mut st = c.prepare("SELECT id, name FROM family ORDER BY name COLLATE NOCASE")?;
    for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (id, name) = r?;
        out.push(NameHint { id, name, kind: "family".into(), hint: "family".into() });
    }
    Ok(out)
}

/// Names typed with commas, to people and pets (sprint 12): a name in the
/// Journey (full name, other name, a first name only one person has, or
/// with its years in brackets: "Ada Hale (born 1912)") is that individual;
/// any other name is added as a new person; a name several share is left
/// out and listed in `unsure`.
fn resolve_names(a: &mut Action, names: &str) -> Result<(Vec<(String, String)>, Assigned)> {
    let mut wanted: Vec<String> = Vec::new();
    for n in names.split([',', ';']).map(|n| n.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|n| !n.is_empty()) {
        if !wanted.iter().any(|w| w.to_lowercase() == n.to_lowercase()) {
            wanted.push(n);
        }
    }
    if wanted.is_empty() {
        return Err(Error::Invalid("Type at least one name.".into()));
    }
    let mut out = Assigned::default();
    let mut who: Vec<(String, String)> = Vec::new();
    let hints = name_hints(a.conn())?;
    for typed in &wanted {
        // "Ada Hale (born 1912)" or "Ada Hale (1912)" picks among people with the same name.
        let (name, said) = match typed.strip_suffix(')').and_then(|t| t.rsplit_once('(')) {
            Some((n, h)) if !n.trim().is_empty() => (n.trim().to_string(), Some(h.trim().to_lowercase())),
            _ => (typed.clone(), None),
        };
        let name = &name;
        let mut found = crate::bulk::find_individual(a.conn(), name)?;
        if found.is_empty() {
            let mut st = a.conn().prepare("SELECT p.id, p.given_name FROM person p WHERE p.given_name IS NOT NULL")?;
            let key = crate::bulk::norm(name);
            for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (id, given) = r?;
                if crate::bulk::norm(&given) == key {
                    found.push(id);
                }
            }
            // A family by its name ("The Rowans").
            if found.is_empty() {
                let mut st = a.conn().prepare("SELECT id, name FROM family")?;
                for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                    let (id, fam) = r?;
                    if crate::bulk::norm(&fam) == key {
                        found.push(id);
                    }
                }
            }
        }
        if let (Some(said), true) = (&said, found.len() > 1) {
            let picked: Vec<String> = found
                .iter()
                .filter(|id| {
                    hints.iter().any(|h| {
                        h.id == **id && {
                            let hint = h.hint.to_lowercase();
                            hint == *said || (said.len() == 4 && said.chars().all(|c| c.is_ascii_digit()) && hint.contains(said.as_str()))
                        }
                    })
                })
                .cloned()
                .collect();
            if !picked.is_empty() {
                found = picked;
            }
        }
        match found.as_slice() {
            [one] => {
                if !who.iter().any(|w| w.0 == *one) {
                    who.push((one.clone(), name_of(a.conn(), one)?));
                }
            }
            [] => {
                let id = crate::bulk::add_person(a, name)?;
                out.added.push(name.clone());
                who.push((id, name.clone()));
            }
            _ => out.unsure.push(typed.clone()),
        }
    }
    Ok((who, out))
}

/// What [`Journey::assign_to_names`] did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Assigned {
    pub memory: String,
    /// Names added as new people.
    pub added: Vec<String>,
    /// Names several people share, left out.
    pub unsure: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub date: Option<String>,
    pub cover: bool,
    /// For a picture: quarter turns clockwise chosen in the app (sprint 8).
    pub turn: i64,
    /// For a recording or video: how its transcript was made ("automatic",
    /// "corrected", "checked" or "typed"; see `voice::made_by`), if it has one.
    pub transcript: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoleLink {
    /// The person, pet or family.
    pub id: String,
    pub name: String,
    /// "person", "pet" or "family".
    pub kind: String,
    pub role: String,
    pub role_note: Option<String>,
    pub probably: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryView {
    pub id: String,
    pub name: String,
    pub date: Option<DateAsKnown>,
    pub caption: Option<String>,
    /// "needs content" or "complete".
    pub status: String,
    pub place: Option<Entry>,
    pub event: Option<Entry>,
    /// True when a cover was chosen; otherwise the first item is the cover.
    pub cover_chosen: bool,
    pub items: Vec<MemoryItem>,
    pub people: Vec<RoleLink>,
    pub stories: Vec<Entry>,
    /// Roles offered for this memory's content.
    pub roles: Vec<String>,
    pub featured: bool,
    /// The caption is long enough to offer turning it into a typed note.
    pub long_caption: bool,
    /// What the memory could still use, in words
    /// such as "a date"; empty for a placeholder, which needs content first.
    pub could_add: Vec<String>,
    /// Marked finished: the app stops suggesting more for it.
    pub finished: bool,
}

/// What a memory with content could still use: a date, a
/// place, people, a caption or note, a story. Suggestions only; nothing is
/// required. A placeholder gets none, since it needs content first,
/// and neither does a memory marked finished.
pub fn could_add(conn: &rusqlite::Connection, memory: &str) -> Result<Vec<String>> {
    let (status, date, place, caption, people, text, story, finished): (String, bool, bool, bool, bool, bool, bool, bool) = conn.query_row(
        "SELECT m.status, m.date_edtf IS NOT NULL, m.place_id IS NOT NULL OR m.event_id IS NOT NULL,
                trim(coalesce(m.caption, '')) <> '',
                EXISTS(SELECT 1 FROM memory_individual mi WHERE mi.memory_id = m.id),
                EXISTS(SELECT 1 FROM memory_content mc JOIN content c ON c.id = mc.content_id WHERE mc.memory_id = m.id AND c.kind = 'text'),
                EXISTS(SELECT 1 FROM story_memory sm WHERE sm.memory_id = m.id), m.finished IS NOT NULL
         FROM memory m WHERE m.id = ?1",
        [memory],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
    )?;
    if status == "needs content" || finished {
        return Ok(vec![]);
    }
    Ok([(!date, "a date"), (!place, "a place"), (!people, "people"), (!caption && !text, "a caption"), (!story, "a story")]
        .into_iter()
        .filter(|(missing, _)| *missing)
        .map(|(_, w)| w.to_string())
        .collect())
}

pub(crate) fn exists(conn: &rusqlite::Connection, table: &str, id: &str) -> Result<bool> {
    Ok(conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1)"), [id], |r| r.get(0))?)
}

fn need_memory(conn: &rusqlite::Connection, id: &str) -> Result<()> {
    if exists(conn, "memory", id)? {
        Ok(())
    } else {
        Err(Error::Invalid("That memory is no longer in the Journey.".into()))
    }
}

pub(crate) fn strings(conn: &rusqlite::Connection, sql: &str, arg: &str) -> Result<Vec<String>> {
    let mut st = conn.prepare(sql)?;
    let out = st.query_map([arg], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
    Ok(out)
}

fn kinds_in(conn: &rusqlite::Connection, memory: &str) -> Result<Vec<String>> {
    strings(conn, "SELECT DISTINCT c.kind FROM memory_content mc JOIN content c ON c.id = mc.content_id WHERE mc.memory_id = ?1", memory)
}

fn individual_or_family(conn: &rusqlite::Connection, id: &str) -> Result<String> {
    let t: Option<String> = conn.query_row("SELECT table_name FROM item WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    match t.as_deref() {
        Some("individual") | Some("family") => Ok(t.unwrap()),
        Some(_) => Err(Error::Invalid("Only people, pets and families can be linked with a role.".into())),
        None => Err(Error::Invalid("That person is no longer in the Journey.".into())),
    }
}

fn pair(k1: &str, v1: &str, k2: &str, v2: &str) -> Row {
    let mut r = Row::new();
    r.insert(k1.into(), Value::from(v1));
    r.insert(k2.into(), Value::from(v2));
    r
}

/// Number the rows of an ordered list 0, 1, 2 ... in their present order.
pub(crate) fn renumber(a: &mut Action, table: &str, owner_col: &str, owner: &str, item_col: &str) -> Result<()> {
    let items = strings(a.conn(), &format!("SELECT {item_col} FROM {table} WHERE {owner_col} = ?1 ORDER BY position, rowid"), owner)?;
    for (i, it) in items.iter().enumerate() {
        a.update(table, pair(owner_col, owner, item_col, it), "position", json!(i as i64))?;
    }
    Ok(())
}

/// Move one row of an ordered list up (`delta` < 0) or down.
pub(crate) fn move_in_list(a: &mut Action, table: &str, owner_col: &str, owner: &str, item_col: &str, item: &str, delta: i64) -> Result<()> {
    renumber(a, table, owner_col, owner, item_col)?;
    let items = strings(a.conn(), &format!("SELECT {item_col} FROM {table} WHERE {owner_col} = ?1 ORDER BY position"), owner)?;
    let from = items.iter().position(|i| i == item).ok_or_else(|| Error::Invalid("That item isn't in this list.".into()))? as i64;
    let to = (from + delta).clamp(0, items.len() as i64 - 1);
    if to == from {
        return Ok(());
    }
    let mut order = items.clone();
    let it = order.remove(from as usize);
    order.insert(to as usize, it);
    for (i, it) in order.iter().enumerate() {
        a.update(table, pair(owner_col, owner, item_col, it), "position", json!(i as i64))?;
    }
    Ok(())
}

/// Keep a memory's status in step with its content.
pub(crate) fn refresh_status(a: &mut Action, memory: &str) -> Result<()> {
    let n: i64 = a.conn().query_row("SELECT count(*) FROM memory_content WHERE memory_id = ?1", [memory], |r| r.get(0))?;
    let status = if n == 0 { "needs content" } else { "complete" };
    a.update("memory", id_key(memory), "status", json!(status))
}

/// Put content into a memory at the end, and mark it "in a memory".
pub(crate) fn put_in(a: &mut Action, memory: &str, content: &str) -> Result<()> {
    let state: Option<String> = a.conn().query_row("SELECT state FROM content WHERE id = ?1", [content], |r| r.get(0)).optional()?;
    if state.is_none() {
        return Err(Error::Invalid("That item is no longer in the Journey.".into()));
    }
    let already: bool = a.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM memory_content WHERE memory_id = ?1 AND content_id = ?2)",
        [memory, content],
        |r| r.get(0),
    )?;
    if already {
        return Ok(());
    }
    let next: i64 = a.conn().query_row("SELECT coalesce(max(position) + 1, 0) FROM memory_content WHERE memory_id = ?1", [memory], |r| r.get(0))?;
    a.insert("memory_content", row(json!({"memory_id": memory, "content_id": content, "position": next})))?;
    a.update("content", id_key(content), "state", json!("in a memory"))
}

/// Take content out of a memory. Content in no other memory goes back to waiting.
pub(crate) fn take_out(a: &mut Action, memory: &str, content: &str) -> Result<()> {
    a.delete("memory_content", row(json!({"memory_id": memory, "content_id": content})))?;
    let cover: Option<String> = a.conn().query_row("SELECT cover_id FROM memory WHERE id = ?1", [memory], |r| r.get(0))?;
    if cover.as_deref() == Some(content) {
        a.update("memory", id_key(memory), "cover_id", Value::Null)?;
    }
    let elsewhere: bool = a.conn().query_row("SELECT EXISTS(SELECT 1 FROM memory_content WHERE content_id = ?1)", [content], |r| r.get(0))?;
    if !elsewhere {
        a.update("content", id_key(content), "state", json!("waiting"))?;
    }
    Ok(())
}

fn link_key(memory: &str, who: &str, role: &str) -> Row {
    row(json!({"memory_id": memory, "individual_id": who, "role": role}))
}

fn check_role(role: &str, note: Option<&str>) -> Result<(String, Value)> {
    let role = role.trim().to_lowercase();
    if role.is_empty() {
        return Err(Error::Invalid("Choose a role.".into()));
    }
    let note = text_value(note.map(Value::from).as_ref());
    if role == "other" && note.is_null() {
        return Err(Error::Invalid("Say in a few words what \"other\" means here.".into()));
    }
    if role != "other" && !ROLES.iter().any(|(_, rs)| rs.contains(&role.as_str())) {
        return Err(Error::Invalid(format!("\"{role}\" isn't one of the roles. Choose \"other\" and describe it instead.")));
    }
    let note = if role == "other" { note } else { Value::Null };
    Ok((role, note))
}

fn memory_title(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(Error::Invalid("A memory needs a title.".into()));
    }
    Ok(n.to_string())
}

pub(crate) fn create_memory(a: &mut Action, name: &str) -> Result<String> {
    a.create("memory", row(json!({"name": name, "status": "needs content"})))
}

impl Journey {
    /// Add a memory with just a title. With `story`, it goes at the end of that story.
    pub fn add_memory(&mut self, name: &str, story: Option<&str>) -> Result<String> {
        let name = memory_title(name)?;
        if let Some(s) = story {
            if !exists(self.conn(), "story", s)? {
                return Err(Error::Invalid("That story is no longer in the Journey.".into()));
            }
        }
        self.act(|a| {
            let id = create_memory(a, &name)?;
            if let Some(s) = story {
                crate::stories::append_memory(a, s, &id)?;
            }
            Ok(id)
        })
    }

    /// Change a memory: `name`, `date` (typed as known), `caption`,
    /// `place_id`, `event_id` and `cover_id` (empty to clear), and `finished`
    /// (true or false; a placeholder can't be finished).
    pub fn update_memory(&mut self, id: &str, changes: &Row) -> Result<()> {
        need_memory(self.conn(), id)?;
        let mut date = None;
        for (k, v) in changes {
            let s = v.as_str().unwrap_or_default();
            match k.as_str() {
                "name" => {
                    memory_title(s)?;
                }
                "caption" => {}
                "finished" => {
                    let status: String = self.conn().query_row("SELECT status FROM memory WHERE id = ?1", [id], |r| r.get(0))?;
                    if v.as_bool() == Some(true) && status == "needs content" {
                        return Err(Error::Invalid("Add a photo, recording or note first; a memory with nothing in it can't be finished.".into()));
                    }
                }
                "date" => date = Some(dates::parse(s)?),
                "place_id" | "event_id" => {
                    let t = if k == "place_id" { "place" } else { "event" };
                    if !s.is_empty() && !exists(self.conn(), t, s)? {
                        return Err(Error::Invalid(format!("That {t} is no longer in the Journey.")));
                    }
                }
                "cover_id" => {
                    let inside: bool = self.conn().query_row(
                        "SELECT EXISTS(SELECT 1 FROM memory_content WHERE memory_id = ?1 AND content_id = ?2)",
                        [id, s],
                        |r| r.get(0),
                    )?;
                    if !s.is_empty() && !inside {
                        return Err(Error::Invalid("The cover must be one of the memory's own items.".into()));
                    }
                    let kind: String = if s.is_empty() {
                        String::new()
                    } else {
                        self.conn().query_row("SELECT kind FROM content WHERE id = ?1", [s], |r| r.get(0))?
                    };
                    if !s.is_empty() && kind != "picture" && kind != "video" {
                        return Err(Error::Invalid("Only a picture or video can be the cover.".into()));
                    }
                }
                _ => return Err(Error::Invalid(format!("{k} can't be changed on a memory."))),
            }
        }
        self.act(|a| {
            for (k, v) in changes {
                match k.as_str() {
                    "date" => {
                        let d = date.clone().flatten();
                        a.update("memory", id_key(id), "date_edtf", json!(d.as_ref().map(|d| d.edtf.clone())))?;
                        a.update("memory", id_key(id), "date", json!(d.and_then(|d| d.nearest)))?;
                    }
                    // The day it was marked finished, or nothing.
                    "finished" => a.update("memory", id_key(id), "finished", json!(v.as_bool().unwrap_or(false).then(|| chrono::Local::now().format("%Y-%m-%d").to_string())))?,
                    "caption" => {
                        // Keep the caption's own line breaks; only trim the ends.
                        a.update("memory", id_key(id), "caption", text_value(Some(v)))?;
                    }
                    f => a.update("memory", id_key(id), f, text_value(Some(v)))?,
                }
            }
            Ok(())
        })
    }

    pub fn memory_view(&self, id: &str) -> Result<MemoryView> {
        let c = self.conn();
        type Base = (String, Option<String>, Option<String>, String, Option<String>, Option<String>, Option<String>);
        let base: Option<Base> = c
            .query_row("SELECT name, date_edtf, caption, status, place_id, event_id, cover_id FROM memory WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
            })
            .optional()?;
        let (name, date, caption, status, place, event, cover) =
            base.ok_or_else(|| Error::Invalid("That memory is no longer in the Journey.".into()))?;
        let mut st = c.prepare(
            "SELECT c.id, c.name, c.kind, c.date,
                    (SELECT coalesce(t.review_status, 'automatic') FROM content_relationship r JOIN text_content t ON t.id = r.from_id
                      WHERE r.to_id = c.id AND r.kind = 'transcript of' LIMIT 1),
                    (SELECT t.transcribed_by FROM content_relationship r JOIN text_content t ON t.id = r.from_id
                      WHERE r.to_id = c.id AND r.kind = 'transcript of' LIMIT 1),
                    (SELECT coalesce(p.turn, 0) FROM picture p WHERE p.id = c.id)
             FROM memory_content mc JOIN content c ON c.id = mc.content_id
             WHERE mc.memory_id = ?1 ORDER BY mc.position, mc.rowid",
        )?;
        let mut items: Vec<MemoryItem> = st
            .query_map([id], |r| {
                let status: Option<String> = r.get(4)?;
                let by: Option<String> = r.get(5)?;
                let transcript = status.as_deref().map(|s| crate::voice::made_by(Some(s), by.as_deref()).to_string());
                Ok(MemoryItem { id: r.get(0)?, name: r.get(1)?, kind: r.get(2)?, date: r.get(3)?, cover: false, turn: r.get::<_, Option<i64>>(6)?.unwrap_or(0), transcript })
            })?
            .collect::<std::result::Result<_, _>>()?;
        // Only a picture or video can be the cover (sprint 5 feedback); unless
        // one was chosen it is the first picture, else the first video.
        let can_cover = |i: &MemoryItem| i.kind == "picture" || i.kind == "video";
        let cover_at = cover
            .as_ref()
            .and_then(|cv| items.iter().position(|i| &i.id == cv))
            .or_else(|| items.iter().position(|i| i.kind == "picture"))
            .or_else(|| items.iter().position(can_cover));
        if let Some(i) = cover_at.and_then(|at| items.get_mut(at)) {
            i.cover = true;
        }
        let mut st = c.prepare(
            "SELECT mi.individual_id, coalesce(i.name, f.name), CASE WHEN f.id IS NOT NULL THEN 'family' WHEN i.kind = 'Pet' THEN 'pet' ELSE 'person' END,
                    mi.role, mi.role_note, mi.certainty = 'probably'
             FROM memory_individual mi LEFT JOIN individual i ON i.id = mi.individual_id LEFT JOIN family f ON f.id = mi.individual_id
             WHERE mi.memory_id = ?1 ORDER BY mi.rowid",
        )?;
        let people = st
            .query_map([id], |r| {
                Ok(RoleLink {
                    id: r.get(0)?,
                    name: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    kind: r.get(2)?,
                    role: r.get(3)?,
                    role_note: r.get(4)?,
                    probably: r.get::<_, Option<bool>>(5)?.unwrap_or(false),
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        let entry = |table: &str, eid: Option<String>| -> Result<Option<Entry>> {
            Ok(match eid {
                Some(e) => Some(Entry { name: name_of(c, &e)?, id: e, note: table.into() }),
                None => None,
            })
        };
        let kinds = kinds_in(c, id)?;
        Ok(MemoryView {
            id: id.to_string(),
            name,
            date: date_value(date),
            long_caption: caption.as_ref().is_some_and(|t| t.chars().count() > LONG_CAPTION),
            caption,
            status,
            place: entry("place", place)?,
            event: entry("event", event)?,
            cover_chosen: cover.is_some(),
            items,
            people,
            stories: crate::people::list(
                c,
                "SELECT s.id, s.name, '' FROM story_memory sm JOIN story s ON s.id = sm.story_id WHERE sm.memory_id = ?1 ORDER BY s.name",
                id,
            )?,
            roles: roles_for(&kinds),
            featured: c.query_row("SELECT EXISTS(SELECT 1 FROM featured WHERE item_id = ?1)", [id], |r| r.get(0))?,
            could_add: could_add(c, id)?,
            finished: c.query_row("SELECT finished IS NOT NULL FROM memory WHERE id = ?1", [id], |r| r.get(0))?,
        })
    }

    /// Add content to a memory, at the end, in the order given.
    pub fn add_to_memory(&mut self, memory: &str, content: &[String]) -> Result<()> {
        need_memory(self.conn(), memory)?;
        self.act(|a| {
            for c in content {
                put_in(a, memory, c)?;
            }
            refresh_status(a, memory)
        })
    }

    /// Take content out of a memory (it returns to waiting unless another memory holds it).
    pub fn remove_from_memory(&mut self, memory: &str, content: &str) -> Result<()> {
        need_memory(self.conn(), memory)?;
        self.act(|a| {
            take_out(a, memory, content)?;
            renumber(a, "memory_content", "memory_id", memory, "content_id")?;
            refresh_status(a, memory)
        })
    }

    /// Move a piece of content earlier (`delta` < 0) or later in its memory.
    pub fn move_in_memory(&mut self, memory: &str, content: &str, delta: i64) -> Result<()> {
        need_memory(self.conn(), memory)?;
        self.act(|a| move_in_list(a, "memory_content", "memory_id", memory, "content_id", content, delta))
    }

    /// Make a typed note (a `.txt` text item) and put it in the memory.
    /// With `from_caption`, the caption is cleared in the same action.
    pub fn add_note(&mut self, memory: &str, title: &str, text: &str, from_caption: bool) -> Result<String> {
        need_memory(self.conn(), memory)?;
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::Invalid("The note is empty.".into()));
        }
        let title = if title.trim().is_empty() {
            let first: String = text.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
            format!("Note: {first}")
        } else {
            title.trim().to_string()
        };
        let root = self.root().to_path_buf();
        let author = self.author().to_string();
        let mut written: Option<std::path::PathBuf> = None;
        let result = self.act(|a| {
            let id = a.create("content", row(json!({"kind": "text", "name": title, "added_by": author, "author": author,
                "source_note": "Typed in the app"})))?;
            a.insert("text_content", row(json!({"id": id, "language": "en"})))?;
            let rel = folder::working_path("text", &id, None, "txt");
            let abs = folder::absolute(&root, &rel);
            if let Some(p) = abs.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&abs, text.replace("\r\n", "\n").replace('\n', "\r\n"))?;
            written = Some(abs.clone());
            crate::import::set_readonly(&abs, true)?;
            let (sha, size) = crate::import::checksum(&abs)?;
            a.insert("content_file", row(json!({"content_id": id, "is_copy": false, "path": rel, "format": "text/plain",
                "size": size as i64, "sha256": sha, "original_name": format!("{title}.txt")})))?;
            put_in(a, memory, &id)?;
            if from_caption {
                a.update("memory", id_key(memory), "caption", Value::Null)?;
            }
            refresh_status(a, memory)?;
            Ok(id)
        });
        if result.is_err() {
            if let Some(p) = written {
                let _ = crate::import::set_readonly(&p, false);
                let _ = std::fs::remove_file(p);
            }
        }
        result
    }

    /// Link a person, pet or family to a memory with a role.
    pub fn link_to_memory(&mut self, memory: &str, who: &str, role: &str, role_note: Option<&str>, probably: bool) -> Result<()> {
        need_memory(self.conn(), memory)?;
        individual_or_family(self.conn(), who)?;
        let (role, note) = check_role(role, role_note)?;
        let taken: bool = self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_individual WHERE memory_id = ?1 AND individual_id = ?2 AND role = ?3)",
            [memory, who, &role],
            |r| r.get(0),
        )?;
        if taken {
            return Err(Error::Invalid(format!("{} already has that role here.", name_of(self.conn(), who)?)));
        }
        self.act(|a| {
            a.insert(
                "memory_individual",
                row(json!({"memory_id": memory, "individual_id": who, "role": role, "role_note": note,
                           "certainty": if probably { "probably" } else { "certain" }})),
            )
        })
    }

    /// The memory screen's "Who" box (sprint 12): link several people by
    /// name, as [`Journey::assign_to_names`] reads them, each with the same
    /// role. People already linked with that role are passed over. One action.
    pub fn link_names_to_memory(&mut self, memory: &str, names: &str, role: &str, role_note: Option<&str>, probably: bool) -> Result<Assigned> {
        need_memory(self.conn(), memory)?;
        let (role, note) = check_role(role, role_note)?;
        self.act(|a| {
            let (who, mut out) = resolve_names(a, names)?;
            if who.is_empty() {
                return Err(Error::Invalid(format!("More than one person is called {}; choose the right one.", out.unsure.join(", "))));
            }
            for (w, _) in &who {
                let taken: bool = a.conn().query_row(
                    "SELECT EXISTS(SELECT 1 FROM memory_individual WHERE memory_id = ?1 AND individual_id = ?2 AND role = ?3)",
                    [memory, w, &role],
                    |r| r.get(0),
                )?;
                if !taken {
                    a.insert(
                        "memory_individual",
                        row(json!({"memory_id": memory, "individual_id": w, "role": role, "role_note": note,
                                   "certainty": if probably { "probably" } else { "certain" }})),
                    )?;
                }
            }
            out.memory = memory.to_string();
            Ok(out)
        })
    }

    /// Change a role link: a new role (and note), or the "probably" flag.
    pub fn update_memory_link(&mut self, memory: &str, who: &str, old_role: &str, role: &str, role_note: Option<&str>, probably: bool) -> Result<()> {
        need_memory(self.conn(), memory)?;
        let (role, note) = check_role(role, role_note)?;
        let found: bool = self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_individual WHERE memory_id = ?1 AND individual_id = ?2 AND role = ?3)",
            [memory, who, old_role],
            |r| r.get(0),
        )?;
        if !found {
            return Err(Error::Invalid("That link is no longer there.".into()));
        }
        self.act(|a| {
            let certainty = json!(if probably { "probably" } else { "certain" });
            if role == old_role {
                a.update("memory_individual", link_key(memory, who, &role), "role_note", note)?;
                a.update("memory_individual", link_key(memory, who, &role), "certainty", certainty)
            } else {
                // The role is part of the link's key, so a new role is a new link.
                a.delete("memory_individual", link_key(memory, who, old_role))?;
                a.insert("memory_individual", row(json!({"memory_id": memory, "individual_id": who, "role": role,
                    "role_note": note, "certainty": certainty})))
            }
        })
    }

    pub fn unlink_from_memory(&mut self, memory: &str, who: &str, role: &str) -> Result<()> {
        self.act(|a| a.delete("memory_individual", link_key(memory, who, role)))
    }

    /// From the Waiting screen: put items into a new memory with this title.
    pub fn new_memory_with(&mut self, name: &str, content: &[String], story: Option<&str>) -> Result<String> {
        let name = memory_title(name)?;
        if let Some(s) = story {
            if !exists(self.conn(), "story", s)? {
                return Err(Error::Invalid("That story is no longer in the Journey.".into()));
            }
        }
        self.act(|a| {
            let id = create_memory(a, &name)?;
            for c in content {
                put_in(a, &id, c)?;
            }
            refresh_status(a, &id)?;
            if let Some(s) = story {
                crate::stories::append_memory(a, s, &id)?;
            }
            Ok(id)
        })
    }

    /// From the Waiting screen: assign items to a person or pet. This makes a
    /// memory about them holding the items, with a role by the first item's
    /// kind. The person links to the memory, never to the
    /// content itself.
    pub fn assign_to_individual(&mut self, who: &str, content: &[String]) -> Result<String> {
        if content.is_empty() {
            return Err(Error::Invalid("Choose at least one item first.".into()));
        }
        individual_or_family(self.conn(), who)?;
        let who_name = name_of(self.conn(), who)?;
        let (first_name, first_kind): (String, String) = self
            .conn()
            .query_row("SELECT name, kind FROM content WHERE id = ?1", [&content[0]], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .ok_or_else(|| Error::Invalid("That item is no longer in the Journey.".into()))?;
        let title = if content.len() == 1 {
            format!("{who_name}: {}", readable(&first_name))
        } else {
            format!("{who_name}: {} items", content.len())
        };
        self.act(|a| {
            let id = create_memory(a, &title)?;
            for c in content {
                put_in(a, &id, c)?;
            }
            refresh_status(a, &id)?;
            a.insert(
                "memory_individual",
                row(json!({"memory_id": id, "individual_id": who, "role": default_role(&first_kind), "certainty": "certain"})),
            )?;
            Ok(id)
        })
    }

    /// From the Waiting screen (sprint 12): assign items to several people
    /// typed as names separated by commas. A name already in the Journey
    /// (full name, other name, or a first name only one person has) uses
    /// that person or pet; any other name is added as a new person at once.
    /// A name several people share is left out and listed. One memory holds
    /// the items with everyone in it, in one action.
    /// Names of people and pets with what tells same-named ones apart.
    pub fn name_hints(&self) -> Result<Vec<NameHint>> {
        name_hints(self.conn())
    }

    pub fn assign_to_names(&mut self, names: &str, content: &[String]) -> Result<Assigned> {
        if content.is_empty() {
            return Err(Error::Invalid("Choose at least one item first.".into()));
        }
        let (first_name, first_kind): (String, String) = self
            .conn()
            .query_row("SELECT name, kind FROM content WHERE id = ?1", [&content[0]], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .ok_or_else(|| Error::Invalid("That item is no longer in the Journey.".into()))?;
        self.act(|a| {
            let (who, mut out) = resolve_names(a, names)?;
            if who.is_empty() {
                return Err(Error::Invalid(format!("More than one person is called {}; choose the right one.", out.unsure.join(", "))));
            }
            let names = match who.as_slice() {
                [one] => one.1.clone(),
                [one, two] => format!("{} and {}", one.1, two.1),
                [one, rest @ ..] => format!("{} and {} others", one.1, rest.len()),
                [] => String::new(),
            };
            let title = if content.len() == 1 { format!("{names}: {}", readable(&first_name)) } else { format!("{names}: {} items", content.len()) };
            let id = create_memory(a, &title)?;
            for c in content {
                put_in(a, &id, c)?;
            }
            refresh_status(a, &id)?;
            for (w, _) in &who {
                a.insert(
                    "memory_individual",
                    row(json!({"memory_id": id, "individual_id": w, "role": default_role(&first_kind), "certainty": "certain"})),
                )?;
            }
            out.memory = id;
            Ok(out)
        })
    }

    /// Every memory, with the stories it is in (or "Not in a story yet"),
    /// for the Stories screen and the pickers.
    pub fn memories(&self) -> Result<Vec<Entry>> {
        let mut st = self.conn().prepare(
            "SELECT m.id, m.name, (SELECT group_concat(s.name, ', ') FROM story_memory sm JOIN story s ON s.id = sm.story_id WHERE sm.memory_id = m.id),
                    m.date_edtf, m.status
             FROM memory m ORDER BY m.name COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| {
            let stories: Option<String> = r.get(2)?;
            let date: Option<String> = r.get(3)?;
            let status: String = r.get(4)?;
            let mut note = vec![stories.unwrap_or_else(|| "Not in a story yet".into())];
            if let Some(d) = date {
                note.push(dates::words(&d));
            }
            if status == "needs content" {
                note.push("needs content".into());
            }
            Ok(Entry { id: r.get(0)?, name: r.get(1)?, note: note.join(" · ") })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The Journey's copy of a piece of content to show large: the export
    /// copy when one was made (JPEG, MP3, MP4), otherwise the original
    ///. Returns the absolute path and its format.
    pub fn content_file_to_show(&self, id: &str) -> Result<Option<(std::path::PathBuf, String)>> {
        let f: Option<(String, String)> = self
            .conn()
            .query_row(
                "SELECT path, format FROM content_file WHERE content_id = ?1 AND format <> 'text/vtt' ORDER BY format = 'text/plain' DESC, is_copy DESC LIMIT 1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(f.map(|(p, fmt)| (folder::absolute(self.root(), &p), fmt)))
    }
}

/// "IMG_0412.jpg" reads as "IMG_0412" in a title.
fn readable(file_name: &str) -> String {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 4 => stem.to_string(),
        _ => file_name.to_string(),
    }
}

/// What removing a memory unlinks, in plain words.
pub(crate) fn memory_warning(c: &rusqlite::Connection, id: &str) -> Result<Vec<String>> {
    let mut out = vec![];
    let alone: i64 = c.query_row(
        "SELECT count(*) FROM memory_content mc WHERE mc.memory_id = ?1
           AND NOT EXISTS (SELECT 1 FROM memory_content o WHERE o.content_id = mc.content_id AND o.memory_id <> ?1)",
        [id],
        |r| r.get(0),
    )?;
    let shared: i64 = c.query_row("SELECT count(*) FROM memory_content WHERE memory_id = ?1", [id], |r| r.get::<_, i64>(0))? - alone;
    if alone > 0 {
        out.push(format!("{} back to waiting (nothing is deleted)", plural(alone, "item goes", "items go")));
    }
    if shared > 0 {
        out.push(format!("{} that also {} in another memory (they stay there)", plural(shared, "item", "items"), if shared == 1 { "sits" } else { "sit" }));
    }
    let who = strings(
        c,
        "SELECT DISTINCT coalesce(i.name, f.name) FROM memory_individual mi LEFT JOIN individual i ON i.id = mi.individual_id
         LEFT JOIN family f ON f.id = mi.individual_id WHERE mi.memory_id = ?1 ORDER BY 1",
        id,
    )?;
    if !who.is_empty() {
        out.push(format!("The roles of {} (they stay in the Journey)", who.join(", ")));
    }
    for s in strings(c, "SELECT s.name FROM story_memory sm JOIN story s ON s.id = sm.story_id WHERE sm.memory_id = ?1 ORDER BY s.name", id)? {
        out.push(format!("Its place in the story {s} (the story stays)"));
    }
    if c.query_row("SELECT EXISTS(SELECT 1 FROM featured WHERE item_id = ?1)", [id], |r| r.get::<_, bool>(0))? {
        out.push("Its place among the featured items on the home page".into());
    }
    Ok(out)
}

/// Remove a memory: its content returns to waiting unless another memory
/// holds it; roles, story places and the featured place go.
pub(crate) fn remove_memory(a: &mut Action, id: &str) -> Result<()> {
    for c in strings(a.conn(), "SELECT content_id FROM memory_content WHERE memory_id = ?1", id)? {
        take_out(a, id, &c)?;
    }
    let links: Vec<(String, String)> = {
        let mut st = a.conn().prepare("SELECT individual_id, role FROM memory_individual WHERE memory_id = ?1")?;
        st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?
    };
    for (who, role) in links {
        a.delete("memory_individual", link_key(id, &who, &role))?;
    }
    for s in strings(a.conn(), "SELECT story_id FROM story_memory WHERE memory_id = ?1", id)? {
        a.delete("story_memory", row(json!({"story_id": s, "memory_id": id})))?;
        renumber(a, "story_memory", "story_id", &s, "memory_id")?;
    }
    crate::stories::unfeature(a, id)?;
    a.delete_item("memory", id)
}

pub(crate) fn plural(n: i64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}
