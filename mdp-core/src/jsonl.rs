//! The plain-text copy of the catalog, `catalog.jsonl`.
//!
//! JSON-LD, one item per line, in a fixed order: the context, the Journey,
//! then every other item by ID. Names are the JSON-LD names;
//! fields kept in the app only are written as `mdp:` names, so
//! the copy holds everything the SQLite catalog holds (it is not the export,
//! which leaves those out). Links sit on the item that owns them.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use rusqlite::Connection;
use serde_json::{json, Map, Value};

use crate::history::from_sql;
use crate::journey::Journey;
use crate::Result;

pub const JSONL_FILE: &str = "catalog.jsonl";
pub const CONTEXT_FILE: &str = "context.jsonld";
pub const CONTEXT: &str = include_str!("context.jsonld");

/// (column, JSON-LD name) per table. `id`, `kind` and `updated` are handled separately.
fn names(table: &str) -> &'static [(&'static str, &'static str)] {
    match table {
        "journey" => &[("name", "name"), ("introduction", "description"), ("language", "inLanguage"),
            ("format_version", "formatVersion"), ("date_modified", "dateModified")],
        "individual" => &[("name", "name"), ("birth_date", "birthDate"), ("birth_date_edtf", "birthDateEdtf"),
            ("death_date", "deathDate"), ("death_date_edtf", "deathDateEdtf"), ("description", "description"), ("image_id", "image")],
        "person" => &[("given_name", "givenName"), ("family_name", "familyName"), ("other_names", "alternateName"),
            ("gender", "gender"), ("living_override", "mdp:livingOverride")],
        "pet" => &[("species", "species"), ("breed", "breed"), ("joined_date_edtf", "joinedDate")],
        "individual_relationship" => &[("from_id", "from"), ("to_id", "to"), ("kind", "kind"),
            ("qualifier", "qualifier"), ("certainty", "certainty"), ("source_note", "sourceNote"),
            ("status", "mdp:relationshipStatus")],
        "family" => &[("name", "name"), ("description", "description")],
        "place" => &[("name", "name"), ("address", "address"), ("exact", "mdp:exact"), ("description", "description")],
        "event" => &[("kind", "kind"), ("name", "name"), ("start_date", "startDate"), ("start_date_edtf", "startEdtf"),
            ("end_date", "endDate"), ("end_date_edtf", "endEdtf"), ("place_id", "location"), ("description", "description")],
        "story" => &[("name", "name"), ("introduction", "description"), ("focus_id", "focus"), ("genre", "genre")],
        "memory" => &[("name", "name"), ("date", "temporalCoverage"), ("date_edtf", "dateEdtf"), ("caption", "description"),
            ("status", "status"), ("place_id", "contentLocation"), ("event_id", "event"), ("cover_id", "cover"), ("finished", "mdp:finished")],
        "content" => &[("state", "mdp:state"), ("name", "name"), ("date", "dateCreated"), ("date_edtf", "dateEdtf"),
            ("description", "description"), ("author", "creditText"), ("copyright", "copyrightNotice"),
            ("source_note", "sourceNote"), ("date_added", "dateAdded"), ("added_by", "addedBy")],
        "picture" => &[("width", "width"), ("height", "height"), ("people_note", "peopleNote"),
            ("latitude", "gpsLatitude"), ("longitude", "gpsLongitude"), ("camera", "camera"), ("turn", "mdp:turn")],
        "voice_recording" => &[("duration", "duration"), ("language", "inLanguage"), ("speaker_note", "speakerNote"),
            ("quiet_start", "mdp:quietStart"), ("play_full", "mdp:playFull")],
        "video" => &[("duration", "duration"), ("width", "width"), ("height", "height"), ("language", "inLanguage"),
            ("people_note", "peopleNote"), ("speaker_note", "speakerNote"),
            ("latitude", "gpsLatitude"), ("longitude", "gpsLongitude"), ("camera", "camera")],
        "text_content" => &[("language", "inLanguage"), ("summary", "abstract"), ("transcribed_by", "transcribedBy"),
            ("review_status", "reviewStatus")],
        "content_file" => &[("is_copy", "copy"), ("path", "contentUrl"), ("format", "encodingFormat"),
            ("size", "contentSize"), ("sha256", "sha256"), ("original_name", "originalName")],
        "memory_individual" => &[("individual_id", "individual"), ("role", "role"), ("role_note", "roleNote"),
            ("certainty", "certainty")],
        _ => &[],
    }
}

/// Columns holding another item's ID, written as `mdpid:` references.
const ID_COLUMNS: &[&str] = &["from_id", "to_id", "place_id", "event_id", "cover_id", "focus_id", "individual_id", "image_id"];

/// The "why it matters" notes on an "In memory of" page, as
/// `[{"memory": "mdpid:…", "note": "…"}]`, in the page's order (format 1.5).
pub(crate) fn memory_notes(links: &[Map<String, Value>]) -> Vec<Value> {
    links
        .iter()
        .filter_map(|r| {
            let note = r.get("note").and_then(|v| v.as_str()).filter(|n| !n.trim().is_empty())?;
            Some(json!({"memory": mdpid(r.get("memory_id")?.as_str()?), "note": note}))
        })
        .collect()
}

fn mdpid(id: &str) -> Value {
    Value::from(format!("mdpid:{id}"))
}

fn select(conn: &Connection, sql: &str, args: &[&str]) -> Result<Vec<Map<String, Value>>> {
    let mut stmt = conn.prepare(sql)?;
    let cols: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |r| {
        let mut m = Map::new();
        for (i, c) in cols.iter().enumerate() {
            m.insert(c.clone(), from_sql(r.get_ref(i)?));
        }
        Ok(m)
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn put_fields(obj: &mut Map<String, Value>, table: &str, row: &Map<String, Value>) {
    for (col, name) in names(table) {
        match row.get(*col) {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if ID_COLUMNS.contains(col) => {
                obj.insert(name.to_string(), mdpid(s));
            }
            Some(v) if *col == "exact" || *col == "is_copy" || *col == "play_full" => {
                obj.insert(name.to_string(), Value::Bool(v.as_i64() == Some(1)));
            }
            Some(v) => {
                obj.insert(name.to_string(), v.clone());
            }
        }
    }
}

/// Rows read a whole table at a time: each table
/// is read once, in order, instead of a few small queries per item. With
/// `only`, just that item's rows are read (the item details panel).
struct Rows<'a> {
    conn: &'a Connection,
    only: Option<&'a str>,
    loaded: HashMap<String, HashMap<String, Vec<Map<String, Value>>>>,
}

impl<'a> Rows<'a> {
    /// The rows of `table` whose `key` is `id`, in `order`. Each item's rows
    /// are asked for once, so they are handed over rather than copied.
    fn of(&mut self, table: &str, key: &str, order: &str, id: &str) -> Result<Vec<Map<String, Value>>> {
        let name = format!("{table}.{key}");
        if !self.loaded.contains_key(&name) {
            let filter = if self.only.is_some() { format!("WHERE {key} = ?1") } else { String::new() };
            let sql = format!("SELECT * FROM {table} {filter} ORDER BY {key}, {order}");
            let args: Vec<&str> = self.only.into_iter().collect();
            let mut by: HashMap<String, Vec<Map<String, Value>>> = HashMap::new();
            for r in select(self.conn, &sql, &args)? {
                let k = r.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                by.entry(k).or_default().push(r);
            }
            self.loaded.insert(name.clone(), by);
        }
        Ok(self.loaded.get_mut(&name).and_then(|m| m.remove(id)).unwrap_or_default())
    }

    fn one(&mut self, table: &str, id: &str) -> Result<Option<Map<String, Value>>> {
        Ok(self.of(table, "id", "id", id)?.into_iter().next())
    }

    fn ids(&mut self, table: &str, key: &str, col: &str, order: &str, id: &str) -> Result<Vec<Value>> {
        Ok(self.of(table, key, order, id)?.iter().filter_map(|r| r.get(col).and_then(|v| v.as_str()).map(mdpid)).collect())
    }
}

/// Build every line of the plain-text copy.
pub fn lines(conn: &Connection) -> Result<Vec<Value>> {
    build(conn, None)
}

/// One item's line, as it appears in the plain-text copy.
pub fn line(conn: &Connection, id: &str) -> Result<Option<Value>> {
    let want = mdpid(id);
    Ok(build(conn, Some(id))?.into_iter().find(|l| l["@id"] == want))
}

fn build(conn: &Connection, only: Option<&str>) -> Result<Vec<Value>> {
    let mut items: BTreeMap<String, Value> = BTreeMap::new();
    let mut journey = Value::Null;
    let mut rows = Rows { conn, only, loaded: HashMap::new() };
    let reg = match only {
        Some(id) => select(conn, "SELECT id, table_name FROM item WHERE id = ?1", &[id])?,
        None => select(conn, "SELECT id, table_name FROM item ORDER BY id", &[])?,
    };
    for reg in reg {
        let id = reg["id"].as_str().unwrap_or_default().to_string();
        let table = reg["table_name"].as_str().unwrap_or_default().to_string();
        let Some(row) = rows.one(&table, &id)? else {
            continue;
        };
        let mut o = Map::new();
        o.insert("@id".into(), mdpid(&id));
        let kind = row.get("kind").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let ty = match table.as_str() {
            "journey" => "Journey",
            "individual" => if kind == "Pet" { "Pet" } else { "Person" },
            "individual_relationship" => "Relationship",
            "family" => "Family",
            "place" => "Place",
            "event" => "Event",
            "story" => "CreativeWork",
            "memory" => "Memory",
            "content" => match kind.as_str() {
                "picture" => "ImageObject",
                "voice recording" => "AudioObject",
                "video" => "VideoObject",
                _ => "CreativeWork",
            },
            other => other,
        };
        o.insert("@type".into(), ty.into());
        put_fields(&mut o, &table, &row);
        let former = rows.ids("alias", "item_id", "old_id", "old_id", &id)?;
        if !former.is_empty() {
            o.insert("mdp:formerId".into(), Value::Array(former));
        }
        match table.as_str() {
            "journey" => {
                let f: Vec<Value> = select(conn, "SELECT item_id AS x FROM featured ORDER BY position", &[])?
                    .into_iter()
                    .filter_map(|r| r.get("x").and_then(|v| v.as_str()).map(mdpid))
                    .collect();
                if !f.is_empty() {
                    o.insert("featured".into(), Value::Array(f));
                }
            }
            "individual" => {
                let ext = if kind == "Pet" { "pet" } else { "person" };
                if let Some(e) = rows.one(ext, &id)? {
                    put_fields(&mut o, ext, &e);
                }
            }
            "family" => {
                let m = rows.ids("family_member", "family_id", "individual_id", "individual_id", &id)?;
                if !m.is_empty() {
                    o.insert("member".into(), Value::Array(m));
                }
                let r = rows.ids("family_relationship", "from_id", "to_id", "to_id", &id)?;
                if !r.is_empty() {
                    o.insert("relatedFamily".into(), Value::Array(r));
                }
            }
            "place" => {
                if let (Some(lat), Some(lon)) = (row.get("latitude").filter(|v| !v.is_null()), row.get("longitude").filter(|v| !v.is_null())) {
                    o.insert("geo".into(), json!({"@type": "GeoCoordinates", "latitude": lat, "longitude": lon}));
                }
            }
            "event" => {
                let a = rows.ids("event_individual", "event_id", "individual_id", "position, individual_id", &id)?;
                if !a.is_empty() {
                    o.insert("attendee".into(), Value::Array(a));
                }
            }
            "story" => {
                let links = rows.of("story_memory", "story_id", "position", &id)?;
                let m: Vec<Value> = links.iter().filter_map(|r| r.get("memory_id").and_then(|v| v.as_str()).map(mdpid)).collect();
                if !m.is_empty() {
                    o.insert("memories".into(), Value::Array(m));
                }
                let notes = memory_notes(&links);
                if !notes.is_empty() {
                    o.insert("memoryNotes".into(), Value::Array(notes));
                }
            }
            "memory" => {
                let c = rows.ids("memory_content", "memory_id", "content_id", "position", &id)?;
                if !c.is_empty() {
                    o.insert("contents".into(), Value::Array(c));
                }
                let people: Vec<Value> = rows
                    .of("memory_individual", "memory_id", "individual_id, role", &id)?
                    .iter()
                    .map(|r| {
                        let mut p = Map::new();
                        put_fields(&mut p, "memory_individual", r);
                        Value::Object(p)
                    })
                    .collect();
                if !people.is_empty() {
                    o.insert("people".into(), Value::Array(people));
                }
            }
            "content" => {
                let ext = match kind.as_str() {
                    "picture" => "picture",
                    "voice recording" => "voice_recording",
                    "video" => "video",
                    _ => "text_content",
                };
                if let Some(e) = rows.one(ext, &id)? {
                    put_fields(&mut o, ext, &e);
                }
                let files: Vec<Value> = rows
                    .of("content_file", "content_id", "is_copy, path", &id)?
                    .iter()
                    .map(|r| {
                        let mut f = Map::new();
                        f.insert("@type".into(), "MediaObject".into());
                        put_fields(&mut f, "content_file", r);
                        Value::Object(f)
                    })
                    .collect();
                if !files.is_empty() {
                    o.insert("encoding".into(), Value::Array(files));
                }
                for r in rows.of("content_relationship", "from_id", "kind, to_id", &id)? {
                    let name = relationship_name(r["kind"].as_str().unwrap_or_default());
                    if let Some(to) = r["to_id"].as_str() {
                        o.insert(name.into(), mdpid(to));
                    }
                }
            }
            _ => {}
        }
        if table == "journey" {
            journey = Value::Object(o);
        } else {
            items.insert(id, Value::Object(o));
        }
    }
    let mut out = vec![json!({"@context": CONTEXT_FILE})];
    if !journey.is_null() {
        out.push(journey);
    }
    out.extend(items.into_values());
    Ok(out)
}

fn relationship_name(kind: &str) -> &'static str {
    match kind {
        "transcript of" => "transcriptOf",
        "captions of" => "mdp:captionsOf",
        "translation of" => "mdp:translationOf",
        "typed copy of" => "mdp:typedCopyOf",
        "back or page of" => "mdp:backOf",
        "still frame of" => "stillFrameOf",
        "clip of" => "mdp:clipOf",
        _ => "mdp:editedVersionOf",
    }
}

/// Rewrite `catalog.jsonl` (and the context beside it) safely: write a
/// temporary file, then replace the old one, so a crash never leaves half a copy.
pub fn write(j: &Journey) -> Result<()> {
    let root = j.root();
    let ctx = root.join(CONTEXT_FILE);
    if !ctx.exists() {
        std::fs::write(&ctx, CONTEXT)?;
    }
    let tmp = root.join(format!("{JSONL_FILE}.tmp"));
    {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        for line in lines(j.conn())? {
            serde_json::to_writer(&mut f, &line).map_err(|e| crate::Error::Invalid(e.to_string()))?;
            f.write_all(b"\n")?;
        }
        f.flush()?;
        f.get_ref().sync_all()?;
    }
    std::fs::rename(&tmp, root.join(JSONL_FILE))?;
    Ok(())
}
