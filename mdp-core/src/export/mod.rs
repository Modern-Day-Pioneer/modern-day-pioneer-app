//! The export and its preview.
//!
//! Three stages:
//!   1. [`plan`]: the catalog is read out of SQLite and written as JSON-LD,
//!      applying the export rules: placeholders and empty stories left out,
//!      only content held by a memory, privacy. The plan
//!      is plain owned data, so the slow part can run off the Journey's lock.
//!   2. [`write`]: each file is copied under its export name with its facts
//!      written inside it ([`facts`]), then the catalog gets the exported
//!      files' sizes and checksums.
//!   3. [`pages`]: every page is made from the catalog by fixed templates.
//!
//! An export is written to a new folder beside the chosen one and replaces
//! the previous export only when it is complete. The preview runs the same
//! stages into the Journey's cache, pointing at the working files instead of
//! copying them, so it is quick and needs no space.

pub mod facts;
pub mod pages;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::history::from_sql;
use crate::jobs::JobContext;
use crate::{dates, folder, Error, Journey, Result};

/// The front door of an export.
pub const FRONT: &str = "Open me - Family History.html";
/// The plain-language guide at the top of an export.
pub const ABOUT_FILE: &str = "ABOUT-THIS-FOLDER.txt";
/// Where the preview is made, inside the Journey's cache folder.
pub const PREVIEW_DIR: &str = "preview";

/// The export's settings, from the Journey's settings: privacy, and the
/// front door (sprint 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Privacy {
    /// Include the dates of living people (the setting "include").
    pub living_dates: bool,
    /// Include the address and coordinates of places marked exact.
    pub exact_places: bool,
    /// Open on the coffee table instead of the bookshelf (the setting
    /// "coffee table"; the bookshelf stays the default).
    pub coffee_table: bool,
    /// Add the Scrapbook stylesheet over the plain one (the setting
    /// "scrapbook", sprint 17). The pages are the same either way.
    pub scrapbook: bool,
    /// Add `manifest-sha256.txt`, a checksum for every file in BagIt's
    /// manifest format, so a damaged copy can be found later (sprint 19).
    pub checksums: bool,
}

impl Default for Privacy {
    fn default() -> Self {
        Privacy { living_dates: false, exact_places: false, coffee_table: false, scrapbook: false, checksums: false }
    }
}

impl Privacy {
    /// From the settings' words: "leave out" or "include", "town only" or "exact".
    pub fn from_settings(living_people: &str, exact_places: &str) -> Privacy {
        Privacy { living_dates: living_people == "include", exact_places: exact_places == "exact", coffee_table: false, scrapbook: false, checksums: false }
    }
    /// The same with the front door: "bookshelf" or "coffee table".
    pub fn with_front_door(self, front_door: &str) -> Privacy {
        Privacy { coffee_table: front_door == "coffee table", ..self }
    }
    /// The same with the export's look: "plain" or "scrapbook" (sprint 17).
    pub fn with_export_style(self, style: &str) -> Privacy {
        Privacy { scrapbook: style == "scrapbook", ..self }
    }
    /// The same with or without the checksum list (sprint 19).
    pub fn with_checksums(self, checksums: bool) -> Privacy {
        Privacy { checksums, ..self }
    }
}

/// What the export leaves out, for the one-line note.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct LeftOut {
    /// Memories with nothing in them yet.
    pub placeholders: usize,
    /// Stories with no memory that is exported.
    pub empty_stories: usize,
    /// Pictures, recordings, videos and texts in no memory: waiting or set aside.
    pub not_in_memory: usize,
    /// Relationships the app suggested that nobody accepted.
    pub suggested: usize,
    /// Living people whose dates are left out (privacy setting).
    pub living_hidden: usize,
    /// Places marked exact whose address and coordinates are left out.
    pub exact_hidden: usize,
}

impl LeftOut {
    /// The note under the preview, in plain words.
    pub fn sentence(&self) -> String {
        let mut parts = Vec::new();
        let n = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        if self.placeholders > 0 {
            parts.push(format!("{} with nothing in them yet", n(self.placeholders, "memory", "memories")));
        }
        if self.empty_stories > 0 {
            parts.push(format!("{} with no finished memory", n(self.empty_stories, "story", "stories")));
        }
        if self.not_in_memory > 0 {
            parts.push(format!("{} not in a memory", n(self.not_in_memory, "item", "items")));
        }
        if self.suggested > 0 {
            parts.push(format!("{} nobody accepted", n(self.suggested, "suggested relationship", "suggested relationships")));
        }
        let mut s = if parts.is_empty() { "Nothing is left out".to_string() } else { format!("Left out: {}", join_and(&parts)) };
        let mut privacy = Vec::new();
        if self.living_hidden > 0 {
            privacy.push(format!("the dates of {}", n(self.living_hidden, "living person", "living people")));
        }
        if self.exact_hidden > 0 {
            privacy.push(format!("the address of {}", n(self.exact_hidden, "exact place", "exact places")));
        }
        if !privacy.is_empty() {
            s.push_str(&format!(". Hidden for privacy: {} (Settings)", join_and(&privacy)));
        }
        s.push('.');
        s
    }
}

fn join_and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        _ => format!("{} and {}", parts[..parts.len() - 1].join(", "), parts[parts.len() - 1]),
    }
}

/// One file to put in the export.
#[derive(Debug, Clone)]
pub struct FilePlan {
    pub content_id: String,
    /// The working file, absolute.
    pub source: PathBuf,
    /// The working file, relative to the Journey folder.
    pub working: String,
    /// The export name inside the export: `photos/1962/1962-06-02_short-name_ID.jpg`.
    pub dest: String,
    pub format: String,
    pub is_copy: bool,
}

/// Everything an export needs, read from the catalog in one go.
#[derive(Debug, Clone)]
pub struct Plan {
    pub catalog: Value,
    pub files: Vec<FilePlan>,
    pub facts: HashMap<String, facts::Facts>,
    pub left_out: LeftOut,
    pub privacy: Privacy,
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

fn s<'a>(r: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    r.get(k).and_then(|v| v.as_str()).filter(|v| !v.is_empty())
}

fn mdpid(id: &str) -> Value {
    Value::from(format!("mdpid:{id}"))
}

/// Drop empty fields, as the catalog leaves them out.
fn clean(o: Value) -> Value {
    match o {
        Value::Object(m) => Value::Object(
            m.into_iter().filter(|(_, v)| !(v.is_null() || v == &json!("") || v.as_array().is_some_and(|a| a.is_empty()))).collect(),
        ),
        v => v,
    }
}

/// A short, safe piece of a file or page name: lowercase letters, digits
/// and dashes only, so names survive every file system and cloud drive.
pub fn slug(text: &str, max: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let folded = fold(c);
        for c in folded.chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c.to_ascii_lowercase());
            } else if !out.ends_with('-') {
                out.push('-');
            }
        }
    }
    let t: String = out.trim_matches('-').chars().take(max).collect();
    let t = t.trim_end_matches('-').to_string();
    if t.is_empty() { "item".into() } else { t }
}

/// Letters with accents as their plain Latin letter (enough for names).
fn fold(c: char) -> String {
    let plain = match c {
        'à'..='å' | 'À'..='Å' | 'ā' | 'Ā' | 'ą' | 'Ą' => "a",
        'æ' | 'Æ' => "ae",
        'ç' | 'Ç' | 'č' | 'Č' | 'ć' | 'Ć' => "c",
        'è'..='ë' | 'È'..='Ë' | 'ē' | 'Ē' | 'ę' | 'Ę' | 'ě' | 'Ě' => "e",
        'ì'..='ï' | 'Ì'..='Ï' | 'ī' | 'Ī' => "i",
        'ñ' | 'Ñ' | 'ń' | 'Ń' | 'ň' | 'Ň' => "n",
        'ò'..='ö' | 'Ò'..='Ö' | 'ø' | 'Ø' | 'ō' | 'Ō' | 'ő' | 'Ő' => "o",
        'œ' | 'Œ' => "oe",
        'ß' => "ss",
        'ś' | 'Ś' | 'š' | 'Š' => "s",
        'ù'..='ü' | 'Ù'..='Ü' | 'ū' | 'Ū' | 'ů' | 'Ů' | 'ű' | 'Ű' => "u",
        'ý' | 'ÿ' | 'Ý' => "y",
        'ž' | 'Ž' | 'ź' | 'Ź' | 'ż' | 'Ż' => "z",
        'ł' | 'Ł' => "l",
        'ř' | 'Ř' => "r",
        'ğ' | 'Ğ' => "g",
        'đ' | 'Đ' | 'ð' | 'Ð' => "d",
        'þ' | 'Þ' => "th",
        '\'' | '’' => "",
        _ => return c.to_string(),
    };
    plain.to_string()
}

/// The date part of an export name: as much of the date as is known
/// (`1962-06-02`, `1913-06`, `1913`), a decade (`1970s`), or `undated`.
pub fn date_part(edtf: Option<&str>, nearest: Option<&str>) -> String {
    let e = edtf.unwrap_or("").trim();
    let first = e.split('/').find(|p| *p != ".." && !p.is_empty()).unwrap_or("");
    let core = first.trim_end_matches(['~', '?', '%']);
    if core.len() >= 4 && core[..4].contains('X') && core[..3].chars().all(|c| c.is_ascii_digit()) {
        return format!("{}0s", &core[..3]);
    }
    let known: String = core.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    let known = known.trim_end_matches('-');
    if matches!(known.len(), 4 | 7 | 10) && known[..4].chars().all(|c| c.is_ascii_digit()) {
        return known.to_string();
    }
    match nearest.map(str::trim) {
        Some(n) if n.len() >= 4 && n[..4].chars().all(|c| c.is_ascii_digit()) => n.chars().take(10).collect(),
        _ => "undated".into(),
    }
}

/// The export name: `photos/1962/1962-06-02_walters-camera_k7f3m2qa.jpg`.
pub fn export_name(kind: &str, name: &str, id: &str, edtf: Option<&str>, nearest: Option<&str>, ext: &str) -> String {
    let d = date_part(edtf, nearest);
    let year = if d[..4.min(d.len())].chars().all(|c| c.is_ascii_digit()) && d.len() >= 4 { d[..4].to_string() } else { "undated".into() };
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let dot = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    // "walter-camera.jpg" as a title gives "walter-camera", not "walter-camera-jpg".
    let lower = name.to_ascii_lowercase();
    let stem = Path::new(name).extension().filter(|e| crate::import::classify(&e.to_string_lossy()).is_some() || lower.ends_with(&dot)).map(|e| &name[..name.len() - e.len() - 1]).unwrap_or(name);
    format!("{}/{year}/{d}_{}_{id}{dot}", export_folder(kind), slug(stem, 28))
}

/// Folders by type in the export.
pub fn export_folder(kind: &str) -> &'static str {
    match kind {
        "picture" => "photos",
        "voice recording" => "audio",
        "video" => "video",
        _ => "stories",
    }
}

fn schema_type(kind: &str) -> &'static str {
    match kind {
        "picture" => "ImageObject",
        "voice recording" => "AudioObject",
        "video" => "VideoObject",
        _ => "CreativeWork",
    }
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

/// The day of the Journey's last change, so the same project exported twice
/// gives the same pages.
fn last_changed(conn: &Connection) -> Result<String> {
    let at: Option<String> = conn.query_row("SELECT max(at) FROM history", [], |r| r.get(0))?;
    Ok(at.map(|a| a.chars().take(10).collect()).unwrap_or_default())
}

/// Read the catalog and apply the export rules.
pub fn plan(j: &Journey, privacy: Privacy) -> Result<Plan> {
    let this_year: i32 = chrono::Local::now().format("%Y").to_string().parse().unwrap_or(2026);
    plan_at(j, privacy, this_year)
}

/// [`plan`] with a fixed year for the living rule (for checks).
pub fn plan_at(j: &Journey, privacy: Privacy, this_year: i32) -> Result<Plan> {
    let conn = j.conn();
    let root = j.root();
    let q = |sql: &str| select(conn, sql, &[]);
    let qa = |sql: &str, a: &str| select(conn, sql, &[a]);
    let mut left = LeftOut::default();
    let mut graph: Vec<Value> = Vec::new();

    // Memories: only those holding something (placeholders stay out).
    let mems = q("SELECT * FROM memory ORDER BY coalesce(date, ''), name, id")?;
    let mut mem_ids: HashSet<String> = HashSet::new();
    let mut mem_content: HashMap<String, Vec<String>> = HashMap::new();
    for m in &mems {
        let id = s(m, "id").unwrap_or_default().to_string();
        let c: Vec<String> = qa("SELECT content_id FROM memory_content WHERE memory_id = ?1 ORDER BY position, content_id", &id)?
            .iter()
            .filter_map(|r| s(r, "content_id").map(str::to_string))
            .collect();
        if c.is_empty() || s(m, "status") == Some("needs content") {
            left.placeholders += 1;
            continue;
        }
        mem_ids.insert(id.clone());
        mem_content.insert(id, c);
    }
    // Content: what an exported memory holds, plus what is made from it.
    let mut held: HashSet<String> = mem_content.values().flatten().cloned().collect();
    let rels = q("SELECT * FROM content_relationship ORDER BY from_id, kind, to_id")?;
    loop {
        let before = held.len();
        for r in &rels {
            if let (Some(f), Some(t)) = (s(r, "from_id"), s(r, "to_id")) {
                if held.contains(t) {
                    held.insert(f.to_string());
                }
            }
        }
        if held.len() == before {
            break;
        }
    }
    let all_content: i64 = conn.query_row("SELECT count(*) FROM content", [], |r| r.get(0))?;
    left.not_in_memory = (all_content as usize).saturating_sub(held.len());

    // Places: exact places keep only their name unless the setting says otherwise.
    let mut place_names: HashMap<String, (String, Option<(f64, f64)>)> = HashMap::new();
    for p in q("SELECT * FROM place ORDER BY name, id")? {
        let id = s(&p, "id").unwrap_or_default().to_string();
        let exact = p.get("exact").and_then(|v| v.as_i64()) == Some(1);
        let hide = exact && !privacy.exact_places;
        let mut o = json!({"@id": mdpid(&id), "@type": "Place", "name": p["name"], "description": p["description"]});
        let coords = match (p.get("latitude").and_then(|v| v.as_f64()), p.get("longitude").and_then(|v| v.as_f64())) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        };
        if hide {
            if s(&p, "address").is_some() || coords.is_some() {
                left.exact_hidden += 1;
            }
        } else {
            o["address"] = p["address"].clone();
            if let Some((a, b)) = coords {
                o["geo"] = json!({"@type": "GeoCoordinates", "latitude": a, "longitude": b});
            }
        }
        place_names.insert(id, (s(&p, "name").unwrap_or_default().to_string(), if hide { None } else { coords }));
        graph.push(clean(o));
    }

    // People and pets: living people get no dates unless the setting says so.
    let mut names: HashMap<String, String> = HashMap::new();
    let mut living_ids: HashSet<String> = HashSet::new();
    for i in q("SELECT i.*, p.given_name, p.family_name, p.other_names, p.gender, p.living_override, \
                t.species, t.breed, t.joined_date_edtf FROM individual i LEFT JOIN person p USING (id) \
                LEFT JOIN pet t USING (id) ORDER BY i.name, i.id")?
    {
        let id = s(&i, "id").unwrap_or_default().to_string();
        let person = s(&i, "kind") == Some("Person");
        names.insert(id.clone(), s(&i, "name").unwrap_or_default().to_string());
        let mut o = json!({"@id": mdpid(&id), "@type": if person { "Person" } else { "Pet" }, "name": i["name"], "description": i["description"]});
        // Their portrait, when that picture is exported too (sprint 8; schema.org image).
        if let Some(img) = s(&i, "image_id").filter(|p| held.contains(*p)) {
            o["image"] = mdpid(img);
        }
        let living = person
            && crate::people::is_living(s(&i, "death_date_edtf"), s(&i, "birth_date_edtf").or(s(&i, "birth_date")), s(&i, "living_override"), this_year);
        if living && !privacy.living_dates {
            o["living"] = json!(true);
            living_ids.insert(id.clone());
            if s(&i, "birth_date_edtf").is_some() || s(&i, "birth_date").is_some() {
                left.living_hidden += 1;
            }
        } else {
            for (col, name) in [("birth_date", "birthDate"), ("birth_date_edtf", "birthDateEdtf"), ("death_date", "deathDate"), ("death_date_edtf", "deathDateEdtf")] {
                o[name] = i[col].clone();
            }
        }
        if person {
            for (col, name) in [("given_name", "givenName"), ("family_name", "familyName"), ("other_names", "alternateName"), ("gender", "gender")] {
                o[name] = i[col].clone();
            }
        } else {
            for (col, name) in [("species", "species"), ("breed", "breed"), ("joined_date_edtf", "joinedDate")] {
                o[name] = i[col].clone();
            }
        }
        graph.push(clean(o));
    }
    for r in q("SELECT * FROM individual_relationship ORDER BY from_id, to_id, kind, id")? {
        let st = s(&r, "status").unwrap_or_default();
        if st != "entered" && st != "accepted" {
            if st == "suggested" {
                left.suggested += 1;
            }
            continue;
        }
        graph.push(clean(json!({"@id": mdpid(s(&r, "id").unwrap_or_default()), "@type": "Relationship",
            "from": mdpid(s(&r, "from_id").unwrap_or_default()), "to": mdpid(s(&r, "to_id").unwrap_or_default()),
            "kind": r["kind"], "qualifier": r["qualifier"], "certainty": r["certainty"]})));
    }
    for f in q("SELECT * FROM family ORDER BY name, id")? {
        let id = s(&f, "id").unwrap_or_default().to_string();
        names.insert(id.clone(), s(&f, "name").unwrap_or_default().to_string());
        let member: Vec<Value> = qa("SELECT m.individual_id FROM family_member m JOIN individual i ON i.id = m.individual_id \
                                     WHERE m.family_id = ?1 ORDER BY i.kind, i.name, i.id", &id)?
            .iter()
            .filter_map(|r| s(r, "individual_id").map(mdpid))
            .collect();
        let related: Vec<Value> = qa("SELECT to_id AS x FROM family_relationship WHERE from_id = ?1 \
                                      UNION SELECT from_id FROM family_relationship WHERE to_id = ?1 ORDER BY 1", &id)?
            .iter()
            .filter_map(|r| s(r, "x").map(mdpid))
            .collect();
        graph.push(clean(json!({"@id": mdpid(&id), "@type": "Family", "name": f["name"], "description": f["description"],
            "member": member, "relatedFamily": related})));
    }
    for e in q("SELECT * FROM event ORDER BY coalesce(start_date, ''), name, id")? {
        let id = s(&e, "id").unwrap_or_default().to_string();
        let who: Vec<String> = qa("SELECT individual_id FROM event_individual WHERE event_id = ?1 ORDER BY position, individual_id", &id)?
            .iter()
            .filter_map(|r| s(r, "individual_id").map(str::to_string))
            .collect();
        // A living person's birth date stays out with their other dates.
        let hide = s(&e, "kind") == Some("birth") && who.first().is_some_and(|w| living_ids.contains(w));
        let mut o = json!({"@id": mdpid(&id), "@type": "Event", "kind": e["kind"], "name": e["name"], "description": e["description"],
            "location": s(&e, "place_id").map(mdpid), "attendee": who.iter().map(|w| mdpid(w)).collect::<Vec<_>>()});
        if !hide {
            for (col, name) in [("start_date", "startDate"), ("start_date_edtf", "startEdtf"), ("end_date", "endDate"), ("end_date_edtf", "endEdtf")] {
                o[name] = e[col].clone();
            }
        }
        graph.push(clean(o));
    }
    let mut stories_out = Vec::new();
    let mut exported_ids: HashSet<String> = HashSet::new();
    for st in q("SELECT * FROM story ORDER BY name, id")? {
        let id = s(&st, "id").unwrap_or_default().to_string();
        let links: Vec<Map<String, Value>> = qa("SELECT memory_id, note FROM story_memory WHERE story_id = ?1 ORDER BY position, memory_id", &id)?
            .into_iter()
            .filter(|r| s(r, "memory_id").is_some_and(|m| mem_ids.contains(m)))
            .collect();
        let ms: Vec<Value> = links.iter().filter_map(|r| s(r, "memory_id").map(mdpid)).collect();
        let notes = crate::jsonl::memory_notes(&links);
        if ms.is_empty() {
            left.empty_stories += 1;
            continue;
        }
        exported_ids.insert(id.clone());
        let genre = if s(&st, "genre") == Some("tribute") { "tribute" } else { "story" };
        stories_out.push(clean(json!({"@id": mdpid(&id), "@type": "CreativeWork", "genre": genre, "name": st["name"],
            "description": st["introduction"], "focus": s(&st, "focus_id").map(mdpid), "memories": ms,
            "memoryNotes": if notes.is_empty() { Value::Null } else { Value::Array(notes) }})));
    }
    graph.extend(stories_out);

    // Facts written into each file come from the memories that hold it.
    let mut fact_people: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut fact_memory: HashMap<String, Map<String, Value>> = HashMap::new();
    for m in &mems {
        let id = s(m, "id").unwrap_or_default().to_string();
        let Some(contents) = mem_content.get(&id) else { continue };
        exported_ids.insert(id.clone());
        let people = qa("SELECT * FROM memory_individual WHERE memory_id = ?1 ORDER BY rowid", &id)?;
        let links: Vec<Value> = people
            .iter()
            .map(|r| clean(json!({"individual": mdpid(s(r, "individual_id").unwrap_or_default()), "role": r["role"],
                "roleNote": r["role_note"], "certainty": r["certainty"]})))
            .collect();
        for c in contents {
            let e = fact_people.entry(c.clone()).or_default();
            for r in &people {
                let who = s(r, "individual_id").unwrap_or_default();
                if let Some(n) = names.get(who) {
                    let pair = (n.clone(), s(r, "role").unwrap_or_default().to_string());
                    if !e.contains(&pair) {
                        e.push(pair);
                    }
                }
            }
            fact_memory.entry(c.clone()).or_insert_with(|| m.clone());
        }
        let nearest = s(m, "date").map(str::to_string).or_else(|| s(m, "date_edtf").and_then(dates::nearest));
        graph.push(clean(json!({"@id": mdpid(&id), "@type": "Memory", "name": m["name"], "temporalCoverage": nearest,
            "dateEdtf": m["date_edtf"], "description": m["caption"], "status": m["status"],
            "contentLocation": s(m, "place_id").map(mdpid), "event": s(m, "event_id").map(mdpid),
            "cover": s(m, "cover_id").filter(|c| contents.iter().any(|x| x == c)).map(mdpid),
            "contents": contents.iter().map(|c| mdpid(c)).collect::<Vec<_>>(), "people": links})));
    }

    // Content and its files.
    let mut files = Vec::new();
    let mut facts_map = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    let content = q("SELECT c.*, p.width AS pw, p.height AS ph, p.people_note AS pn, p.latitude AS plat, p.longitude AS plon, p.turn AS pturn, \
                     v.duration AS vd, v.language AS vl, v.speaker_note AS vs, v.quiet_start AS vq, v.play_full AS vf, \
                     w.duration AS wd, w.language AS wl, w.width AS ww, w.height AS wh, w.people_note AS wpn, w.speaker_note AS wsn, \
                     t.summary, t.transcribed_by, t.review_status, t.language AS tl \
                     FROM content c LEFT JOIN picture p USING (id) LEFT JOIN voice_recording v USING (id) \
                     LEFT JOIN video w USING (id) LEFT JOIN text_content t USING (id) ORDER BY c.id")?;
    let by_id: HashMap<String, &Map<String, Value>> = content.iter().map(|c| (s(c, "id").unwrap_or_default().to_string(), c)).collect();
    for c in &content {
        let id = s(c, "id").unwrap_or_default().to_string();
        if !held.contains(&id) {
            continue;
        }
        exported_ids.insert(id.clone());
        let kind = s(c, "kind").unwrap_or_default().to_string();
        let mut o = json!({"@id": mdpid(&id), "@type": schema_type(&kind), "name": c["name"],
            "dateCreated": s(c, "date").map(str::to_string).or_else(|| s(c, "date_edtf").and_then(dates::nearest)),
            "dateEdtf": c["date_edtf"], "description": c["description"], "creditText": c["author"],
            "copyrightNotice": c["copyright"], "sourceNote": c["source_note"], "dateAdded": c["date_added"], "addedBy": c["added_by"]});
        match kind.as_str() {
            "picture" => {
                o["width"] = c["pw"].clone();
                o["height"] = c["ph"].clone();
                o["peopleNote"] = c["pn"].clone();
            }
            "voice recording" => {
                o["duration"] = c["vd"].clone();
                o["inLanguage"] = c["vl"].clone();
                o["speakerNote"] = c["vs"].clone();
                // A quiet start is skipped when played; the file itself is whole.
                if let Some(q) = c["vq"].as_f64().filter(|q| *q > 0.0 && c["vf"].as_i64() != Some(1)) {
                    o["mdp:playFrom"] = json!(q);
                }
            }
            "video" => {
                o["duration"] = c["wd"].clone();
                o["inLanguage"] = c["wl"].clone();
                o["width"] = c["ww"].clone();
                o["height"] = c["wh"].clone();
                o["peopleNote"] = c["wpn"].clone();
                o["speakerNote"] = c["wsn"].clone();
            }
            _ => {
                o["abstract"] = c["summary"].clone();
                o["transcribedBy"] = c["transcribed_by"].clone();
                o["reviewStatus"] = c["review_status"].clone();
                o["inLanguage"] = c["tl"].clone();
            }
        }
        // A transcript (or other made-from file) sits beside the item it belongs to.
        let mut place_as = c;
        for r in rels.iter().filter(|r| s(r, "from_id") == Some(&id)) {
            let to = s(r, "to_id").unwrap_or_default();
            if held.contains(to) {
                o[relationship_name(s(r, "kind").unwrap_or_default())] = mdpid(to);
            }
            if s(r, "kind") == Some("transcript of") {
                if let Some(t) = by_id.get(to) {
                    place_as = *t;
                }
            }
        }
        let place_kind = s(place_as, "kind").unwrap_or_default().to_string();
        // Content with no date of its own is named by its memory's date.
        let place_mem = fact_memory.get(s(place_as, "id").unwrap_or_default());
        let (edtf, nearest) = if s(place_as, "date_edtf").is_some() || s(place_as, "date").is_some() {
            (s(place_as, "date_edtf").map(str::to_string), s(place_as, "date").map(str::to_string))
        } else {
            (place_mem.and_then(|m| s(m, "date_edtf")).map(str::to_string), place_mem.and_then(|m| s(m, "date")).map(str::to_string))
        };
        let mut enc = Vec::new();
        for f in qa("SELECT * FROM content_file WHERE content_id = ?1 ORDER BY is_copy, path", &id)? {
            let rel = s(&f, "path").unwrap_or_default().to_string();
            let ext = Path::new(&rel).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
            let first = export_name(&place_kind, s(c, "name").unwrap_or_default(), &id, edtf.as_deref(), nearest.as_deref(), &ext);
            let mut dest = first.clone();
            let mut n = 2;
            while used.contains(&dest.to_ascii_lowercase()) {
                dest = export_name(&place_kind, &format!("{} {n}", s(c, "name").unwrap_or_default()), &id, edtf.as_deref(), nearest.as_deref(), &ext);
                // A long name is cut short, which can cut off the number: number the file itself.
                if used.contains(&dest.to_ascii_lowercase()) {
                    dest = match first.rsplit_once('.') {
                        Some((stem, e)) if !stem.ends_with('/') => format!("{stem}-{n}.{e}"),
                        _ => format!("{first}-{n}"),
                    };
                }
                n += 1;
            }
            used.insert(dest.to_ascii_lowercase());
            let is_copy = f.get("is_copy").and_then(|v| v.as_i64()) == Some(1);
            enc.push(clean(json!({"@type": "MediaObject", "contentUrl": dest, "encodingFormat": f["format"], "contentSize": f["size"],
                "sha256": f["sha256"], "copy": is_copy, "originalName": f["original_name"]})));
            files.push(FilePlan {
                content_id: id.clone(),
                source: folder::absolute(root, &rel),
                working: rel,
                dest,
                format: s(&f, "format").unwrap_or_default().to_string(),
                is_copy,
            });
        }
        o["encoding"] = Value::Array(enc);
        graph.push(clean(o));

        // The facts for this file.
        let mem = fact_memory.get(&id).or_else(|| {
            rels.iter().filter(|r| s(r, "from_id") == Some(&id)).find_map(|r| fact_memory.get(s(r, "to_id").unwrap_or_default()))
        });
        let people = fact_people
            .get(&id)
            .cloned()
            .or_else(|| rels.iter().filter(|r| s(r, "from_id") == Some(&id)).find_map(|r| fact_people.get(s(r, "to_id").unwrap_or_default()).cloned()))
            .unwrap_or_default();
        let place = mem.and_then(|m| s(m, "place_id")).and_then(|p| place_names.get(p)).cloned();
        let date_edtf = s(c, "date_edtf").map(str::to_string).or_else(|| mem.and_then(|m| s(m, "date_edtf")).map(str::to_string));
        let date = s(c, "date").map(str::to_string).or_else(|| date_edtf.as_deref().and_then(dates::nearest));
        facts_map.insert(id.clone(), facts::Facts {
            id: id.clone(),
            title: s(c, "name").unwrap_or_default().to_string(),
            date,
            date_words: date_edtf.as_deref().map(dates::words),
            description: s(c, "description").or(mem.and_then(|m| s(m, "caption"))).map(str::to_string),
            people: people.iter().map(|p| p.0.clone()).fold(Vec::new(), |mut v, n| {
                if !v.contains(&n) {
                    v.push(n);
                }
                v
            }),
            in_picture: people.iter().filter(|p| matches!(p.1.as_str(), "in the picture" | "on camera")).map(|p| p.0.clone()).collect(),
            place: place.as_ref().map(|p| p.0.clone()),
            coords: place.as_ref().and_then(|p| p.1),
            credit: s(c, "author").map(str::to_string),
            copyright: s(c, "copyright").map(str::to_string),
            people_note: s(c, "pn").or(s(c, "wpn")).map(str::to_string),
            speaker_note: s(c, "vs").or(s(c, "wsn")).map(str::to_string),
            transcript: None,
            // A picture's or video's own GPS is as exact as an address, so it
            // follows the exact-places setting.
            remove_location: !privacy.exact_places,
            has_gps: c.get("plat").is_some_and(|v| !v.is_null()),
            turn: c.get("pturn").and_then(|v| v.as_i64()).unwrap_or(0),
        });
    }
    // MP3 copies also carry their transcript.
    for c in &content {
        let id = s(c, "id").unwrap_or_default();
        if !held.contains(id) {
            continue;
        }
        if let Some(r) = rels.iter().find(|r| s(r, "from_id") == Some(id) && s(r, "kind") == Some("transcript of")) {
            let to = s(r, "to_id").unwrap_or_default().to_string();
            let text = files.iter().find(|f| f.content_id == id && f.format == "text/plain").and_then(|f| read_text(&f.source));
            if let (Some(t), Some(f)) = (text, facts_map.get_mut(&to)) {
                f.transcript = Some(t);
            }
        }
    }

    let jr = q("SELECT * FROM journey LIMIT 1")?.into_iter().next().ok_or_else(|| Error::Invalid("The catalog has no Journey.".into()))?;
    let featured: Vec<Value> = q("SELECT item_id FROM featured ORDER BY position")?
        .iter()
        .filter_map(|r| s(r, "item_id").filter(|i| exported_ids.contains(*i)).map(mdpid))
        .collect();
    let mut cat = Map::new();
    cat.insert("@context".into(), crate::jsonl::CONTEXT_FILE.into());
    cat.insert("@id".into(), mdpid(s(&jr, "id").unwrap_or_default()));
    cat.insert("@type".into(), "Journey".into());
    cat.insert("name".into(), jr["name"].clone());
    if let Some(d) = s(&jr, "introduction") {
        cat.insert("description".into(), d.into());
    }
    cat.insert("inLanguage".into(), jr["language"].clone());
    cat.insert("formatVersion".into(), jr["format_version"].clone());
    cat.insert("dateModified".into(), last_changed(conn)?.into());
    if !featured.is_empty() {
        cat.insert("featured".into(), Value::Array(featured));
    }
    cat.insert("@graph".into(), Value::Array(graph));
    Ok(Plan { catalog: Value::Object(cat), files, facts: facts_map, left_out: left, privacy })
}

/// A text file as text; old files that are not UTF-8 are read as Windows-1252-ish Latin-1.
pub(crate) fn read_text(p: &Path) -> Option<String> {
    let b = std::fs::read(p).ok()?;
    let t = match String::from_utf8(b) {
        Ok(t) => t,
        Err(e) => e.into_bytes().iter().map(|&c| c as char).collect(),
    };
    Some(t.trim_start_matches('\u{feff}').replace("\r\n", "\n"))
}

/// What an export or preview made.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub folder: String,
    pub pages: usize,
    pub files: usize,
    /// Files copied as they are because their facts could not be written.
    pub untagged: Vec<String>,
    /// Files of 4 GB or more: a FAT32 USB stick can't hold them.
    pub too_big_for_fat32: Vec<String>,
    pub left_out: LeftOut,
    pub note: String,
}

/// FAT32's largest file is one byte under 4 GiB.
pub const FAT32_LIMIT: u64 = 4 * 1024 * 1024 * 1024 - 1;

fn ensure_parent(p: &Path) -> Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    Ok(())
}

/// True when `dir` looks like an earlier export (so it may be replaced).
pub fn is_export(dir: &Path) -> bool {
    dir.join(ABOUT_FILE).is_file() && dir.join("catalog.json").is_file()
}

/// Try a rename or removal again for a few seconds while it is refused:
/// OneDrive, antivirus and File Explorer briefly hold files that were just
/// written, which Windows reports as "Access is denied".
fn patiently<T>(mut f: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut wait = 50;
    for _ in 0..12 {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(e),
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(wait));
                wait = (wait * 2).min(2000);
            }
        }
    }
    f()
}

fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    patiently(|| std::fs::rename(from, to))
}

fn remove_tree(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    // Copies made read-only elsewhere would stop the removal on Windows.
    for e in walk(dir) {
        let _ = crate::import::set_readonly(&e, false);
    }
    patiently(|| match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        r => r,
    })?;
    Ok(())
}

/// A free name for the old export while the new one is swapped in: one left
/// by an earlier export that could not be removed (OneDrive still syncing it,
/// say) is skipped rather than stopping this one.
fn free_previous(parent: &Path, name: &str) -> PathBuf {
    for n in 1.. {
        let p = if n == 1 { parent.join(format!("{name} {OLD}")) } else { parent.join(format!("{name} {OLD} {n}")) };
        if !p.exists() || (is_export(&p) && remove_tree(&p).is_ok()) {
            return p;
        }
    }
    unreachable!()
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
    }
    out
}

const BUILDING: &str = "(export in progress)";
const OLD: &str = "(previous export)";

/// Where the finished export is swapped in from.
enum Swap {
    /// Built beside the export folder, in this parent; the old one is moved aside first.
    Beside(PathBuf),
    /// Built inside the export folder, because its parent can't be written.
    Inside,
}

/// The new export's folder while it is written; removed unless it is finished.
struct Building(PathBuf, bool);

impl Building {
    fn new(dir: PathBuf) -> Result<Building> {
        remove_tree(&dir)?; // left by an export that was cut off
        std::fs::create_dir_all(&dir)?;
        Ok(Building(dir, false))
    }
    fn finish(mut self, dest: &Path) -> Result<()> {
        rename(&self.0, dest).map_err(|e| {
            Error::Invalid(format!("The new export is complete in \"{}\" but could not be moved into place ({e}). Close File Explorer windows showing it and try again.", self.0.display()))
        })?;
        self.1 = true;
        Ok(())
    }
    /// Built inside `dest`: move the old export's entries into a folder of
    /// their own, then the new ones up into `dest`, putting the old back if a
    /// move fails.
    fn finish_inside(self, dest: &Path) -> Result<()> {
        let old = dest.join(OLD);
        remove_tree(&old)?;
        std::fs::create_dir_all(&old)?;
        let entries = |dir: &Path, skip: &[&str]| -> Result<Vec<std::ffi::OsString>> {
            let mut v: Vec<_> = std::fs::read_dir(dir)?.flatten().map(|e| e.file_name()).filter(|n| !skip.iter().any(|s| n == s)).collect();
            v.sort();
            Ok(v)
        };
        let busy = |e: std::io::Error| Error::Invalid(format!("The previous export could not be replaced; close any of its files that are open and try again ({e})."));
        let mut moved_old = Vec::new();
        for n in entries(dest, &[BUILDING, OLD])? {
            if let Err(e) = rename(&dest.join(&n), &old.join(&n)) {
                for m in &moved_old {
                    let _ = rename(&old.join(m), &dest.join(m));
                }
                return Err(busy(e));
            }
            moved_old.push(n);
        }
        let mut moved_new = Vec::new();
        for n in entries(&self.0, &[])? {
            if let Err(e) = rename(&self.0.join(&n), &dest.join(&n)) {
                for m in &moved_new {
                    let _ = rename(&dest.join(m), &self.0.join(m));
                }
                for m in &moved_old {
                    let _ = rename(&old.join(m), &dest.join(m));
                }
                return Err(busy(e));
            }
            moved_new.push(n);
        }
        let _ = remove_tree(&old);
        let _ = remove_tree(&self.0);
        Ok(())
    }
}

impl Drop for Building {
    fn drop(&mut self) {
        if !self.1 {
            let _ = remove_tree(&self.0);
        }
    }
}

/// Write the export into `dest`: everything goes into
/// a new folder beside it, which replaces the previous export only once it
/// is complete. A folder that holds anything other than an earlier export is
/// never replaced.
pub fn write(plan: &Plan, dest: &Path, ctx: &JobContext) -> Result<Report> {
    let holds_other = dest.exists()
        && std::fs::read_dir(dest)?.flatten().any(|e| {
            let n = e.file_name();
            n != BUILDING && n != OLD
        })
        && !is_export(dest);
    if holds_other {
        return Err(Error::Invalid(format!(
            "{} already holds other files, so the export won't replace it. Choose an empty folder or a new one in Settings.",
            dest.display()
        )));
    }
    let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| Error::Invalid("Choose a folder for the export.".into()))?;
    let parent = dest.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let cannot = |e: std::io::Error| {
        Error::Invalid(format!("The app can't write to {} ({e}). Choose another export folder in Settings, such as one in Documents.", dest.display()))
    };
    std::fs::create_dir_all(&parent).map_err(cannot)?;
    // The new export is built beside the folder when the app may write there,
    // otherwise inside it (a folder chosen at the top of a drive, for example).
    let beside = parent.join(format!("{name} {BUILDING}"));
    let (staging, swap) = match Building::new(beside) {
        Ok(b) => (b, Swap::Beside(parent.clone())),
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            std::fs::create_dir_all(dest).map_err(cannot)?;
            (Building::new(dest.join(BUILDING)).map_err(|e| match e {
                Error::Io(e) => cannot(e),
                e => e,
            })?, Swap::Inside)
        }
        Err(Error::Io(e)) => return Err(cannot(e)),
        Err(e) => return Err(e),
    };
    let out = staging.0.clone();
    ctx.set_total(plan.files.len() as u64 + 2);
    let tool = crate::convert::find_tool();
    let mut cat = plan.catalog.clone();
    let mut untagged = Vec::new();
    let mut written: HashMap<String, (u64, String)> = HashMap::new();
    for f in &plan.files {
        ctx.check()?;
        let target = out.join(f.dest.replace('/', std::path::MAIN_SEPARATOR_STR));
        ensure_parent(&target)?;
        let label = f.dest.rsplit('/').next().unwrap_or(&f.dest).to_string();
        let facts = plan.facts.get(&f.content_id);
        match facts::write_copy(&f.source, &target, &f.format, facts, tool.as_ref()) {
            Ok(tagged) => {
                if !tagged {
                    untagged.push(label.clone());
                }
                let (sha, size) = crate::import::checksum(&target)?;
                written.insert(f.dest.clone(), (size, sha));
                ctx.advance(format!("Copying {label}"));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&target);
                ctx.skip(label, format!("The file could not be copied ({e})."));
            }
        }
    }
    // The catalog lists the exported files as they are (their checksums differ).
    if let Some(g) = cat.get_mut("@graph").and_then(|g| g.as_array_mut()) {
        for o in g.iter_mut() {
            if let Some(enc) = o.get_mut("encoding").and_then(|e| e.as_array_mut()) {
                enc.retain(|e| e["contentUrl"].as_str().is_some_and(|u| written.contains_key(u)));
                for e in enc.iter_mut() {
                    if let Some((size, sha)) = e["contentUrl"].as_str().and_then(|u| written.get(u)) {
                        e["contentSize"] = json!(size);
                        e["sha256"] = json!(sha);
                    }
                }
            }
        }
    }
    ctx.check()?;
    ctx.message("Making the pages");
    let text = |c: &str| plan.files.iter().find(|f| f.content_id == c && f.format == "text/plain").and_then(|f| read_text(&f.source));
    let n = pages::render(&cat, &out, &|u: &str| u.to_string(), &text, plan.privacy, None)?;
    ctx.advance("Making the pages");
    ctx.check()?;
    if plan.privacy.checksums {
        ctx.message("Listing a checksum for every file");
        write_manifest(&out, ctx)?;
    }

    // Swap in the new export: the old one is moved aside first, and put back if anything fails.
    ctx.message("Replacing the previous export");
    match swap {
        Swap::Beside(parent) => {
            let previous = free_previous(&parent, &name);
            let had_old = dest.exists();
            if had_old {
                rename(dest, &previous).map_err(|e| {
                    Error::Invalid(format!("The previous export could not be replaced; close any of its files that are open and try again ({e})."))
                })?;
            }
            match staging.finish(dest) {
                Ok(()) => {
                    // The old export, and any an earlier export could not remove.
                    let stale = format!("{name} {OLD}");
                    for e in std::fs::read_dir(&parent).into_iter().flatten().flatten() {
                        if e.file_name().to_string_lossy().starts_with(&stale) && is_export(&e.path()) {
                            let _ = remove_tree(&e.path());
                        }
                    }
                }
                Err(e) => {
                    if had_old {
                        let _ = rename(&previous, dest);
                    }
                    return Err(e);
                }
            }
        }
        Swap::Inside => staging.finish_inside(dest)?,
    }
    ctx.advance("Done");
    let note = plan.left_out.sentence();
    let mut too_big_for_fat32: Vec<String> = written.iter().filter(|(_, (size, _))| *size > FAT32_LIMIT).map(|(d, _)| d.clone()).collect();
    too_big_for_fat32.sort();
    Ok(Report { folder: dest.display().to_string(), pages: n, files: written.len(), untagged, too_big_for_fat32, left_out: plan.left_out.clone(), note })
}

/// Make the preview in the Journey's cache with
/// the same page templates. Pages point at the working files, so nothing is
/// copied; the files' facts are written only in the export.
pub fn preview(plan: &Plan, journey_root: &Path) -> Result<(PathBuf, Report)> {
    let out = journey_root.join(folder::CACHE_DIR).join(PREVIEW_DIR);
    remove_tree(&out)?;
    std::fs::create_dir_all(&out)?;
    let mut to_working: HashMap<&str, String> =
        plan.files.iter().map(|f| (f.dest.as_str(), format!("../../{}", f.working))).collect();
    // A turned picture shows from a turned copy, as the export writes it.
    let tool = crate::convert::find_tool();
    for f in &plan.files {
        let turned = plan.facts.get(&f.content_id).filter(|x| x.turn.rem_euclid(4) != 0);
        if turned.is_none() || !f.format.starts_with("image/") || f.format.starts_with("image/hei") || f.format == "image/avif" || f.format == "image/bmp" || f.format == "image/x-adobe-dng" {
            continue;
        }
        let target = out.join(f.dest.replace('/', std::path::MAIN_SEPARATOR_STR));
        ensure_parent(&target)?;
        if facts::write_copy(&f.source, &target, &f.format, turned, tool.as_ref()).is_ok() {
            to_working.insert(f.dest.as_str(), f.dest.clone());
        }
    }
    let map = |u: &str| to_working.get(u).cloned().unwrap_or_else(|| u.to_string());
    let text = |c: &str| plan.files.iter().find(|f| f.content_id == c && f.format == "text/plain").and_then(|f| read_text(&f.source));
    let note = plan.left_out.sentence();
    let n = pages::render(&plan.catalog, &out, &map, &text, plan.privacy, Some(&note))?;
    let report = Report {
        folder: out.display().to_string(),
        pages: n,
        files: plan.files.len(),
        untagged: vec![],
        too_big_for_fat32: vec![],
        left_out: plan.left_out.clone(),
        note,
    };
    Ok((out.join(FRONT), report))
}

impl Journey {
    /// What the export would leave out with this privacy setting.
    pub fn export_left_out(&self, privacy: Privacy) -> Result<LeftOut> {
        Ok(plan(self, privacy)?.left_out)
    }

    /// Export in one go, on the calling thread (the app runs [`plan`] under
    /// its lock and [`write`] in the background instead).
    pub fn export(&self, dest: &Path, privacy: Privacy, ctx: &JobContext) -> Result<Report> {
        let p = plan(self, privacy)?;
        write(&p, dest, ctx)
    }

    /// Make the preview and return the front door's path.
    pub fn preview(&self, privacy: Privacy) -> Result<(PathBuf, Report)> {
        let p = plan(self, privacy)?;
        preview(&p, self.root())
    }
}

/// The checksum list in an export (sprint 19).
pub const MANIFEST_FILE: &str = "manifest-sha256.txt";

/// Every file under `dir`, as paths with `/` from `dir`, A to Z.
fn all_files(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                todo.push(p);
            } else if let Ok(r) = p.strip_prefix(dir) {
                out.push(r.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Write `manifest-sha256.txt`: one line per file, its SHA-256 then two
/// spaces then its path, as BagIt (RFC 8493) manifests and `sha256sum -c`
/// read them. The export keeps its own layout (the front door at the top),
/// so the list is BagIt's manifest, not a whole bag.
pub fn write_manifest(dir: &Path, ctx: &JobContext) -> Result<usize> {
    let mut lines = String::new();
    let files = all_files(dir)?;
    let mut n = 0;
    for f in files.iter().filter(|f| *f != MANIFEST_FILE) {
        ctx.check()?;
        let (sha, _) = crate::import::checksum(&dir.join(f.replace('/', std::path::MAIN_SEPARATOR_STR)))?;
        lines.push_str(&format!("{sha}  {f}\n"));
        n += 1;
    }
    std::fs::write(dir.join(MANIFEST_FILE), lines)?;
    Ok(n)
}

/// Check an export against its checksum list: the files that are missing
/// or changed. Empty when every file is as it was.
pub fn check_manifest(dir: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(dir.join(MANIFEST_FILE))?;
    let mut bad = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Some((sha, path)) = line.split_once("  ") else { continue };
        match crate::import::checksum(&dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR))) {
            Ok((s, _)) if s == sha => {}
            _ => bad.push(path.to_string()),
        }
    }
    Ok(bad)
}

/// Every item in the catalog by `@id`, for the page templates.
pub(crate) fn index(cat: &Value) -> BTreeMap<String, Value> {
    cat["@graph"]
        .as_array()
        .map(|g| g.iter().filter_map(|o| o["@id"].as_str().map(|id| (id.to_string(), o.clone()))).collect())
        .unwrap_or_default()
}
