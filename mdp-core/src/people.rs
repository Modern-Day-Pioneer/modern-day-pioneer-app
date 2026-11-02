//! People, pets, families and relationships.
//!
//! Every change is one action in history. A person or pet is always a
//! base `individual` row plus its own `person` or `pet` row. A relationship is
//! stored once, from one individual to another, and each side reads it in
//! its own words. Removing anything unlinks only,
//! and [`Journey::removal_warning`] says first what will be unlinked.

use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::{json, Value};

use crate::dates::{self, DateAsKnown};
use crate::summary::Entry;
use crate::{id_key, Action, Error, Journey, Result, Row};

/// The kinds offered in the relationship list. A family's own term is
/// accepted too.
pub const RELATIONSHIP_KINDS: &[&str] = &[
    "parent", "child", "sibling", "partner", "grandparent", "grandchild", "uncle or aunt", "niece or nephew", "cousin",
    "in-law", "godparent", "godchild", "guardian", "ward", "close family friend",
];

/// Qualifiers for a parent (`individual_relationship.qualifier`).
pub const PARENT_QUALIFIERS: &[&str] = &["birth", "adoptive", "step", "foster"];

pub const GENDERS: &[&str] = &["male", "female"];

/// Kinds that are stored the other way round, so each link has one form.
fn stored_form(kind: &str) -> Option<&'static str> {
    match kind {
        "child" => Some("parent"),
        "grandchild" => Some("grandparent"),
        "niece or nephew" => Some("uncle or aunt"),
        "godchild" => Some("godparent"),
        "ward" => Some("guardian"),
        _ => None,
    }
}

/// How the other side reads a stored kind.
pub(crate) fn reverse(kind: &str) -> Option<&'static str> {
    match kind {
        "parent" => Some("child"),
        "grandparent" => Some("grandchild"),
        "uncle or aunt" => Some("niece or nephew"),
        "godparent" => Some("godchild"),
        "guardian" => Some("ward"),
        "sibling" => Some("sibling"),
        "partner" => Some("partner"),
        "cousin" => Some("cousin"),
        "in-law" => Some("in-law"),
        "close family friend" => Some("close family friend"),
        _ => None,
    }
}

/// A kind worded for a person of the given gender (gender only words things).
pub(crate) fn gendered(kind: &str, gender: Option<&str>) -> String {
    let pick = |m: &str, f: &str| match gender {
        Some("male") => m.to_string(),
        Some("female") => f.to_string(),
        _ => kind.to_string(),
    };
    match kind {
        "parent" => pick("father", "mother"),
        "child" => pick("son", "daughter"),
        "sibling" => pick("brother", "sister"),
        "grandparent" => pick("grandfather", "grandmother"),
        "grandchild" => pick("grandson", "granddaughter"),
        "uncle or aunt" => pick("uncle", "aunt"),
        "niece or nephew" => pick("nephew", "niece"),
        "godparent" => pick("godfather", "godmother"),
        "godchild" => pick("godson", "goddaughter"),
        _ => kind.to_string(),
    }
}

pub(crate) fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// A trimmed text value, or null when empty.
pub(crate) fn text_value(v: Option<&Value>) -> Value {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() => Value::from(s.trim()),
        Some(Value::Number(n)) => Value::Number(n.clone()),
        Some(Value::Bool(b)) => Value::Bool(*b),
        _ => Value::Null,
    }
}

pub(crate) fn date_value(edtf: Option<String>) -> Option<DateAsKnown> {
    let e = edtf?;
    Some(DateAsKnown { nearest: dates::nearest(&e), words: dates::words(&e), edtf: e })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RelationshipView {
    pub id: String,
    pub other_id: String,
    pub other_name: String,
    /// The whole line as this individual's screen shows it, for example
    /// "Father of Ellen Rowan" or "Daughter of Walter Rowan (birth)".
    pub words: String,
    /// The kind as stored, and whether this individual is its `from` side.
    pub kind: String,
    pub from_this_side: bool,
    pub qualifier: Option<String>,
    pub certainty: Option<String>,
    pub source_note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndividualView {
    pub id: String,
    /// "Person" or "Pet".
    pub kind: String,
    pub name: String,
    pub description: Option<String>,
    pub born: Option<DateAsKnown>,
    pub died: Option<DateAsKnown>,
    /// From the birth event.
    pub birthplace: Option<Entry>,
    pub birth_event: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub other_names: Option<String>,
    pub gender: Option<String>,
    pub living_override: Option<String>,
    /// For a person: living under the export's privacy rule, after any override.
    pub living: bool,
    pub species: Option<String>,
    pub breed: Option<String>,
    pub joined: Option<DateAsKnown>,
    pub families: Vec<Entry>,
    pub relationships: Vec<RelationshipView>,
    pub events: Vec<Entry>,
    /// Memories this individual is linked to, with the role as the note.
    pub memories: Vec<Entry>,
    /// Stories about them or holding one of their memories.
    pub stories: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FamilyView {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub members: Vec<Entry>,
    pub related: Vec<Entry>,
    pub events: Vec<Entry>,
    pub memories: Vec<Entry>,
    /// Stories about the family or holding one of its memories.
    pub stories: Vec<Entry>,
}

pub(crate) fn list(conn: &rusqlite::Connection, sql: &str, arg: &str) -> Result<Vec<Entry>> {
    let mut st = conn.prepare(sql)?;
    let rows = st.query_map([arg], |r| {
        Ok(Entry { id: r.get(0)?, name: r.get(1)?, note: r.get::<_, Option<String>>(2)?.unwrap_or_default() })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn exists(conn: &rusqlite::Connection, table: &str, id: &str) -> Result<bool> {
    Ok(conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1)"), [id], |r| r.get(0))?)
}

pub(crate) fn name_of(conn: &rusqlite::Connection, id: &str) -> Result<String> {
    for t in ["individual", "family", "place", "event", "story", "memory"] {
        if let Some(n) = conn.query_row(&format!("SELECT name FROM {t} WHERE id = ?1"), [id], |r| r.get(0)).optional()? {
            return Ok(n);
        }
    }
    Err(Error::Invalid("That item is no longer in the Journey.".into()))
}

/// Living, for the export's privacy rule: no death date, and born within the last 100 years or
/// birth date unknown, unless overridden.
pub fn is_living(death_edtf: Option<&str>, birth_edtf: Option<&str>, over: Option<&str>, this_year: i32) -> bool {
    match over {
        Some("living") => return true,
        Some("not living") => return false,
        _ => {}
    }
    if death_edtf.is_some() {
        return false;
    }
    match birth_edtf.and_then(dates::first_year) {
        Some(y) => this_year - y <= 100,
        None => true,
    }
}

/// The birth or death event whose first person is this individual.
pub(crate) fn life_event(conn: &rusqlite::Connection, individual: &str, kind: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT e.id FROM event e JOIN event_individual ei ON ei.event_id = e.id
             WHERE e.kind = ?2 AND ei.individual_id = ?1
               AND ei.position = (SELECT min(position) FROM event_individual WHERE event_id = e.id)
             ORDER BY e.id LIMIT 1",
            [individual, kind],
            |r| r.get(0),
        )
        .optional()?)
}

/// Make a person's birth or death event follow their date.
fn sync_life_event(a: &mut Action, individual: &str, kind: &str, date: Option<&DateAsKnown>) -> Result<()> {
    let existing = life_event(a.conn(), individual, kind)?;
    match (existing, date) {
        (Some(e), d) => {
            a.update("event", id_key(&e), "start_date_edtf", json!(d.map(|d| d.edtf.clone())))?;
            a.update("event", id_key(&e), "start_date", json!(d.and_then(|d| d.nearest.clone())))?;
        }
        (None, Some(d)) => {
            let name = name_of(a.conn(), individual)?;
            let title = format!("{} of {name}", if kind == "birth" { "Birth" } else { "Death" });
            let e = a.create("event", row(json!({"kind": kind, "name": title, "start_date": d.nearest, "start_date_edtf": d.edtf})))?;
            a.insert("event_individual", row(json!({"event_id": e, "individual_id": individual, "position": 0})))?;
        }
        (None, None) => {}
    }
    Ok(())
}

/// When a birth or death event's date changes, the first person's date follows.
pub(crate) fn sync_from_event(a: &mut Action, event: &str) -> Result<()> {
    let (kind, edtf, nearest): (String, Option<String>, Option<String>) =
        a.conn().query_row("SELECT kind, start_date_edtf, start_date FROM event WHERE id = ?1", [event], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let (date_col, edtf_col) = match kind.as_str() {
        "birth" => ("birth_date", "birth_date_edtf"),
        "death" => ("death_date", "death_date_edtf"),
        _ => return Ok(()),
    };
    let first: Option<String> = a
        .conn()
        .query_row(
            "SELECT ei.individual_id FROM event_individual ei JOIN individual i ON i.id = ei.individual_id
             WHERE ei.event_id = ?1 ORDER BY ei.position LIMIT 1",
            [event],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(p) = first {
        if life_event(a.conn(), &p, &kind)?.as_deref() == Some(event) {
            a.update("individual", id_key(&p), edtf_col, json!(edtf))?;
            a.update("individual", id_key(&p), date_col, json!(nearest))?;
        }
    }
    Ok(())
}

pub(crate) fn row(v: Value) -> Row {
    v.as_object().cloned().unwrap_or_default()
}

/// Fields of a person or pet that [`Journey::update_individual`] accepts, and
/// the table each lives in. Dates (`born`, `died`, `joined`) are typed as known.
const PERSON_FIELDS: &[&str] = &["given_name", "family_name", "other_names", "gender", "living_override"];
const PET_FIELDS: &[&str] = &["species", "breed"];

impl Journey {
    /// Add a person or pet with just a name: the quick-add offered wherever a
    /// name is asked for. A person's first and last words become their given
    /// and family names.
    pub fn add_individual(&mut self, kind: &str, name: &str) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("A name is needed.".into()));
        }
        let kind = match kind.to_lowercase().as_str() {
            "person" => "Person",
            "pet" => "Pet",
            _ => return Err(Error::Invalid(format!("\"{kind}\" isn't a person or a pet."))),
        };
        self.act(|a| {
            let id = a.create("individual", row(json!({"kind": kind, "name": name})))?;
            if kind == "Person" {
                let words: Vec<&str> = name.split_whitespace().collect();
                let (given, family) = match words.as_slice() {
                    [one] => (Some(*one), None),
                    [first, .., last] => (Some(*first), Some(*last)),
                    [] => (None, None),
                };
                a.insert("person", row(json!({"id": id, "given_name": given, "family_name": family})))?;
            } else {
                a.insert("pet", row(json!({"id": id})))?;
            }
            Ok(id)
        })
    }

    /// Change several fields of a person or pet as one action. Accepted:
    /// `name`, `description`, `born`, `died`, and for a person `given_name`,
    /// `family_name`, `other_names`, `gender`, `living_override`; for a pet
    /// `species`, `breed`, `joined`. Empty text clears a field.
    pub fn update_individual(&mut self, id: &str, changes: &Row) -> Result<()> {
        let kind: String = self
            .conn()
            .query_row("SELECT kind FROM individual WHERE id = ?1", [id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| Error::Invalid("That person or pet is no longer in the Journey.".into()))?;
        // Read dates before the action, so a bad date changes nothing.
        let mut parsed = std::collections::HashMap::new();
        for f in ["born", "died", "joined"] {
            if let Some(v) = changes.get(f) {
                parsed.insert(f, dates::parse(v.as_str().unwrap_or_default())?);
            }
        }
        for (k, v) in changes {
            let ok = matches!(k.as_str(), "name" | "description" | "born" | "died")
                || (kind == "Person" && PERSON_FIELDS.contains(&k.as_str()))
                || (kind == "Pet" && (PET_FIELDS.contains(&k.as_str()) || k == "joined"));
            if !ok {
                return Err(Error::Invalid(format!("{k} can't be changed on a {}.", kind.to_lowercase())));
            }
            if k == "name" && text_value(Some(v)).is_null() {
                return Err(Error::Invalid("A name is needed.".into()));
            }
            if k == "gender" && !text_value(Some(v)).is_null() && !GENDERS.contains(&v.as_str().unwrap_or_default()) {
                return Err(Error::Invalid("Gender is male, female or not given.".into()));
            }
            if k == "living_override" && !text_value(Some(v)).is_null() && !matches!(v.as_str(), Some("living" | "not living")) {
                return Err(Error::Invalid("Living is worked out, living or not living.".into()));
            }
        }
        let id = id.to_string();
        self.act(move |a| {
            let key = id_key(&id);
            for (k, v) in changes {
                match k.as_str() {
                    "name" | "description" => a.update("individual", key.clone(), k, text_value(Some(v)))?,
                    "born" | "died" => {
                        let d = parsed[k.as_str()].clone();
                        let (date_col, edtf_col, ev) =
                            if k == "born" { ("birth_date", "birth_date_edtf", "birth") } else { ("death_date", "death_date_edtf", "death") };
                        a.update("individual", key.clone(), edtf_col, json!(d.as_ref().map(|d| d.edtf.clone())))?;
                        a.update("individual", key.clone(), date_col, json!(d.as_ref().and_then(|d| d.nearest.clone())))?;
                        sync_life_event(a, &id, ev, d.as_ref())?;
                    }
                    "joined" => {
                        let d = parsed["joined"].clone();
                        a.update("pet", key.clone(), "joined_date_edtf", json!(d.map(|d| d.edtf)))?;
                    }
                    f if PERSON_FIELDS.contains(&f) => a.update("person", key.clone(), f, text_value(Some(v)))?,
                    f => a.update("pet", key.clone(), f, text_value(Some(v)))?,
                }
            }
            // Keep birth and death event names in step with the name.
            if changes.contains_key("name") {
                for ev in ["birth", "death"] {
                    if let Some(e) = life_event(a.conn(), &id, ev)? {
                        let name = name_of(a.conn(), &id)?;
                        let old: String = a.conn().query_row("SELECT name FROM event WHERE id = ?1", [&e], |r| r.get(0))?;
                        if old.starts_with("Birth of ") || old.starts_with("Death of ") {
                            let title = format!("{} of {name}", if ev == "birth" { "Birth" } else { "Death" });
                            a.update("event", id_key(&e), "name", json!(title))?;
                        }
                    }
                }
            }
            Ok(())
        })
    }

    /// Everything the person or pet screen shows.
    pub fn individual_view(&self, id: &str) -> Result<IndividualView> {
        let c = self.conn();
        let base: Option<(String, String, Option<String>, Option<String>, Option<String>)> = c
            .query_row(
                "SELECT kind, name, description, birth_date_edtf, death_date_edtf FROM individual WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let (kind, name, description, birth, death) =
            base.ok_or_else(|| Error::Invalid("That person or pet is no longer in the Journey.".into()))?;
        let person: Option<[Option<String>; 5]> = c
            .query_row("SELECT given_name, family_name, other_names, gender, living_override FROM person WHERE id = ?1", [id], |r| {
                Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?])
            })
            .optional()?;
        let pet: Option<[Option<String>; 3]> = c
            .query_row("SELECT species, breed, joined_date_edtf FROM pet WHERE id = ?1", [id], |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?]))
            .optional()?;
        let [given_name, family_name, other_names, gender, living_override] = person.clone().unwrap_or_default();
        let [species, breed, joined] = pet.unwrap_or_default();
        let birth_event = life_event(c, id, "birth")?;
        let birthplace = match &birth_event {
            Some(e) => list(c, "SELECT p.id, p.name, '' FROM event e JOIN place p ON p.id = e.place_id WHERE e.id = ?1", e)?.pop(),
            None => None,
        };
        let this_year: i32 = chrono::Local::now().format("%Y").to_string().parse().unwrap_or(2026);
        let living = person.is_some() && is_living(death.as_deref(), birth.as_deref(), living_override.as_deref(), this_year);
        Ok(IndividualView {
            id: id.to_string(),
            kind,
            name,
            description,
            born: date_value(birth),
            died: date_value(death),
            birthplace,
            birth_event,
            given_name,
            family_name,
            other_names,
            gender,
            living_override,
            living,
            species,
            breed,
            joined: date_value(joined),
            families: list(c, "SELECT f.id, f.name, '' FROM family_member fm JOIN family f ON f.id = fm.family_id WHERE fm.individual_id = ?1 ORDER BY f.name", id)?,
            relationships: self.relationships_of(id)?,
            events: events_of(c, id)?,
            memories: memories_of(c, id)?,
            stories: stories_of(c, id)?,
        })
    }

    /// One individual's relationships, each worded from their side.
    pub fn relationships_of(&self, id: &str) -> Result<Vec<RelationshipView>> {
        let c = self.conn();
        let gender_of = |who: &str| -> Result<Option<String>> {
            Ok(c.query_row("SELECT gender FROM person WHERE id = ?1", [who], |r| r.get(0)).optional()?.flatten())
        };
        let mine = gender_of(id)?;
        let mut st = c.prepare(
            "SELECT r.id, r.from_id, r.to_id, r.kind, r.qualifier, r.certainty, r.source_note, i.name
             FROM individual_relationship r JOIN individual i ON i.id = CASE WHEN r.from_id = ?1 THEN r.to_id ELSE r.from_id END
             WHERE (r.from_id = ?1 OR r.to_id = ?1) AND r.status IN ('entered', 'accepted')
             ORDER BY i.name",
        )?;
        let rows: Vec<(String, String, String, String, Option<String>, Option<String>, Option<String>, String)> = st
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?
            .collect::<std::result::Result<_, _>>()?;
        let mut out = vec![];
        for (rid, from, to, kind, qualifier, certainty, source_note, other_name) in rows {
            let from_this_side = from == id;
            let other_id = if from_this_side { to } else { from };
            let words = match (from_this_side, reverse(&kind)) {
                (true, _) => format!("{} of {other_name}", capital(&gendered(&kind, mine.as_deref()))),
                (false, Some(rev)) => format!("{} of {other_name}", capital(&gendered(rev, mine.as_deref()))),
                (false, None) => format!("{other_name} is their {kind}"),
            };
            let mut words = words;
            if let Some(q) = &qualifier {
                words.push_str(&format!(" ({q})"));
            }
            if certainty.as_deref() == Some("probably") {
                words.push_str(", probably");
            }
            out.push(RelationshipView { id: rid, other_id, other_name, words, kind, from_this_side, qualifier, certainty, source_note });
        }
        Ok(out)
    }

    /// Enter a relationship by hand: `from` is the `kind` of `to` ("Walter is
    /// the parent of Ellen"). Reversed kinds are stored the other way round,
    /// so each link has one form. No suggestions are made (release 1).
    pub fn add_relationship(
        &mut self,
        from: &str,
        kind: &str,
        to: &str,
        qualifier: Option<&str>,
        certainty: Option<&str>,
        source_note: Option<&str>,
    ) -> Result<String> {
        let kind = kind.trim().to_lowercase();
        if kind.is_empty() {
            return Err(Error::Invalid("Choose what the relationship is.".into()));
        }
        if from == to {
            return Err(Error::Invalid("A relationship needs two different people or pets.".into()));
        }
        for who in [from, to] {
            if !exists(self.conn(), "individual", who)? {
                return Err(Error::Invalid("That person or pet is no longer in the Journey.".into()));
            }
        }
        let (from, to, kind) = match stored_form(&kind) {
            Some(k) => (to, from, k.to_string()),
            None => (from, to, kind),
        };
        let qualifier = qualifier.map(str::trim).filter(|q| !q.is_empty());
        if qualifier.is_some() && kind != "parent" {
            return Err(Error::Invalid("Birth, adoptive, step and foster apply to parents only.".into()));
        }
        let certainty = match certainty {
            Some("probably") => Some("probably"),
            _ => Some("certain"),
        };
        let dup: bool = self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM individual_relationship WHERE kind = ?3 AND
               ((from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1 AND kind NOT IN ('parent', 'grandparent', 'uncle or aunt', 'godparent', 'guardian'))))",
            [from, to, kind.as_str()],
            |r| r.get(0),
        )?;
        if dup {
            return Err(Error::Invalid("That relationship is already there.".into()));
        }
        let source_note = source_note.map(str::trim).filter(|s| !s.is_empty());
        self.act(|a| {
            a.create(
                "individual_relationship",
                row(json!({"from_id": from, "to_id": to, "kind": kind, "qualifier": qualifier, "certainty": certainty,
                           "source_note": source_note, "status": "entered"})),
            )
        })
    }

    /// Change a relationship's qualifier, certainty or source note.
    pub fn update_relationship(&mut self, id: &str, changes: &Row) -> Result<()> {
        if !exists(self.conn(), "individual_relationship", id)? {
            return Err(Error::Invalid("That relationship is no longer in the Journey.".into()));
        }
        for k in changes.keys() {
            if !matches!(k.as_str(), "qualifier" | "certainty" | "source_note") {
                return Err(Error::Invalid(format!("{k} can't be changed on a relationship; remove it and add it again.")));
            }
        }
        self.act(|a| {
            for (k, v) in changes {
                let v = match (k.as_str(), text_value(Some(v))) {
                    ("certainty", Value::Null) => json!("certain"),
                    (_, v) => v,
                };
                a.update("individual_relationship", id_key(id), k, v)?;
            }
            Ok(())
        })
    }

    pub fn remove_relationship(&mut self, id: &str) -> Result<()> {
        if !exists(self.conn(), "individual_relationship", id)? {
            return Err(Error::Invalid("That relationship is no longer in the Journey.".into()));
        }
        self.act(|a| a.delete_item("individual_relationship", id))
    }

    pub fn add_family(&mut self, name: &str) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("A family needs a name.".into()));
        }
        self.act(|a| a.create("family", row(json!({"name": name}))))
    }

    /// Change a family's `name` or `description`.
    pub fn update_family(&mut self, id: &str, changes: &Row) -> Result<()> {
        if !exists(self.conn(), "family", id)? {
            return Err(Error::Invalid("That family is no longer in the Journey.".into()));
        }
        for (k, v) in changes {
            if !matches!(k.as_str(), "name" | "description") {
                return Err(Error::Invalid(format!("{k} can't be changed on a family.")));
            }
            if k == "name" && text_value(Some(v)).is_null() {
                return Err(Error::Invalid("A family needs a name.".into()));
            }
        }
        self.act(|a| {
            for (k, v) in changes {
                a.update("family", id_key(id), k, text_value(Some(v)))?;
            }
            Ok(())
        })
    }

    pub fn add_family_member(&mut self, family: &str, individual: &str) -> Result<()> {
        if !exists(self.conn(), "family", family)? || !exists(self.conn(), "individual", individual)? {
            return Err(Error::Invalid("That family or person is no longer in the Journey.".into()));
        }
        let there: bool = self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM family_member WHERE family_id = ?1 AND individual_id = ?2)",
            [family, individual],
            |r| r.get(0),
        )?;
        if there {
            return Ok(());
        }
        self.act(|a| a.insert("family_member", row(json!({"family_id": family, "individual_id": individual}))))
    }

    /// Add several people and pets to a family in one action (one undo).
    /// Returns how many were not in it already (sprint 8).
    pub fn add_family_members(&mut self, family: &str, individuals: &[String]) -> Result<usize> {
        if !exists(self.conn(), "family", family)? {
            return Err(Error::Invalid("That family is no longer in the Journey.".into()));
        }
        let mut new = Vec::new();
        for i in individuals {
            if !exists(self.conn(), "individual", i)? {
                return Err(Error::Invalid("Someone chosen is no longer in the Journey.".into()));
            }
            let there: bool = self.conn().query_row(
                "SELECT EXISTS(SELECT 1 FROM family_member WHERE family_id = ?1 AND individual_id = ?2)",
                [family, i.as_str()],
                |r| r.get(0),
            )?;
            if !there && !new.contains(i) {
                new.push(i.clone());
            }
        }
        if new.is_empty() {
            return Ok(0);
        }
        self.act(|a| {
            for i in &new {
                a.insert("family_member", row(json!({"family_id": family, "individual_id": i})))?;
            }
            Ok(())
        })?;
        Ok(new.len())
    }

    pub fn remove_family_member(&mut self, family: &str, individual: &str) -> Result<()> {
        self.act(|a| a.delete("family_member", row(json!({"family_id": family, "individual_id": individual}))))
    }

    /// Link two families (no kind). Stored once.
    pub fn relate_families(&mut self, a_id: &str, b_id: &str) -> Result<()> {
        if a_id == b_id {
            return Err(Error::Invalid("Choose a different family.".into()));
        }
        if !exists(self.conn(), "family", a_id)? || !exists(self.conn(), "family", b_id)? {
            return Err(Error::Invalid("That family is no longer in the Journey.".into()));
        }
        let there: bool = self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM family_relationship WHERE (from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1))",
            [a_id, b_id],
            |r| r.get(0),
        )?;
        if there {
            return Ok(());
        }
        self.act(|a| a.insert("family_relationship", row(json!({"from_id": a_id, "to_id": b_id}))))
    }

    pub fn unrelate_families(&mut self, a_id: &str, b_id: &str) -> Result<()> {
        self.act(|a| {
            for (f, t) in [(a_id, b_id), (b_id, a_id)] {
                let there: bool = a.conn().query_row(
                    "SELECT EXISTS(SELECT 1 FROM family_relationship WHERE from_id = ?1 AND to_id = ?2)",
                    [f, t],
                    |r| r.get(0),
                )?;
                if there {
                    a.delete("family_relationship", row(json!({"from_id": f, "to_id": t})))?;
                }
            }
            Ok(())
        })
    }

    pub fn family_view(&self, id: &str) -> Result<FamilyView> {
        let c = self.conn();
        let (name, description): (String, Option<String>) = c
            .query_row("SELECT name, description FROM family WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .ok_or_else(|| Error::Invalid("That family is no longer in the Journey.".into()))?;
        Ok(FamilyView {
            id: id.to_string(),
            name,
            description,
            members: list(
                c,
                "SELECT i.id, i.name, CASE WHEN i.kind = 'Pet' THEN coalesce(p.species, 'pet') ELSE '' END
                 FROM family_member fm JOIN individual i ON i.id = fm.individual_id LEFT JOIN pet p ON p.id = i.id
                 WHERE fm.family_id = ?1 ORDER BY i.kind DESC, i.name",
                id,
            )?,
            related: list(
                c,
                "SELECT f.id, f.name, '' FROM family_relationship r JOIN family f ON f.id = CASE WHEN r.from_id = ?1 THEN r.to_id ELSE r.from_id END
                 WHERE r.from_id = ?1 OR r.to_id = ?1 ORDER BY f.name",
                id,
            )?,
            events: events_of(c, id)?,
            memories: memories_of(c, id)?,
            stories: stories_of(c, id)?,
        })
    }
}

pub(crate) fn events_of(c: &rusqlite::Connection, id: &str) -> Result<Vec<Entry>> {
    let mut out = list(
        c,
        "SELECT e.id, e.name, trim(coalesce(e.start_date_edtf, '') || CASE WHEN p.name IS NULL THEN '' ELSE ' · ' || p.name END)
         FROM event_individual ei JOIN event e ON e.id = ei.event_id LEFT JOIN place p ON p.id = e.place_id
         WHERE ei.individual_id = ?1 ORDER BY e.start_date IS NULL, e.start_date, e.name",
        id,
    )?;
    for e in &mut out {
        if let Some((d, rest)) = e.note.split_once(" · ").map(|(a, b)| (a.to_string(), b.to_string())) {
            e.note = format!("{} · {rest}", dates::words(&d));
        } else if !e.note.is_empty() && !e.note.starts_with('·') {
            e.note = dates::words(&e.note);
        }
        e.note = e.note.trim_start_matches("· ").to_string();
    }
    Ok(out)
}

pub(crate) fn stories_of(c: &rusqlite::Connection, id: &str) -> Result<Vec<Entry>> {
    list(
        c,
        "SELECT s.id, s.name, CASE WHEN s.focus_id = ?1 THEN 'About them' ELSE '' END FROM story s
         WHERE s.focus_id = ?1 OR EXISTS (SELECT 1 FROM story_memory sm JOIN memory_individual mi ON mi.memory_id = sm.memory_id
                                          WHERE sm.story_id = s.id AND mi.individual_id = ?1)
         ORDER BY s.name",
        id,
    )
}

pub(crate) fn memories_of(c: &rusqlite::Connection, id: &str) -> Result<Vec<Entry>> {
    list(
        c,
        "SELECT m.id, m.name, group_concat(CASE WHEN mi.role = 'other' THEN coalesce(mi.role_note, 'other') ELSE mi.role END, ', ')
         FROM memory_individual mi JOIN memory m ON m.id = mi.memory_id WHERE mi.individual_id = ?1
         GROUP BY m.id ORDER BY m.date IS NULL, m.date, m.name",
        id,
    )
}
