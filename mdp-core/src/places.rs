//! Places and events, merging places, and removing people, pets, families,
//! places and events.
//!
//! Only memories and events link to a place. Events come from the fixed
//! list. Removing unlinks only: linked items stay, and
//! [`Journey::removal_warning`] lists what will be unlinked first.

use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::{json, Value};

use crate::dates::{self, DateAsKnown};
use crate::people::{date_value, events_of, list, memories_of, name_of, row, sync_from_event, text_value};
use crate::summary::Entry;
use crate::{id_key, Action, Error, Journey, Result, Row};

/// The fixed event kinds.
pub const EVENT_KINDS: &[&str] =
    &["birth", "death", "wedding", "divorce", "move", "graduation", "reunion", "funeral", "holiday", "trip"];

#[derive(Debug, Clone, Serialize)]
pub struct PlaceView {
    pub id: String,
    pub name: String,
    pub address: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub exact: bool,
    pub description: Option<String>,
    pub events: Vec<Entry>,
    pub memories: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub start: Option<DateAsKnown>,
    pub end: Option<DateAsKnown>,
    pub place: Option<Entry>,
    pub description: Option<String>,
    /// People, pets and families who took part, in order (for a birth, the person born first).
    pub people: Vec<Entry>,
    pub memories: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlacesAndEvents {
    pub places: Vec<Entry>,
    pub events: Vec<Entry>,
}

/// Read "41.223, -111.974" into a latitude and longitude.
pub fn parse_coordinates(text: &str) -> Result<Option<(f64, f64)>> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let bad = || Error::Invalid(format!("\"{t}\" isn't a pair of coordinates. Type them like 41.223, -111.974 (latitude, then longitude)."));
    let parts: Vec<&str> = t.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
    let [lat, lon] = parts.as_slice() else { return Err(bad()) };
    let lat: f64 = lat.trim_end_matches('°').parse().map_err(|_| bad())?;
    let lon: f64 = lon.trim_end_matches('°').parse().map_err(|_| bad())?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(Error::Invalid("Latitude goes from -90 to 90 and longitude from -180 to 180.".into()));
    }
    Ok(Some((lat, lon)))
}

fn exists(conn: &rusqlite::Connection, table: &str, id: &str) -> Result<bool> {
    Ok(conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1)"), [id], |r| r.get(0))?)
}

/// Which table holds an item.
fn table_of(conn: &rusqlite::Connection, id: &str) -> Result<String> {
    conn.query_row("SELECT table_name FROM item WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::Invalid("That item is no longer in the Journey.".into()))
}

fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => {
            // "Walter Rowan and June Rowan" reads better as "Walter and June Rowan".
            let (af, al) = a.rsplit_once(' ').unwrap_or((a, ""));
            let (_, bl) = b.rsplit_once(' ').unwrap_or((b, ""));
            if !al.is_empty() && al == bl { format!("{af} and {b}") } else { format!("{a} and {b}") }
        }
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// A name for an event from its kind and who took part, such as "Birth of June Rowan".
pub fn suggest_event_name(kind: &str, names: &[String]) -> String {
    let who = join_names(names);
    let label = match kind {
        "birth" => "Birth",
        "death" => "Death",
        "wedding" => "Wedding",
        "divorce" => "Divorce",
        "move" => "Move",
        "graduation" => "Graduation",
        "reunion" => "Reunion",
        "funeral" => "Funeral",
        "holiday" => "Holiday",
        "trip" => "Trip",
        other => other,
    };
    if who.is_empty() {
        return label.to_string();
    }
    match kind {
        "holiday" | "trip" | "reunion" => format!("{label} with {who}"),
        _ => format!("{label} of {who}"),
    }
}

impl Journey {
    /// Which table holds an item ("individual", "family", "place", "event", ...),
    /// so a screen can open the right view.
    pub fn item_table(&self, id: &str) -> Result<String> {
        table_of(self.conn(), id)
    }

    pub fn places_and_events(&self) -> Result<PlacesAndEvents> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT p.id, p.name, (SELECT count(*) FROM memory WHERE place_id = p.id) + (SELECT count(*) FROM event WHERE place_id = p.id)
             FROM place p ORDER BY p.name COLLATE NOCASE",
        )?;
        let places = st
            .query_map([], |r| {
                let n: i64 = r.get(2)?;
                Ok(Entry { id: r.get(0)?, name: r.get(1)?, note: if n == 1 { "1 link".into() } else { format!("{n} links") } })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut st = c.prepare(
            "SELECT e.id, e.name, e.start_date_edtf, p.name FROM event e LEFT JOIN place p ON p.id = e.place_id
             ORDER BY e.start_date IS NULL, e.start_date, e.name",
        )?;
        let events = st
            .query_map([], |r| {
                let d: Option<String> = r.get(2)?;
                let p: Option<String> = r.get(3)?;
                let note = [d.map(|d| dates::words(&d)), p].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                Ok(Entry { id: r.get(0)?, name: r.get(1)?, note })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(PlacesAndEvents { places, events })
    }

    pub fn add_place(&mut self, name: &str) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("A place needs a name.".into()));
        }
        self.act(|a| a.create("place", row(json!({"name": name}))))
    }

    /// Change a place: `name`, `address`, `description`, `exact` (true or
    /// false) and `coordinates` ("41.223, -111.974", empty to clear).
    pub fn update_place(&mut self, id: &str, changes: &Row) -> Result<()> {
        if !exists(self.conn(), "place", id)? {
            return Err(Error::Invalid("That place is no longer in the Journey.".into()));
        }
        let mut coords = None;
        for (k, v) in changes {
            match k.as_str() {
                "name" if text_value(Some(v)).is_null() => return Err(Error::Invalid("A place needs a name.".into())),
                "name" | "address" | "description" | "exact" => {}
                "coordinates" => coords = Some(parse_coordinates(v.as_str().unwrap_or_default())?),
                _ => return Err(Error::Invalid(format!("{k} can't be changed on a place."))),
            }
        }
        self.act(|a| {
            for (k, v) in changes {
                match k.as_str() {
                    "exact" => a.update("place", id_key(id), "exact", json!(v.as_bool().unwrap_or(false) as i64))?,
                    "coordinates" => {
                        let c = coords.flatten();
                        a.update("place", id_key(id), "latitude", json!(c.map(|c| c.0)))?;
                        a.update("place", id_key(id), "longitude", json!(c.map(|c| c.1)))?;
                    }
                    f => a.update("place", id_key(id), f, text_value(Some(v)))?,
                }
            }
            Ok(())
        })
    }

    pub fn place_view(&self, id: &str) -> Result<PlaceView> {
        let c = self.conn();
        let base: Option<(String, Option<String>, Option<f64>, Option<f64>, i64, Option<String>)> = c
            .query_row("SELECT name, address, latitude, longitude, exact, description FROM place WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .optional()?;
        let (name, address, latitude, longitude, exact, description) =
            base.ok_or_else(|| Error::Invalid("That place is no longer in the Journey.".into()))?;
        let mut events = list(c, "SELECT id, name, start_date_edtf FROM event WHERE place_id = ?1 ORDER BY start_date IS NULL, start_date, name", id)?;
        for e in &mut events {
            if !e.note.is_empty() {
                e.note = dates::words(&e.note);
            }
        }
        Ok(PlaceView {
            id: id.to_string(),
            name,
            address,
            latitude,
            longitude,
            exact: exact != 0,
            description,
            events,
            memories: dated(list(c, "SELECT id, name, date_edtf FROM memory WHERE place_id = ?1 ORDER BY date IS NULL, date, name", id)?),
        })
    }

    /// Merge two places by hand: every memory and event at `other`
    /// moves to `keep`, and `other` is removed, its ID kept as an alias of `keep`. Its details fill any of
    /// `keep`'s that are empty. One action, so undo puts everything back.
    pub fn merge_places(&mut self, keep: &str, other: &str) -> Result<()> {
        if keep == other {
            return Err(Error::Invalid("Choose two different places.".into()));
        }
        if !exists(self.conn(), "place", keep)? || !exists(self.conn(), "place", other)? {
            return Err(Error::Invalid("That place is no longer in the Journey.".into()));
        }
        self.act(|a| {
            for table in ["memory", "event"] {
                let ids: Vec<String> = {
                    let mut st = a.conn().prepare(&format!("SELECT id FROM {table} WHERE place_id = ?1"))?;
                    st.query_map([other], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
                };
                for i in ids {
                    a.update(table, id_key(&i), "place_id", json!(keep))?;
                }
            }
            for f in ["address", "latitude", "longitude", "description"] {
                let (mine, theirs): (Value, Value) = a.conn().query_row(
                    &format!("SELECT k.{f}, o.{f} FROM place k, place o WHERE k.id = ?1 AND o.id = ?2"),
                    [keep, other],
                    |r| Ok((crate::history::from_sql(r.get_ref(0)?), crate::history::from_sql(r.get_ref(1)?))),
                )?;
                if mine.is_null() && !theirs.is_null() {
                    a.update("place", id_key(keep), f, theirs)?;
                }
            }
            crate::sorting::keep_alias(a, keep, other)?;
            a.delete_item("place", other)
        })
    }

    /// Add an event of a fixed kind. With no name, one is suggested from the
    /// kind and who took part. `when` is typed as known.
    pub fn add_event(&mut self, kind: &str, name: Option<&str>, people: &[String], when: &str, place: Option<&str>) -> Result<String> {
        let kind = kind.trim().to_lowercase();
        if !EVENT_KINDS.contains(&kind.as_str()) {
            return Err(Error::Invalid(format!("\"{kind}\" isn't one of the event kinds.")));
        }
        let date = dates::parse(when)?;
        let mut names = vec![];
        for p in people {
            let t = table_of(self.conn(), p)?;
            if t != "individual" && t != "family" {
                return Err(Error::Invalid("Only people, pets and families take part in an event.".into()));
            }
            names.push(name_of(self.conn(), p)?);
        }
        if let Some(pl) = place {
            if !exists(self.conn(), "place", pl)? {
                return Err(Error::Invalid("That place is no longer in the Journey.".into()));
            }
        }
        if matches!(kind.as_str(), "birth" | "death") {
            if let Some(first) = people.first() {
                if crate::people::life_event(self.conn(), first, &kind)?.is_some() {
                    return Err(Error::Invalid(format!("{} already has a {kind} event; change that one instead.", names[0])));
                }
            }
        }
        let name = name.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| suggest_event_name(&kind, &names));
        self.act(|a| {
            let id = a.create(
                "event",
                row(json!({"kind": kind, "name": name, "start_date": date.as_ref().and_then(|d| d.nearest.clone()),
                           "start_date_edtf": date.as_ref().map(|d| d.edtf.clone()), "place_id": place})),
            )?;
            for (i, p) in people.iter().enumerate() {
                a.insert("event_individual", row(json!({"event_id": id, "individual_id": p, "position": i})))?;
            }
            sync_from_event(a, &id)?;
            Ok(id)
        })
    }

    /// Change an event: `name`, `kind`, `description`, `start` and `end`
    /// (typed as known), `place_id` (empty for none).
    pub fn update_event(&mut self, id: &str, changes: &Row) -> Result<()> {
        if !exists(self.conn(), "event", id)? {
            return Err(Error::Invalid("That event is no longer in the Journey.".into()));
        }
        let mut parsed = std::collections::HashMap::new();
        for (k, v) in changes {
            match k.as_str() {
                "name" if text_value(Some(v)).is_null() => return Err(Error::Invalid("An event needs a name.".into())),
                "kind" if !EVENT_KINDS.contains(&v.as_str().unwrap_or_default()) => {
                    return Err(Error::Invalid("Choose one of the event kinds.".into()));
                }
                "start" | "end" => {
                    parsed.insert(k.clone(), dates::parse(v.as_str().unwrap_or_default())?);
                }
                "place_id" => {
                    if let Some(p) = v.as_str().filter(|p| !p.is_empty()) {
                        if !exists(self.conn(), "place", p)? {
                            return Err(Error::Invalid("That place is no longer in the Journey.".into()));
                        }
                    }
                }
                "name" | "kind" | "description" => {}
                _ => return Err(Error::Invalid(format!("{k} can't be changed on an event."))),
            }
        }
        self.act(|a| {
            for (k, v) in changes {
                match k.as_str() {
                    "start" | "end" => {
                        let d = parsed[k].clone();
                        let (col, edtf) = if k == "start" { ("start_date", "start_date_edtf") } else { ("end_date", "end_date_edtf") };
                        a.update("event", id_key(id), edtf, json!(d.as_ref().map(|d| d.edtf.clone())))?;
                        a.update("event", id_key(id), col, json!(d.and_then(|d| d.nearest)))?;
                    }
                    f => a.update("event", id_key(id), f, text_value(Some(v)))?,
                }
            }
            sync_from_event(a, id)?;
            Ok(())
        })
    }

    /// Set who took part, in order (for a birth, the person born first).
    pub fn set_event_people(&mut self, id: &str, people: &[String]) -> Result<()> {
        if !exists(self.conn(), "event", id)? {
            return Err(Error::Invalid("That event is no longer in the Journey.".into()));
        }
        for p in people {
            let t = table_of(self.conn(), p)?;
            if t != "individual" && t != "family" {
                return Err(Error::Invalid("Only people, pets and families take part in an event.".into()));
            }
        }
        let mut seen = std::collections::HashSet::new();
        if !people.iter().all(|p| seen.insert(p)) {
            return Err(Error::Invalid("Someone is listed twice.".into()));
        }
        self.act(|a| {
            let now: Vec<(String, i64)> = {
                let mut st = a.conn().prepare("SELECT individual_id, position FROM event_individual WHERE event_id = ?1")?;
                st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?
            };
            for (who, _) in &now {
                if !people.contains(who) {
                    a.delete("event_individual", row(json!({"event_id": id, "individual_id": who})))?;
                }
            }
            for (i, p) in people.iter().enumerate() {
                match now.iter().find(|(w, _)| w == p) {
                    Some((_, pos)) if *pos == i as i64 => {}
                    Some(_) => a.update("event_individual", row(json!({"event_id": id, "individual_id": p})), "position", json!(i))?,
                    None => a.insert("event_individual", row(json!({"event_id": id, "individual_id": p, "position": i})))?,
                }
            }
            sync_from_event(a, id)?;
            Ok(())
        })
    }

    pub fn event_view(&self, id: &str) -> Result<EventView> {
        let c = self.conn();
        let base: Option<(String, String, Option<String>, Option<String>, Option<String>, Option<String>)> = c
            .query_row("SELECT kind, name, start_date_edtf, end_date_edtf, place_id, description FROM event WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .optional()?;
        let (kind, name, start, end, place_id, description) =
            base.ok_or_else(|| Error::Invalid("That event is no longer in the Journey.".into()))?;
        let place = match place_id {
            Some(p) => list(c, "SELECT id, name, '' FROM place WHERE id = ?1", &p)?.pop(),
            None => None,
        };
        Ok(EventView {
            id: id.to_string(),
            kind,
            name,
            start: date_value(start),
            end: date_value(end),
            place,
            description,
            people: list(
                c,
                "SELECT ei.individual_id, coalesce(i.name, f.name), CASE WHEN f.id IS NOT NULL THEN 'family' WHEN i.kind = 'Pet' THEN 'pet' ELSE 'person' END
                 FROM event_individual ei LEFT JOIN individual i ON i.id = ei.individual_id LEFT JOIN family f ON f.id = ei.individual_id
                 WHERE ei.event_id = ?1 ORDER BY ei.position",
                id,
            )?,
            memories: dated(list(c, "SELECT id, name, date_edtf FROM memory WHERE event_id = ?1 ORDER BY date IS NULL, date, name", id)?),
        })
    }

    /// What removing a person, pet, family, place, event, story or memory will unlink, in
    /// plain words, for the warning shown before it happens.
    pub fn removal_warning(&self, id: &str) -> Result<Vec<String>> {
        let c = self.conn();
        let table = table_of(c, id)?;
        let n = |sql: &str| -> Result<i64> { Ok(c.query_row(sql, [id], |r| r.get(0))?) };
        let plural = |k: i64, one: &str, many: &str| if k == 1 { format!("1 {one}") } else { format!("{k} {many}") };
        let mut out = vec![];
        match table.as_str() {
            "individual" | "family" => {
                if table == "individual" {
                    let r = n("SELECT count(*) FROM individual_relationship WHERE from_id = ?1 OR to_id = ?1")?;
                    if r > 0 {
                        out.push(format!("{} with others", plural(r, "relationship", "relationships")));
                    }
                    for f in list(c, "SELECT f.id, f.name, '' FROM family_member fm JOIN family f ON f.id = fm.family_id WHERE fm.individual_id = ?1 ORDER BY f.name", id)? {
                        out.push(format!("Membership in {}", f.name));
                    }
                } else {
                    let m = n("SELECT count(*) FROM family_member WHERE family_id = ?1")?;
                    if m > 0 {
                        out.push(format!("The family's {} (they stay in the Journey)", plural(m, "member", "members")));
                    }
                    let r = n("SELECT count(*) FROM family_relationship WHERE from_id = ?1 OR to_id = ?1")?;
                    if r > 0 {
                        out.push(format!("Links to {}", plural(r, "related family", "related families")));
                    }
                }
                for e in lone_life_events(c, id)? {
                    out.push(format!("{} (removed too, since no one else is in it)", e.1));
                }
                let lone: Vec<String> = lone_life_events(c, id)?.into_iter().map(|e| e.0).collect();
                for e in events_of(c, id)? {
                    if !lone.contains(&e.id) {
                        out.push(format!("Taking part in {} (the event stays)", e.name));
                    }
                }
                let m = memories_of(c, id)?.len() as i64;
                if m > 0 {
                    out.push(format!("Their role in {} (the memories stay)", plural(m, "memory", "memories")));
                }
                for s in list(c, "SELECT id, name, '' FROM story WHERE focus_id = ?1 ORDER BY name", id)? {
                    out.push(format!("Being the focus of the story {} (the story stays)", s.name));
                }
            }
            "place" => {
                let m = n("SELECT count(*) FROM memory WHERE place_id = ?1")?;
                if m > 0 {
                    out.push(format!("The place of {} (the memories stay)", plural(m, "memory", "memories")));
                }
                let e = n("SELECT count(*) FROM event WHERE place_id = ?1")?;
                if e > 0 {
                    out.push(format!("The place of {} (the events stay)", plural(e, "event", "events")));
                }
            }
            "event" => {
                let p = n("SELECT count(*) FROM event_individual WHERE event_id = ?1")?;
                if p > 0 {
                    out.push(format!("{} who took part (they stay in the Journey)", plural(p, "person", "people")));
                }
                let m = n("SELECT count(*) FROM memory WHERE event_id = ?1")?;
                if m > 0 {
                    out.push(format!("The event of {} (the memories stay)", plural(m, "memory", "memories")));
                }
                let k: String = c.query_row("SELECT kind FROM event WHERE id = ?1", [id], |r| r.get(0))?;
                if matches!(k.as_str(), "birth" | "death") {
                    out.push(format!("The {} date stays on the person", if k == "birth" { "birth" } else { "death" }));
                }
            }
            "story" => out = crate::stories::story_warning(c, id)?,
            "memory" => out = crate::memories::memory_warning(c, id)?,
            _ => return Err(Error::Invalid("That item can't be removed here.".into())),
        }
        Ok(out)
    }

    /// Remove a person, pet, family, place, event, story or memory, unlinking it from
    /// everything first. One action, so undo brings it all back.
    pub fn remove_item(&mut self, id: &str) -> Result<()> {
        let table = table_of(self.conn(), id)?;
        if !matches!(table.as_str(), "individual" | "family" | "place" | "event" | "story" | "memory") {
            return Err(Error::Invalid("That item can't be removed here.".into()));
        }
        self.act(|a| {
            match table.as_str() {
                "story" => return crate::stories::remove_story(a, id),
                "memory" => return crate::memories::remove_memory(a, id),
                "individual" | "family" => unlink_individual_or_family(a, id, &table)?,
                "place" => {
                    for t in ["memory", "event"] {
                        for i in ids(a, &format!("SELECT id FROM {t} WHERE place_id = ?1"), id)? {
                            a.update(t, id_key(&i), "place_id", Value::Null)?;
                        }
                    }
                }
                _ => remove_event_links(a, id)?,
            }
            a.delete_item(&table, id)
        })
    }
}

/// Entries whose note is an EDTF date, shown in words.
fn dated(mut entries: Vec<Entry>) -> Vec<Entry> {
    for e in &mut entries {
        if !e.note.is_empty() {
            e.note = dates::words(&e.note);
        }
    }
    entries
}

fn ids(a: &Action, sql: &str, arg: &str) -> Result<Vec<String>> {
    let mut st = a.conn().prepare(sql)?;
    let out = st.query_map([arg], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
    Ok(out)
}

/// Birth and death events where this individual is the only one taking part.
fn lone_life_events(c: &rusqlite::Connection, id: &str) -> Result<Vec<(String, String)>> {
    let mut st = c.prepare(
        "SELECT e.id, e.name FROM event e JOIN event_individual ei ON ei.event_id = e.id
         WHERE ei.individual_id = ?1 AND e.kind IN ('birth', 'death')
           AND (SELECT count(*) FROM event_individual WHERE event_id = e.id) = 1 ORDER BY e.name",
    )?;
    let out = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
    Ok(out)
}

fn remove_event_links(a: &mut Action, event: &str) -> Result<()> {
    for m in ids(a, "SELECT id FROM memory WHERE event_id = ?1", event)? {
        a.update("memory", id_key(&m), "event_id", Value::Null)?;
    }
    for p in ids(a, "SELECT individual_id FROM event_individual WHERE event_id = ?1", event)? {
        a.delete("event_individual", row(json!({"event_id": event, "individual_id": p})))?;
    }
    Ok(())
}

pub(crate) fn unlink_individual_or_family(a: &mut Action, id: &str, table: &str) -> Result<()> {
    if table == "individual" {
        for r in ids(a, "SELECT id FROM individual_relationship WHERE from_id = ?1 OR to_id = ?1", id)? {
            a.delete_item("individual_relationship", &r)?;
        }
        for f in ids(a, "SELECT family_id FROM family_member WHERE individual_id = ?1", id)? {
            a.delete("family_member", row(json!({"family_id": f, "individual_id": id})))?;
        }
        // A birth or death event of this individual alone has no meaning without them.
        let lone: Vec<String> = lone_life_events(a.conn(), id)?.into_iter().map(|e| e.0).collect();
        for e in lone {
            remove_event_links(a, &e)?;
            a.delete_item("event", &e)?;
        }
    } else {
        for m in ids(a, "SELECT individual_id FROM family_member WHERE family_id = ?1", id)? {
            a.delete("family_member", row(json!({"family_id": id, "individual_id": m})))?;
        }
        let pairs: Vec<(String, String)> = {
            let mut st = a.conn().prepare("SELECT from_id, to_id FROM family_relationship WHERE from_id = ?1 OR to_id = ?1")?;
            st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?
        };
        for (f, t) in pairs {
            a.delete("family_relationship", row(json!({"from_id": f, "to_id": t})))?;
        }
    }
    for e in ids(a, "SELECT event_id FROM event_individual WHERE individual_id = ?1", id)? {
        a.delete("event_individual", row(json!({"event_id": e, "individual_id": id})))?;
    }
    let roles: Vec<(String, String)> = {
        let mut st = a.conn().prepare("SELECT memory_id, role FROM memory_individual WHERE individual_id = ?1")?;
        st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?
    };
    for (m, role) in roles {
        a.delete("memory_individual", row(json!({"memory_id": m, "individual_id": id, "role": role})))?;
    }
    for s in ids(a, "SELECT id FROM story WHERE focus_id = ?1", id)? {
        a.update("story", id_key(&s), "focus_id", Value::Null)?;
    }
    Ok(())
}
