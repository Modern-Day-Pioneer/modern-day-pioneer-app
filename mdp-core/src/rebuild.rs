//! Bring an exported history back in as a full rebuild (release 2, sprint
//! 11; asked for by the owner on 2026-10-31): people and pets, families,
//! relationships, places, events, stories, memories and their files, from
//! the export's `catalog.json` and files. The same way merges two Journeys:
//! export one and bring it into the other.
//!
//! - Every item keeps the export's ID, so an item already in the Journey
//!   (the same ID, or an ID it was merged into) is not added twice, and
//!   bringing the same export in again adds only what is missing.
//! - People, pets, places and families with the same name as one already in
//!   the Journey are offered as matches ([`look`]); a ticked match uses the
//!   one already there, so two Journeys merge without doubles.
//! - Files are copied and checked against the export's checksums, like any
//!   import; a file already in the Journey is not copied again.
//! - Everything is recorded in one action, so one undo takes it all back.
//!
//! What an export leaves out is not in its catalog, so it can't come back:
//! memories with nothing in them, living people's dates and exact places
//! when the export hid them, suggested relationships and memories'
//! "finished" marks.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::import::{row, Batch, Prepared};
use crate::jobs::JobContext;
use crate::journey::CATALOG_FILE;
use crate::{id_key, Action, Error, Journey, Result};

/// The export's catalog file.
pub const EXPORT_CATALOG: &str = "catalog.json";

/// Read an export's catalog.
pub fn read_catalog(dir: &Path) -> Result<Value> {
    let p = dir.join(EXPORT_CATALOG);
    let text = std::fs::read_to_string(&p).map_err(|e| Error::Invalid(format!("The export's catalog could not be read ({e}).")))?;
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| Error::Invalid(format!("The export's catalog is damaged ({e}).")))?;
    if v.get("@graph").and_then(|g| g.as_array()).is_none() {
        return Err(Error::Invalid("This doesn't look like a history exported by the app.".into()));
    }
    crate::check_not_newer(v["formatVersion"].as_str(), "export")?;
    Ok(v)
}

fn graph(cat: &Value) -> &[Value] {
    cat["@graph"].as_array().map(|a| a.as_slice()).unwrap_or(&[])
}

fn id_of(v: &Value) -> Option<String> {
    v.as_str().map(|s| s.trim_start_matches("mdpid:").to_string())
}

fn ty(n: &Value) -> &str {
    n["@type"].as_str().unwrap_or_default()
}

fn text(n: &Value, k: &str) -> Value {
    match &n[k] {
        Value::String(s) if !s.trim().is_empty() => Value::String(s.clone()),
        Value::Number(x) => Value::Number(x.clone()),
        _ => Value::Null,
    }
}

fn refs(n: &Value, k: &str) -> Vec<String> {
    match &n[k] {
        Value::Array(a) => a.iter().filter_map(id_of).collect(),
        v => id_of(v).into_iter().collect(),
    }
}

/// Content kind for a catalog type, or `None` for things that aren't content.
fn content_kind(n: &Value) -> Option<&'static str> {
    match ty(n) {
        "ImageObject" => Some("picture"),
        "AudioObject" => Some("voice recording"),
        "VideoObject" => Some("video"),
        "CreativeWork" if !matches!(n["genre"].as_str(), Some("story" | "tribute")) => Some("text"),
        _ => None,
    }
}

/// The file to bring in for a content item: its original, else its first copy.
fn file_of(n: &Value) -> Option<&Value> {
    let enc = n["encoding"].as_array()?;
    enc.iter().find(|e| e["copy"].as_bool() != Some(true)).or_else(|| enc.first())
}

fn table_for(n: &Value) -> Option<&'static str> {
    Some(match ty(n) {
        "Person" | "Pet" => "individual",
        "Place" => "place",
        "Family" => "family",
        "Event" => "event",
        "Memory" => "memory",
        "Relationship" => "individual_relationship",
        "CreativeWork" if matches!(n["genre"].as_str(), Some("story" | "tribute")) => "story",
        _ => content_kind(n).map(|_| "content")?,
    })
}

/// A person, pet, place or family in the export with the same name as one
/// already in the Journey.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Match {
    pub export_id: String,
    /// "person", "pet", "place" or "family".
    pub kind: String,
    pub name: String,
    pub journey_id: String,
}

/// What an export holds, before bringing it in.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Look {
    pub name: String,
    pub people: usize,
    pub places: usize,
    pub families: usize,
    pub events: usize,
    pub stories: usize,
    pub memories: usize,
    pub files: usize,
    /// Items whose ID is already in the Journey (an earlier import or the same Journey).
    pub already: usize,
    pub matches: Vec<Match>,
    /// Everything that can be chosen, in the export's order: people and
    /// pets, families, places, events, stories and memories not already here.
    pub items: Vec<Item>,
}

/// One thing in the export that the person can leave out.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Item {
    pub export_id: String,
    /// "person", "pet", "family", "place", "event", "story" or "memory".
    pub kind: String,
    pub name: String,
    /// A date, or how many files or memories it holds.
    pub detail: String,
    /// The one with the same name already in the Journey, if any.
    pub journey_id: Option<String>,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The content an import needs when `leave_out` is not brought in: what the
/// memories brought in hold, and what is made from that (transcripts, backs).
fn needed_content(cat: &Value, leave_out: &HashSet<String>) -> HashSet<String> {
    let mut need: HashSet<String> = graph(cat)
        .iter()
        .filter(|n| ty(n) == "Memory" && id_of(&n["@id"]).is_some_and(|id| !leave_out.contains(&id)))
        .flat_map(|n| refs(n, "contents"))
        .collect();
    loop {
        let before = need.len();
        for n in graph(cat).iter().filter(|n| content_kind(n).is_some()) {
            let Some(id) = id_of(&n["@id"]) else { continue };
            let made_from = ["transcriptOf", "mdp:captionsOf", "mdp:translationOf", "mdp:typedCopyOf", "mdp:backOf", "stillFrameOf", "mdp:clipOf", "mdp:editedVersionOf"]
                .iter()
                .any(|k| id_of(&n[*k]).is_some_and(|to| need.contains(&to)));
            if made_from {
                need.insert(id);
            }
        }
        if need.len() == before {
            return need;
        }
    }
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The ID this export ID stands for in the Journey already: itself, or the
/// item it was merged into (alias), when it is there as the same kind of item.
fn present(c: &Connection, id: &str, table: &str) -> Result<Option<String>> {
    let id = c.query_row("SELECT item_id FROM alias WHERE old_id = ?1", [id], |r| r.get::<_, String>(0)).optional()?.unwrap_or_else(|| id.to_string());
    let t: Option<String> = c.query_row("SELECT table_name FROM item WHERE id = ?1", [&id], |r| r.get(0)).optional()?;
    Ok(t.filter(|t| t == table).map(|_| id))
}

fn taken(c: &Connection, id: &str) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM item WHERE id = ?1) OR EXISTS(SELECT 1 FROM alias WHERE old_id = ?1)", [id], |r| r.get(0))?)
}

/// What the export holds, and the matches to offer.
pub fn look(j: &Journey, dir: &Path) -> Result<Look> {
    let cat = read_catalog(dir)?;
    let c = j.conn();
    let mut out = Look { name: cat["name"].as_str().unwrap_or_default().to_string(), ..Look::default() };
    let mut names: HashMap<(&str, String), Vec<String>> = HashMap::new();
    for (table, kind, sql) in [
        ("individual", "person", "SELECT id, name FROM individual WHERE kind = 'Person'"),
        ("individual", "pet", "SELECT id, name FROM individual WHERE kind = 'Pet'"),
        ("place", "place", "SELECT id, name FROM place"),
        ("family", "family", "SELECT id, name FROM family"),
    ] {
        let _ = table;
        let mut st = c.prepare(sql)?;
        for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (id, name) = r?;
            names.entry((kind, norm(&name))).or_default().push(id);
        }
    }
    for n in graph(&cat) {
        let Some(id) = id_of(&n["@id"]) else { continue };
        let Some(table) = table_for(n) else { continue };
        match ty(n) {
            "Person" | "Pet" => out.people += 1,
            "Place" => out.places += 1,
            "Family" => out.families += 1,
            "Event" => out.events += 1,
            "Memory" => out.memories += 1,
            "CreativeWork" if table == "story" => out.stories += 1,
            _ => {}
        }
        if table == "content" && file_of(n).is_some() {
            out.files += 1;
        }
        if present(c, &id, table)?.is_some() {
            out.already += 1;
            continue;
        }
        let kind = match (ty(n), table) {
            ("Person", _) => "person",
            ("Pet", _) => "pet",
            ("Place", _) => "place",
            ("Family", _) => "family",
            ("Event", _) => "event",
            ("Memory", _) => "memory",
            (_, "story") => "story",
            _ => continue,
        };
        let name = n["name"].as_str().unwrap_or_default();
        let words = |k: &str| n[k].as_str().filter(|d| !d.is_empty()).map(crate::dates::words);
        let detail = match kind {
            "memory" => {
                let files = plural(refs(n, "contents").len(), "file", "files");
                match words("dateEdtf").or_else(|| words("temporalCoverage")) {
                    Some(d) => format!("{d}, {files}"),
                    None => files,
                }
            }
            "story" => plural(refs(n, "memories").len(), "memory", "memories"),
            "family" => plural(refs(n, "member").len(), "member", "members"),
            "event" => words("startEdtf").unwrap_or_default(),
            "person" | "pet" => match (words("birthDateEdtf"), words("deathDateEdtf")) {
                (Some(b), Some(d)) => format!("{b} to {d}"),
                (Some(b), None) => format!("born {b}"),
                (None, Some(d)) => format!("died {d}"),
                _ => String::new(),
            },
            _ => String::new(),
        };
        let journey_id = match names.get(&(kind, norm(name))).map(|v| v.as_slice()) {
            Some([one]) => Some(one.clone()),
            _ => None,
        };
        if let Some(one) = &journey_id {
            out.matches.push(Match { export_id: id.clone(), kind: kind.into(), name: name.into(), journey_id: one.clone() });
        }
        out.items.push(Item { export_id: id, kind: kind.into(), name: name.into(), detail, journey_id });
    }
    Ok(out)
}

/// Part 1's result: the export's files copied into the holding folder.
pub struct ExportBatch {
    pub batch: Batch,
    pub catalog: Value,
    /// Content ID in the export to its prepared file.
    pub files: HashMap<String, Prepared>,
    /// Export IDs the person chose not to bring in.
    pub leave_out: HashSet<String>,
}

/// Part 1 (a background job, no catalog changes): copy the files of content
/// that is not in the Journey yet into the holding folder, checked against
/// the export's checksums. Damaged or missing files are reported as skipped.
pub fn prepare(journey_root: &Path, dir: &Path, leave_out: &HashSet<String>, ctx: &JobContext) -> Result<ExportBatch> {
    ctx.message("Reading the exported history…");
    let catalog = read_catalog(dir)?;
    let need = needed_content(&catalog, leave_out);
    let batch = Batch::new(journey_root)?;
    let conn = Connection::open_with_flags(journey_root.join(CATALOG_FILE), OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    let todo: Vec<(String, &Value, String)> = graph(&catalog)
        .iter()
        .filter(|n| content_kind(n).is_some())
        .filter_map(|n| Some((id_of(&n["@id"])?, n, file_of(n)?["contentUrl"].as_str()?.to_string())))
        .filter(|(id, _, _)| need.contains(id))
        .collect();
    let needed: u64 = todo.iter().filter(|(_, _, u)| crate::folder::is_inside(u)).filter_map(|(_, _, u)| std::fs::metadata(dir.join(u)).ok()).map(|m| m.len()).sum();
    crate::bulk::check_space(journey_root, needed)?;
    ctx.set_total(todo.len() as u64);
    let mut files = HashMap::new();
    for (n, (id, node, url)) in todo.iter().enumerate() {
        ctx.check()?;
        let label = format!("Copying {} ({} of {})", url.rsplit('/').next().unwrap_or(url), n + 1, todo.len());
        if present(&conn, id, "content")?.is_some() {
            ctx.advance(label);
            continue;
        }
        // The export's catalog could be changed by anyone it was shared
        // with, so a file is only taken from inside the export folder.
        if !crate::folder::is_inside(url) {
            ctx.skip(url.clone(), "The export lists this file outside its own folder, so it was not brought in.");
            ctx.advance(label);
            continue;
        }
        let src: PathBuf = url.split('/').fold(dir.to_path_buf(), |p, part| p.join(part));
        match crate::import::prepare_one(batch.dir(), &src, &conn) {
            Ok(mut p) => {
                let want = file_of(node).and_then(|f| f["sha256"].as_str()).unwrap_or_default();
                if !want.is_empty() && want != p.sha256 {
                    let _ = std::fs::remove_file(&p.holding);
                    ctx.skip(url.clone(), "The file has changed or is damaged since the export, so it was not brought in.");
                } else {
                    if let Some(name) = file_of(node).and_then(|f| f["originalName"].as_str()).filter(|s| !s.is_empty()) {
                        p.original_name = name.to_string();
                    }
                    p.source_folder = node["sourceNote"].as_str().unwrap_or_default().to_string();
                    files.insert(id.clone(), p);
                }
            }
            Err(Error::Canceled) => return Err(Error::Canceled),
            Err(e) => ctx.skip(url.clone(), crate::import::plain_reason(&e)),
        }
        ctx.advance(label);
    }
    Ok(ExportBatch { batch, catalog, files, leave_out: leave_out.clone() })
}

/// What a rebuild added and what it found already there.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RebuildReport {
    pub added: usize,
    pub files: usize,
    pub matched: usize,
    pub already: usize,
    /// Content IDs brought in, for making export copies afterwards.
    pub content: Vec<String>,
    pub notes: Vec<String>,
}

struct Ids<'a> {
    map: HashMap<String, String>,
    merge: &'a HashMap<String, String>,
}

impl Ids<'_> {
    fn get(&self, export_id: &str) -> Option<String> {
        self.map.get(export_id).cloned()
    }
}

fn exists(a: &Action, sql: &str, args: &[&str]) -> Result<bool> {
    Ok(a.conn().query_row(sql, rusqlite::params_from_iter(args.iter()), |r| r.get(0))?)
}

fn put(a: &mut Action, table: &str, key: &str, fields: &Map<String, Value>) -> Result<()> {
    for (k, v) in fields {
        if !v.is_null() {
            a.update(table, id_key(key), k, v.clone())?;
        }
    }
    Ok(())
}

/// Make the item under the export's ID, or under a new one when that ID
/// means something else here.
fn make(a: &mut Action, table: &str, id: &str, fields: Value) -> Result<String> {
    if taken(a.conn(), id)? {
        a.create(table, row(fields))
    } else {
        a.create_with_id(table, id, row(fields))?;
        Ok(id.to_string())
    }
}

impl Journey {
    /// Part 2: rebuild the export in this Journey as one action. `merge`
    /// maps export IDs to items already here (the ticked matches).
    pub fn finish_export(&mut self, eb: ExportBatch, merge: &HashMap<String, String>) -> Result<RebuildReport> {
        let ExportBatch { batch, catalog, files, leave_out } = eb;
        let root = self.root().to_path_buf();
        let author = self.author().to_string();
        let mut moved: Vec<PathBuf> = Vec::new();
        let mut report = RebuildReport::default();
        let nodes = graph(&catalog);
        let result = self.act(|a| {
            let mut ids = Ids { map: HashMap::new(), merge };
            let mut added: HashSet<String> = HashSet::new();
            // Which export IDs are here already, merged, or to be made.
            for n in nodes {
                let (Some(id), Some(table)) = (id_of(&n["@id"]), table_for(n)) else { continue };
                if let Some(to) = ids.merge.get(&id).filter(|to| present(a.conn(), to, table).ok().flatten().is_some()) {
                    ids.map.insert(id, to.clone());
                    report.matched += 1;
                } else if let Some(here) = present(a.conn(), &id, table)? {
                    ids.map.insert(id, here);
                    report.already += 1;
                }
            }
            // Not here yet, and not left out by the person.
            let new = |ids: &Ids, n: &Value| id_of(&n["@id"]).filter(|id| !ids.map.contains_key(id) && !leave_out.contains(id));
            // Places.
            for n in nodes.iter().filter(|n| ty(n) == "Place") {
                let Some(id) = new(&ids, n) else { continue };
                let (lat, lon) = (n["geo"]["latitude"].as_f64(), n["geo"]["longitude"].as_f64());
                let made = make(a, "place", &id, json!({"name": n["name"].as_str().unwrap_or("Place"), "description": text(n, "description"),
                    "address": text(n, "address"), "latitude": lat, "longitude": lon}))?;
                added.insert(made.clone());
                ids.map.insert(id, made);
            }
            // People and pets.
            for n in nodes.iter().filter(|n| matches!(ty(n), "Person" | "Pet")) {
                let Some(id) = new(&ids, n) else { continue };
                let person = ty(n) == "Person";
                let made = make(a, "individual", &id, json!({"kind": ty(n), "name": n["name"].as_str().unwrap_or("Unnamed"),
                    "description": text(n, "description"), "birth_date": text(n, "birthDate"), "birth_date_edtf": text(n, "birthDateEdtf"),
                    "death_date": text(n, "deathDate"), "death_date_edtf": text(n, "deathDateEdtf")}))?;
                if person {
                    let gender = text(n, "gender");
                    a.insert("person", row(json!({"id": made, "given_name": text(n, "givenName"), "family_name": text(n, "familyName"),
                        "other_names": text(n, "alternateName"), "gender": gender})))?;
                } else {
                    a.insert("pet", row(json!({"id": made, "species": text(n, "species"), "breed": text(n, "breed"), "joined_date_edtf": text(n, "joinedDate")})))?;
                }
                added.insert(made.clone());
                ids.map.insert(id, made);
            }
            // Families, their members and related families.
            for n in nodes.iter().filter(|n| ty(n) == "Family") {
                if let Some(id) = new(&ids, n) {
                    let made = make(a, "family", &id, json!({"name": n["name"].as_str().unwrap_or("Family"), "description": text(n, "description")}))?;
                    added.insert(made.clone());
                    ids.map.insert(id, made);
                }
            }
            for n in nodes.iter().filter(|n| ty(n) == "Family") {
                let Some(fam) = id_of(&n["@id"]).and_then(|i| ids.get(&i)) else { continue };
                for m in refs(n, "member").iter().filter_map(|m| ids.get(m)) {
                    if !exists(a, "SELECT EXISTS(SELECT 1 FROM family_member WHERE family_id = ?1 AND individual_id = ?2)", &[&fam, &m])? {
                        a.insert("family_member", row(json!({"family_id": fam, "individual_id": m})))?;
                    }
                }
                for other in refs(n, "relatedFamily").iter().filter_map(|m| ids.get(m)) {
                    if other != fam && !exists(a, "SELECT EXISTS(SELECT 1 FROM family_relationship WHERE (from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1))", &[&fam, &other])? {
                        a.insert("family_relationship", row(json!({"from_id": fam, "to_id": other})))?;
                    }
                }
            }
            // Relationships between people and pets.
            for n in nodes.iter().filter(|n| ty(n) == "Relationship") {
                let Some(id) = new(&ids, n) else { continue };
                let (Some(from), Some(to), Some(kind)) = (id_of(&n["from"]).and_then(|i| ids.get(&i)), id_of(&n["to"]).and_then(|i| ids.get(&i)), n["kind"].as_str()) else { continue };
                if exists(a, "SELECT EXISTS(SELECT 1 FROM individual_relationship WHERE from_id = ?1 AND to_id = ?2 AND kind = ?3)", &[&from, &to, kind])? {
                    continue;
                }
                let made = make(a, "individual_relationship", &id, json!({"from_id": from, "to_id": to, "kind": kind,
                    "qualifier": text(n, "qualifier"), "certainty": text(n, "certainty"), "status": "entered"}))?;
                added.insert(made.clone());
                ids.map.insert(id, made);
            }
            // Events and who took part.
            // A kind the family added comes back as it was typed (sprint 19).
            for n in nodes.iter().filter(|n| ty(n) == "Event") {
                let Some(id) = new(&ids, n) else { continue };
                if !n["kind"].as_str().is_some_and(|k| !k.trim().is_empty()) {
                    report.notes.push(format!("The event \u{201c}{}\u{201d} has no kind, so it was left out.", n["name"].as_str().unwrap_or_default()));
                    continue;
                }
                let made = make(a, "event", &id, json!({"kind": n["kind"].as_str().unwrap_or("trip"), "name": n["name"].as_str().unwrap_or("Event"),
                    "description": text(n, "description"), "start_date": text(n, "startDate"), "start_date_edtf": text(n, "startEdtf"),
                    "end_date": text(n, "endDate"), "end_date_edtf": text(n, "endEdtf"),
                    "place_id": id_of(&n["location"]).and_then(|p| ids.get(&p))}))?;
                for (pos, who) in refs(n, "attendee").iter().filter_map(|w| ids.get(w)).enumerate() {
                    if !exists(a, "SELECT EXISTS(SELECT 1 FROM event_individual WHERE event_id = ?1 AND individual_id = ?2)", &[&made, &who])? {
                        a.insert("event_individual", row(json!({"event_id": made, "individual_id": who, "position": pos as i64})))?;
                    }
                }
                added.insert(made.clone());
                ids.map.insert(id, made);
            }
            // Content and its files.
            for n in nodes.iter().filter(|n| content_kind(n).is_some()) {
                let Some(id) = new(&ids, n) else { continue };
                let Some(p) = files.get(&id) else { continue };
                if let Some(existing) = crate::import::existing_content(a.conn(), &p.sha256)? {
                    ids.map.insert(id, existing);
                    report.already += 1;
                    continue;
                }
                let want = if taken(a.conn(), &id)? { None } else { Some(id.as_str()) };
                let made = crate::import::record_prepared(a, &root, &author, p, want, &mut moved)?;
                let mut f = Map::new();
                f.insert("name".into(), text(n, "name"));
                f.insert("date".into(), text(n, "dateCreated"));
                f.insert("date_edtf".into(), text(n, "dateEdtf"));
                f.insert("description".into(), text(n, "description"));
                f.insert("author".into(), text(n, "creditText"));
                f.insert("copyright".into(), text(n, "copyrightNotice"));
                f.insert("date_added".into(), text(n, "dateAdded"));
                f.insert("added_by".into(), text(n, "addedBy"));
                put(a, "content", &made, &f)?;
                let mut x = Map::new();
                let table = match p.kind {
                    "picture" => {
                        x.insert("people_note".into(), text(n, "peopleNote"));
                        "picture"
                    }
                    "voice recording" => {
                        x.insert("language".into(), text(n, "inLanguage"));
                        x.insert("speaker_note".into(), text(n, "speakerNote"));
                        x.insert("quiet_start".into(), n["mdp:playFrom"].as_f64().map(Value::from).unwrap_or(Value::Null));
                        "voice_recording"
                    }
                    "video" => {
                        x.insert("language".into(), text(n, "inLanguage"));
                        x.insert("people_note".into(), text(n, "peopleNote"));
                        x.insert("speaker_note".into(), text(n, "speakerNote"));
                        "video"
                    }
                    _ => {
                        x.insert("language".into(), text(n, "inLanguage"));
                        x.insert("summary".into(), text(n, "abstract"));
                        x.insert("transcribed_by".into(), text(n, "transcribedBy"));
                        x.insert("review_status".into(), text(n, "reviewStatus"));
                        "text_content"
                    }
                };
                put(a, table, &made, &x)?;
                report.content.push(made.clone());
                report.files += 1;
                added.insert(made.clone());
                ids.map.insert(id, made);
            }
            // Transcripts, backs of pictures and other made-from links.
            for n in nodes.iter().filter(|n| content_kind(n).is_some()) {
                let Some(from) = id_of(&n["@id"]).and_then(|i| ids.get(&i)) else { continue };
                for (key, kind) in [("transcriptOf", "transcript of"), ("mdp:captionsOf", "captions of"), ("mdp:translationOf", "translation of"),
                    ("mdp:typedCopyOf", "typed copy of"), ("mdp:backOf", "back or page of"), ("stillFrameOf", "still frame of"),
                    ("mdp:clipOf", "clip of"), ("mdp:editedVersionOf", "edited version of")]
                {
                    let Some(to) = id_of(&n[key]).and_then(|i| ids.get(&i)) else { continue };
                    if to != from && !exists(a, "SELECT EXISTS(SELECT 1 FROM content_relationship WHERE from_id = ?1 AND to_id = ?2 AND kind = ?3)", &[&from, &to, kind])? {
                        a.insert("content_relationship", row(json!({"from_id": from, "to_id": to, "kind": kind})))?;
                    }
                }
            }
            // Memories with their content and people.
            for n in nodes.iter().filter(|n| ty(n) == "Memory") {
                let Some(id) = new(&ids, n) else { continue };
                let contents: Vec<String> = refs(n, "contents").iter().filter_map(|c| ids.get(c)).collect();
                if contents.is_empty() {
                    report.notes.push(format!("\u{201c}{}\u{201d} was left out because none of its files could be brought in.", n["name"].as_str().unwrap_or_default()));
                    continue;
                }
                let made = make(a, "memory", &id, json!({"name": n["name"].as_str().unwrap_or("Memory"), "status": "needs content",
                    "date": text(n, "temporalCoverage"), "date_edtf": text(n, "dateEdtf"), "caption": text(n, "description"),
                    "place_id": id_of(&n["contentLocation"]).and_then(|p| ids.get(&p)), "event_id": id_of(&n["event"]).and_then(|e| ids.get(&e))}))?;
                for c in &contents {
                    crate::memories::put_in(a, &made, c)?;
                }
                crate::memories::refresh_status(a, &made)?;
                if let Some(cover) = id_of(&n["cover"]).and_then(|c| ids.get(&c)).filter(|c| contents.contains(c)) {
                    a.update("memory", id_key(&made), "cover_id", json!(cover))?;
                }
                for link in n["people"].as_array().into_iter().flatten() {
                    let (Some(who), Some(role)) = (id_of(&link["individual"]).and_then(|w| ids.get(&w)), link["role"].as_str()) else { continue };
                    let note = text(link, "roleNote");
                    if role == "other" && note.is_null() {
                        continue;
                    }
                    if !exists(a, "SELECT EXISTS(SELECT 1 FROM memory_individual WHERE memory_id = ?1 AND individual_id = ?2 AND role = ?3)", &[&made, &who, role])? {
                        a.insert("memory_individual", row(json!({"memory_id": made, "individual_id": who, "role": role,
                            "role_note": note, "certainty": text(link, "certainty")})))?;
                    }
                }
                added.insert(made.clone());
                ids.map.insert(id, made);
            }
            // Stories, made new or added to.
            for n in nodes.iter().filter(|n| table_for(n) == Some("story")) {
                let story = match new(&ids, n) {
                    Some(id) => {
                        let made = make(a, "story", &id, json!({"name": n["name"].as_str().unwrap_or("Story"), "introduction": text(n, "description"),
                            "focus_id": id_of(&n["focus"]).and_then(|f| ids.get(&f)),
                            "genre": if n["genre"].as_str() == Some("tribute") { "tribute" } else { "story" }}))?;
                        added.insert(made.clone());
                        ids.map.insert(id, made.clone());
                        made
                    }
                    None => match id_of(&n["@id"]).and_then(|i| ids.get(&i)) {
                        Some(s) => s,
                        None => continue,
                    },
                };
                for m in refs(n, "memories").iter().filter_map(|m| ids.get(m)) {
                    crate::stories::append_memory(a, &story, &m)?;
                }
                // "Why it matters" notes on an "In memory of" page (format 1.5).
                for l in n["memoryNotes"].as_array().into_iter().flatten() {
                    let (Some(m), Some(note)) = (id_of(&l["memory"]).and_then(|m| ids.get(&m)), l["note"].as_str()) else { continue };
                    crate::stories::set_note(a, &story, &m, note)?;
                }
            }
            // Portraits of people added now, and the featured list.
            for n in nodes.iter().filter(|n| matches!(ty(n), "Person" | "Pet")) {
                let (Some(who), Some(img)) = (id_of(&n["@id"]).and_then(|i| ids.get(&i)), id_of(&n["image"]).and_then(|i| ids.get(&i))) else { continue };
                if added.contains(&who) {
                    a.update("individual", id_key(&who), "image_id", json!(img))?;
                }
            }
            for f in refs(&catalog, "featured").iter().filter_map(|f| ids.get(f)) {
                if !exists(a, "SELECT EXISTS(SELECT 1 FROM featured WHERE item_id = ?1)", &[&f])? {
                    let next: i64 = a.conn().query_row("SELECT coalesce(max(position) + 1, 0) FROM featured", [], |r| r.get(0))?;
                    a.insert("featured", row(json!({"position": next, "item_id": f})))?;
                }
            }
            report.added = added.len();
            Ok(())
        });
        drop(batch);
        if let Err(e) = result {
            for m in moved {
                let _ = crate::import::set_readonly(&m, false);
                let _ = std::fs::remove_file(&m);
            }
            return Err(e);
        }
        Ok(report)
    }
}
