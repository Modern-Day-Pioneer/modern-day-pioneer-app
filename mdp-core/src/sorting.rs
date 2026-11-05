//! Easier sorting and tidying (release 2, sprint 10).
//!
//! - Dates and titles read from file names: a short list of
//!   naming patterns, kept to the precision found, and offered only as
//!   suggestions with their source.
//! - Waiting items in suggested groups: the same day,
//!   the same place or the same folder, each made into a memory in one step.
//! - Proposed values for a memory: a date from photo
//!   facts or the file name, a place from the picture's location or other
//!   memories that day, and people from other memories that day.
//! - Suggested relationships from a small rule table.
//! - Merging duplicate people and pets, keeping the old ID as an alias
//!   (places already merge, and now keep their alias too).
//!
//! Nothing here changes the catalog unless the person accepts it; a
//! suggestion turned down for a relationship is kept as "rejected" so it is
//! not offered again.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;

use regex::Regex;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::memories::{default_role, exists};
use crate::{dates, id_key, Action, Error, Journey, Result};

// ------------------------------------------------------------ file names

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

fn this_year() -> i32 {
    chrono::Datelike::year(&chrono::Local::now())
}

fn stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((s, ext)) if !s.is_empty() && ext.len() <= 4 => s,
        _ => name,
    }
}

/// A file name that is only a camera's code, such as `IMG_4512` or `DSC01987`.
fn camera_code(stem: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^(?i)[a-z]{1,5}[_ -]?\d+([_ -]\d+)*$").is_match(stem.trim())
}

/// A date read from a file name, in ISO 8601 to the precision found
/// (`2024-06-12`, `1952-06`, `1952`), or a decade as typed (`1950s`).
/// Ambiguous forms such as `06-12-52` give nothing.
pub fn file_name_date(name: &str) -> Option<String> {
    let s = stem(name);
    let ok_year = |y: i32| (1800..=this_year()).contains(&y);
    let full = |y: &str, m: &str, d: &str| -> Option<String> {
        let (y, m, d): (i32, u32, u32) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
        (ok_year(y) && chrono::NaiveDate::from_ymd_opt(y, m, d).is_some()).then(|| format!("{y:04}-{m:02}-{d:02}"))
    };
    // 2024-06-12, 2024_06_12, 2024.06.12, and 20240612 (phone cameras), not inside a longer number.
    static FULL: OnceLock<Regex> = OnceLock::new();
    for c in re(&FULL, r"(?:^|\D)(\d{4})([-_.]?)(\d{2})(?:[-_.]?)(\d{2})(?:\D|$)").captures_iter(s) {
        if let Some(d) = full(&c[1], &c[3], &c[4]) {
            return Some(d);
        }
    }
    // 1952-06 or 1952_06, then something that isn't a digit.
    static MONTH: OnceLock<Regex> = OnceLock::new();
    for c in re(&MONTH, r"(?:^|\D)(\d{4})[-_](\d{2})(?:\D|$)").captures_iter(s) {
        let (y, m): (i32, u32) = (c[1].parse().ok()?, c[2].parse().ok()?);
        if ok_year(y) && (1..=12).contains(&m) {
            return Some(format!("{y:04}-{m:02}"));
        }
    }
    if camera_code(s) {
        return None;
    }
    // 1950s, then a year on its own.
    static DECADE: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = re(&DECADE, r"(?:^|\D)(\d{3}0)'?s(?:[^a-zA-Z]|$)").captures(s) {
        if c[1].parse().is_ok_and(ok_year) {
            return Some(format!("{}s", &c[1]));
        }
    }
    static YEAR: OnceLock<Regex> = OnceLock::new();
    re(&YEAR, r"(?:^|\D)(\d{4})(?:\D|$)").captures_iter(s).find_map(|c| c[1].parse().ok().filter(|y| ok_year(*y)).map(|y: i32| format!("{y:04}")))
}

/// A title suggested from a file name: the name without its extension, the
/// date, the time and camera prefixes, with `_` and `-` as spaces. Nothing
/// when only digits or a camera code are left (`IMG_4512`).
pub fn file_name_title(name: &str) -> Option<String> {
    let s = stem(name);
    if camera_code(s) {
        return None;
    }
    static PREFIX: OnceLock<Regex> = OnceLock::new();
    static DATE: OnceLock<Regex> = OnceLock::new();
    static TIME: OnceLock<Regex> = OnceLock::new();
    let mut t = re(&PREFIX, r"^(?i)(img|pxl|dsc|dscn|dscf|vid|mvi|pano|screenshot|whatsapp (image|video|audio)|signal|scan)(?:[_ -]+|$)")
        .replace(s, "")
        .into_owned();
    t = re(&TIME, r"(?i)\b(at )?\d{1,2}[.:]\d{2}([.:]\d{2})?\b").replace_all(&t, " ").into_owned();
    t = re(&DATE, r"(\d{4}[-_.]?\d{2}[-_.]?\d{2}|\d{4}[-_]\d{2}|\d{3}0'?s|\d{4})([-_ ]?\d{3,9})?").replace_all(&t, " ").into_owned();
    let t = t.replace(['_', '-'], " ");
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != ')' && c != '(').to_string();
    if t.chars().filter(|c| c.is_alphabetic()).count() < 2 || camera_code(&t) {
        return None;
    }
    Some(t)
}

/// Where a suggested date came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DateSource {
    PhotoFacts,
    FileName,
}

impl DateSource {
    pub fn words(self) -> &'static str {
        match self {
            DateSource::PhotoFacts => "from photo facts",
            DateSource::FileName => "from the file name",
        }
    }
}

/// Which date wins: the camera date, unless the file
/// name's date is 20 or more years older, as with a scan of an old photo
/// named by hand.
pub fn best_date(camera: Option<&str>, name: &str) -> Option<(String, DateSource)> {
    let from_name = file_name_date(name);
    let year = |d: &str| d.get(..4).and_then(|y| y.parse::<i32>().ok());
    match (camera, from_name) {
        (Some(c), Some(n)) if year(&n).zip(year(c)).is_some_and(|(n, c)| n + 20 <= c) => Some((n, DateSource::FileName)),
        (Some(c), _) => Some((c.to_string(), DateSource::PhotoFacts)),
        (None, Some(n)) => Some((n, DateSource::FileName)),
        (None, None) => None,
    }
}

/// The ISO day a date names, when it names one (`2024-06-12`).
fn day_of(d: &str) -> Option<&str> {
    (d.len() == 10 && d.as_bytes()[4] == b'-').then_some(d)
}

/// Distance in meters between two points on the earth.
fn meters(a: (f64, f64), b: (f64, f64)) -> f64 {
    let r = 6_371_000.0_f64;
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let dla = la2 - la1;
    let dlo = (b.1 - a.1).to_radians();
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    2.0 * r * h.sqrt().asin()
}

/// How close two pictures must be to count as the same place.
pub const NEAR_METERS: f64 = 500.0;

// ------------------------------------------------------------ waiting groups

/// A few waiting items that seem to belong together.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Group {
    /// Stable for the same items: "day:2024-06-12", "place:…", "folder:…".
    pub key: String,
    /// "Taken the same day", "Taken in the same place", "From the same folder".
    pub reason: String,
    /// What the group is: "June 12, 2024", "The Rowan house", "Grandpa's box".
    pub label: String,
    pub items: Vec<String>,
    pub title: String,
    /// A date to give the memory, typed as known, and where it came from.
    pub date: Option<String>,
    pub date_words: Option<String>,
    pub date_source: Option<String>,
    pub place_id: Option<String>,
    pub place_name: Option<String>,
}

struct Waiting {
    id: String,
    name: String,
    date: Option<(String, DateSource)>,
    folder: String,
    at: Option<(f64, f64)>,
}

/// A date that every item shares, to the closest precision they all agree on.
fn shared_date(items: &[&Waiting]) -> Option<(String, DateSource)> {
    let dated: Vec<&(String, DateSource)> = items.iter().filter_map(|w| w.date.as_ref()).collect();
    let first = dated.first()?;
    let source = if dated.iter().all(|d| d.1 == DateSource::PhotoFacts) { DateSource::PhotoFacts } else { DateSource::FileName };
    for len in [10, 7, 4] {
        let Some(p) = first.0.get(..len) else { continue };
        if dated.iter().all(|d| d.0.len() >= len && d.0.get(..len) == Some(p)) {
            return Some((p.to_string(), source));
        }
    }
    None
}

pub(crate) fn last_folder(path: &str) -> String {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

impl Journey {
    fn waiting_items(&self) -> Result<Vec<Waiting>> {
        let mut st = self.conn().prepare(&format!(
            "SELECT c.id, c.name, c.date, coalesce(c.source_note, ''), coalesce(p.latitude, v.latitude), coalesce(p.longitude, v.longitude)
             FROM content c LEFT JOIN picture p ON p.id = c.id LEFT JOIN video v ON v.id = c.id
             WHERE c.state = 'waiting' AND {} ORDER BY c.date_added, c.name",
            crate::voice::NOT_ATTACHED.replace("content.id", "c.id")
        ))?;
        let rows = st.query_map([], |r| {
            let (id, name, date, folder): (String, String, Option<String>, String) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
            let at = match (r.get::<_, Option<f64>>(4)?, r.get::<_, Option<f64>>(5)?) {
                (Some(a), Some(b)) => Some((a, b)),
                _ => None,
            };
            let date = best_date(date.as_deref(), &name);
            Ok(Waiting { id, name, date, folder, at })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Places with map coordinates, for matching a picture's location.
    fn places_on_map(&self) -> Result<Vec<(String, String, (f64, f64))>> {
        let mut st = self.conn().prepare("SELECT id, name, latitude, longitude FROM place WHERE latitude IS NOT NULL AND longitude IS NOT NULL ORDER BY name")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, (r.get(2)?, r.get(3)?))))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    fn place_near(&self, at: (f64, f64)) -> Result<Option<(String, String)>> {
        Ok(self
            .places_on_map()?
            .into_iter()
            .map(|(id, name, p)| (meters(at, p), id, name))
            .filter(|(d, _, _)| *d <= NEAR_METERS)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id, name)| (id, name)))
    }

    /// Waiting items in suggested groups of two or more: first the same day,
    /// then the same place, then the same folder. Each item is in one group
    /// at most; the biggest groups come first.
    pub fn waiting_groups(&self) -> Result<Vec<Group>> {
        let items = self.waiting_items()?;
        let mut used: BTreeSet<usize> = BTreeSet::new();
        let mut out = Vec::new();
        let mut make = |reason: &str, key: String, label: String, members: Vec<usize>, title: Option<String>, place: Option<(String, String)>, used: &mut BTreeSet<usize>| {
            let refs: Vec<&Waiting> = members.iter().map(|i| &items[*i]).collect();
            let date = shared_date(&refs);
            // A title the file names agree on, or else what the group is.
            let names: BTreeSet<String> = refs.iter().filter_map(|w| file_name_title(&w.name)).collect();
            let title = title.or_else(|| (names.len() == 1).then(|| names.into_iter().next()).flatten()).unwrap_or_else(|| label.clone());
            used.extend(members.iter().copied());
            out.push(Group {
                key,
                reason: reason.into(),
                label,
                items: refs.iter().map(|w| w.id.clone()).collect(),
                title,
                date_words: date.as_ref().map(|d| dates::parse(&d.0).ok().flatten().map(|k| k.words).unwrap_or_else(|| d.0.clone())),
                date_source: date.as_ref().map(|d| d.1.words().to_string()),
                date: date.map(|d| d.0),
                place_name: place.as_ref().map(|p| p.1.clone()),
                place_id: place.map(|p| p.0),
            });
        };
        let mut by_day: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, w) in items.iter().enumerate() {
            if let Some(d) = w.date.as_ref().and_then(|d| day_of(&d.0)) {
                by_day.entry(d.to_string()).or_default().push(i);
            }
        }
        for (day, members) in by_day.into_iter().filter(|(_, m)| m.len() >= 2) {
            let words = dates::parse(&day).ok().flatten().map(|d| d.words).unwrap_or_else(|| day.clone());
            let place = members.iter().find_map(|i| items[*i].at).map(|at| self.place_near(at)).transpose()?.flatten();
            make("Taken the same day", format!("day:{day}"), words, members, None, place, &mut used);
        }
        // The same place: pictures and videos within NEAR_METERS of the first of a cluster.
        let mut clusters: Vec<((f64, f64), Vec<usize>)> = Vec::new();
        for (i, w) in items.iter().enumerate() {
            let Some(at) = w.at.filter(|_| !used.contains(&i)) else { continue };
            match clusters.iter_mut().find(|(c, _)| meters(*c, at) <= NEAR_METERS) {
                Some((_, m)) => m.push(i),
                None => clusters.push((at, vec![i])),
            }
        }
        for (at, members) in clusters.into_iter().filter(|(_, m)| m.len() >= 2) {
            let place = self.place_near(at)?;
            let label = place.as_ref().map(|p| p.1.clone()).unwrap_or_else(|| format!("Near {:.3}, {:.3}", at.0, at.1));
            let title = place.as_ref().map(|p| p.1.clone());
            make("Taken in the same place", format!("place:{:.3},{:.3}", at.0, at.1), label, members, title, place, &mut used);
        }
        let mut by_folder: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, w) in items.iter().enumerate() {
            if !used.contains(&i) && !w.folder.is_empty() {
                by_folder.entry(w.folder.clone()).or_default().push(i);
            }
        }
        for (folder, members) in by_folder.into_iter().filter(|(_, m)| m.len() >= 2) {
            let name = last_folder(&folder);
            let title = file_name_title(&name);
            make("From the same folder", format!("folder:{folder}"), name, members, title, None, &mut used);
        }
        out.sort_by(|a, b| b.items.len().cmp(&a.items.len()).then_with(|| a.key.cmp(&b.key)));
        Ok(out)
    }

    /// Make one memory from a group in one step: the items, a title, and the
    /// suggested date and place when given. One action, so undo takes it back.
    pub fn memory_from_group(&mut self, name: &str, content: &[String], date: Option<&str>, place: Option<&str>) -> Result<String> {
        if content.is_empty() {
            return Err(Error::Invalid("Choose at least one item first.".into()));
        }
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("A memory needs a title.".into()));
        }
        let date = match date.map(str::trim).filter(|d| !d.is_empty()) {
            Some(d) => dates::parse(d)?,
            None => None,
        };
        if let Some(p) = place.filter(|p| !p.is_empty()) {
            if !exists(self.conn(), "place", p)? {
                return Err(Error::Invalid("That place is no longer in the Journey.".into()));
            }
        }
        self.act(|a| {
            let id = crate::memories::create_memory(a, name)?;
            for c in content {
                crate::memories::put_in(a, &id, c)?;
            }
            crate::memories::refresh_status(a, &id)?;
            if let Some(d) = &date {
                a.update("memory", id_key(&id), "date_edtf", json!(d.edtf))?;
                a.update("memory", id_key(&id), "date", json!(d.nearest))?;
            }
            if let Some(p) = place.filter(|p| !p.is_empty()) {
                a.update("memory", id_key(&id), "place_id", json!(p))?;
            }
            Ok(id)
        })
    }

    /// Each item its own memory, titled and dated from its file name or photo
    /// facts where they say something ("each its own memory"). One action.
    pub fn memory_each(&mut self, content: &[String]) -> Result<Vec<String>> {
        if content.is_empty() {
            return Err(Error::Invalid("Choose at least one item first.".into()));
        }
        let mut plan = Vec::new();
        for c in content {
            let (name, date): (String, Option<String>) = self
                .conn()
                .query_row("SELECT name, date FROM content WHERE id = ?1", [c], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?
                .ok_or_else(|| Error::Invalid("That item is no longer in the Journey.".into()))?;
            let title = file_name_title(&name).unwrap_or_else(|| stem(&name).to_string());
            let when = best_date(date.as_deref(), &name).and_then(|d| dates::parse(&d.0).ok().flatten());
            plan.push((c.clone(), title, when));
        }
        self.act(|a| {
            let mut ids = Vec::new();
            for (c, title, when) in &plan {
                let id = crate::memories::create_memory(a, title)?;
                crate::memories::put_in(a, &id, c)?;
                crate::memories::refresh_status(a, &id)?;
                if let Some(d) = when {
                    a.update("memory", id_key(&id), "date_edtf", json!(d.edtf))?;
                    a.update("memory", id_key(&id), "date", json!(d.nearest))?;
                }
                ids.push(id);
            }
            Ok(ids)
        })
    }
}

// ------------------------------------------------------------ proposed values

/// A value the app proposes for a memory. Accepting it
/// is an ordinary change: `field` "name", "date" or "place_id" goes to
/// `update_memory` with `value`; "person" links `value` with `role`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Proposal {
    pub field: String,
    pub value: String,
    /// What it is, in words: "June 12, 2024", "The Rowan house", "Ellen Rowan".
    pub words: String,
    /// Where it came from: "from photo facts", "from other memories that day".
    pub source: String,
    pub role: Option<String>,
}

impl Journey {
    /// What the app can propose for a memory that lacks a date, a place,
    /// people or a real title. Nothing for a placeholder or a memory marked
    /// finished; the suggestions switch is honored by the screens.
    pub fn memory_proposals(&self, id: &str) -> Result<Vec<Proposal>> {
        let c = self.conn();
        type M = (String, Option<String>, Option<String>, String, Option<String>);
        let (name, date, place, status, finished): M = c
            .query_row("SELECT name, date, place_id, status, finished FROM memory WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .optional()?
            .ok_or_else(|| Error::Invalid("That memory is no longer in the Journey.".into()))?;
        if status == "needs content" || finished.is_some() {
            return Ok(vec![]);
        }
        let mut st = c.prepare(
            "SELECT ct.name, ct.date, ct.kind, coalesce(p.latitude, v.latitude), coalesce(p.longitude, v.longitude)
             FROM memory_content mc JOIN content ct ON ct.id = mc.content_id
             LEFT JOIN picture p ON p.id = ct.id LEFT JOIN video v ON v.id = ct.id
             WHERE mc.memory_id = ?1 ORDER BY mc.position",
        )?;
        type C = (String, Option<String>, String, Option<f64>, Option<f64>);
        let content: Vec<C> = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<std::result::Result<_, _>>()?;
        let mut out = Vec::new();
        // A title from the file name, when the memory is still called by it.
        for (file, ..) in &content {
            if name == *file || name == stem(file) || camera_code(&name) {
                if let Some(t) = file_name_title(file).filter(|t| *t != name) {
                    out.push(Proposal { field: "name".into(), value: t.clone(), words: t, source: DateSource::FileName.words().into(), role: None });
                    break;
                }
            }
        }
        let mut day = date.clone().filter(|d| day_of(d).is_some());
        if date.is_none() {
            if let Some((d, src)) = content.iter().find_map(|(n, d, ..)| best_date(d.as_deref(), n)) {
                let words = dates::parse(&d).ok().flatten().map(|k| k.words).unwrap_or_else(|| d.clone());
                day = day_of(&d).map(str::to_string);
                out.push(Proposal { field: "date".into(), value: d, words, source: src.words().into(), role: None });
            }
        }
        // Other memories of the same day.
        let others: Vec<String> = match &day {
            Some(d) => {
                let mut st = c.prepare("SELECT id FROM memory WHERE date = ?1 AND id <> ?2 ORDER BY name")?;
                st.query_map([d.as_str(), id], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
            }
            None => vec![],
        };
        if place.is_none() {
            let near = content.iter().find_map(|(.., la, lo)| la.zip(*lo)).map(|at| self.place_near(at)).transpose()?.flatten();
            if let Some((pid, pname)) = near {
                out.push(Proposal { field: "place_id".into(), value: pid, words: pname, source: "from the picture's location".into(), role: None });
            } else {
                let mut seen = BTreeSet::new();
                for o in &others {
                    let p: Option<(String, String)> = c
                        .query_row("SELECT p.id, p.name FROM memory m JOIN place p ON p.id = m.place_id WHERE m.id = ?1", [o], |r| Ok((r.get(0)?, r.get(1)?)))
                        .optional()?;
                    if let Some((pid, pname)) = p.filter(|p| seen.insert(p.0.clone())) {
                        out.push(Proposal { field: "place_id".into(), value: pid, words: pname, source: "from other memories that day".into(), role: None });
                    }
                }
            }
        }
        let linked: BTreeSet<String> = crate::memories::strings(c, "SELECT individual_id FROM memory_individual WHERE memory_id = ?1", id)?.into_iter().collect();
        let role = content.first().map(|(_, _, k, ..)| default_role(k).to_string());
        let mut seen = BTreeSet::new();
        for o in &others {
            let mut st = c.prepare(
                "SELECT mi.individual_id, coalesce(i.name, f.name) FROM memory_individual mi
                 LEFT JOIN individual i ON i.id = mi.individual_id LEFT JOIN family f ON f.id = mi.individual_id
                 WHERE mi.memory_id = ?1 ORDER BY 2",
            )?;
            let people: Vec<(String, String)> = st.query_map([o], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
            for (who, wname) in people {
                if !linked.contains(&who) && seen.insert(who.clone()) && seen.len() <= 6 {
                    out.push(Proposal { field: "person".into(), value: who, words: wname, source: "from other memories that day".into(), role: role.clone() });
                }
            }
        }
        Ok(out)
    }
}

// ------------------------------------------------------------ relationships

/// A relationship the data supports, in the stored form: `from` is
/// the `kind` of `to`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuggestedRelationship {
    pub from_id: String,
    pub kind: String,
    pub to_id: String,
    pub qualifier: Option<String>,
    /// "Ada Hale is probably Ellen Rowan's grandmother".
    pub words: String,
    /// "Ada Hale is Walter Rowan's mother, and Walter is Ellen's father."
    pub because: String,
}

struct Tree {
    names: HashMap<String, String>,
    genders: HashMap<String, Option<String>>,
    /// child -> parents
    parents: HashMap<String, BTreeSet<String>>,
    partners: HashMap<String, BTreeSet<String>>,
    siblings: HashMap<String, BTreeSet<String>>,
    /// Every pair with any relationship at all, entered, accepted or turned down.
    linked: BTreeSet<(String, String)>,
}

impl Tree {
    fn read(c: &rusqlite::Connection) -> Result<Tree> {
        let mut t = Tree { names: HashMap::new(), genders: HashMap::new(), parents: HashMap::new(), partners: HashMap::new(), siblings: HashMap::new(), linked: BTreeSet::new() };
        let mut st = c.prepare("SELECT i.id, i.name, p.gender FROM individual i LEFT JOIN person p ON p.id = i.id")?;
        for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))? {
            let (id, name, g) = r?;
            t.names.insert(id.clone(), name);
            t.genders.insert(id, g);
        }
        let mut st = c.prepare("SELECT from_id, to_id, kind, status FROM individual_relationship")?;
        for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))? {
            let (from, to, kind, status) = r?;
            t.linked.insert(pair(&from, &to));
            if status != "entered" && status != "accepted" {
                continue;
            }
            match kind.as_str() {
                "parent" => {
                    t.parents.entry(to).or_default().insert(from);
                }
                "partner" => {
                    t.partners.entry(from.clone()).or_default().insert(to.clone());
                    t.partners.entry(to).or_default().insert(from);
                }
                "sibling" => {
                    t.siblings.entry(from.clone()).or_default().insert(to.clone());
                    t.siblings.entry(to).or_default().insert(from);
                }
                _ => {}
            }
        }
        Ok(t)
    }

    fn parents_of(&self, id: &str) -> BTreeSet<String> {
        self.parents.get(id).cloned().unwrap_or_default()
    }

    fn children_of(&self, id: &str) -> BTreeSet<String> {
        self.parents.iter().filter(|(_, ps)| ps.contains(id)).map(|(c, _)| c.clone()).collect()
    }

    fn partners_of(&self, id: &str) -> BTreeSet<String> {
        self.partners.get(id).cloned().unwrap_or_default()
    }

    /// Entered siblings and everyone who shares a parent.
    fn siblings_of(&self, id: &str) -> BTreeSet<String> {
        let mut s = self.siblings.get(id).cloned().unwrap_or_default();
        for p in self.parents_of(id) {
            s.extend(self.children_of(&p));
        }
        s.remove(id);
        s
    }

    fn name(&self, id: &str) -> &str {
        self.names.get(id).map(String::as_str).unwrap_or("someone")
    }

    fn as_kind(&self, id: &str, kind: &str) -> String {
        crate::people::gendered(kind, self.genders.get(id).cloned().flatten().as_deref())
    }
}

fn pair(a: &str, b: &str) -> (String, String) {
    if a < b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) }
}

impl Journey {
    /// Relationships of this person the data supports and nobody has entered
    /// or turned down yet, from a small rule table: siblings (a shared
    /// parent), grandparents, uncles and aunts, cousins, step-parents (a
    /// parent's partner) and in-laws (a partner's parent or sibling, or a
    /// sibling's partner).
    pub fn suggested_relationships(&self, id: &str) -> Result<Vec<SuggestedRelationship>> {
        if !exists(self.conn(), "individual", id)? {
            return Err(Error::Invalid("That person or pet is no longer in the Journey.".into()));
        }
        let t = Tree::read(self.conn())?;
        let mut out: Vec<SuggestedRelationship> = Vec::new();
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        let mut add = |from: &str, kind: &str, to: &str, qualifier: Option<&str>, because: String| {
            if from == to || t.linked.contains(&pair(from, to)) || !seen.insert(pair(from, to)) {
                return;
            }
            let (subject, object) = (t.name(from), t.name(to));
            let words = format!("{subject} is probably {object}'s {}", t.as_kind(from, kind));
            out.push(SuggestedRelationship { from_id: from.into(), kind: kind.into(), to_id: to.into(), qualifier: qualifier.map(str::to_string), words, because });
        };
        let is = |who: &str, kind: &str, of: &str| format!("{} is {}'s {}", t.name(who), t.name(of), t.as_kind(who, kind));
        let parents = t.parents_of(id);
        // Siblings: a shared parent.
        for p in &parents {
            for s in t.children_of(p) {
                add(&s, "sibling", id, None, format!("Both are children of {}.", t.name(p)));
            }
        }
        // Grandparents, uncles and aunts, cousins and step-parents, through each parent.
        for p in &parents {
            for g in t.parents_of(p) {
                add(&g, "grandparent", id, None, format!("{}, and {}.", is(&g, "parent", p), is(p, "parent", id)));
            }
            for u in t.siblings_of(p) {
                add(&u, "uncle or aunt", id, None, format!("{}, and {}.", is(&u, "sibling", p), is(p, "parent", id)));
                for cousin in t.children_of(&u) {
                    add(&cousin, "cousin", id, None, format!("{}, and {}.", is(&cousin, "child", &u), is(&u, "sibling", p)));
                }
            }
            for step in t.partners_of(p) {
                if !parents.contains(&step) {
                    add(&step, "parent", id, Some("step"), format!("{}, and {}.", is(&step, "partner", p), is(p, "parent", id)));
                }
            }
        }
        // The other way round: this person as grandparent, uncle or aunt, and step-parent.
        for c in t.children_of(id) {
            for g in t.children_of(&c) {
                add(id, "grandparent", &g, None, format!("{}, and {}.", is(id, "parent", &c), is(&c, "parent", &g)));
            }
        }
        for s in t.siblings_of(id) {
            for n in t.children_of(&s) {
                add(id, "uncle or aunt", &n, None, format!("{}, and {}.", is(id, "sibling", &s), is(&s, "parent", &n)));
            }
        }
        for partner in t.partners_of(id) {
            for c in t.children_of(&partner) {
                if !t.parents_of(&c).contains(id) {
                    add(id, "parent", &c, Some("step"), format!("{}, and {}.", is(id, "partner", &partner), is(&partner, "parent", &c)));
                }
            }
            // In-laws: a partner's parents and siblings.
            for pp in t.parents_of(&partner) {
                add(&pp, "in-law", id, None, format!("{}, and {}.", is(&pp, "parent", &partner), is(&partner, "partner", id)));
            }
            for ps in t.siblings_of(&partner) {
                add(&ps, "in-law", id, None, format!("{}, and {}.", is(&ps, "sibling", &partner), is(&partner, "partner", id)));
            }
        }
        // In-laws: a sibling's partner, and a child's partner.
        for s in t.siblings_of(id) {
            for sp in t.partners_of(&s) {
                add(&sp, "in-law", id, None, format!("{}, and {}.", is(&sp, "partner", &s), is(&s, "sibling", id)));
            }
        }
        for c in t.children_of(id) {
            for cp in t.partners_of(&c) {
                add(&cp, "in-law", id, None, format!("{}, and {}.", is(&cp, "partner", &c), is(&c, "child", id)));
            }
        }
        Ok(out)
    }

    /// Accept a suggested relationship (stored as "accepted") or turn it down
    /// (stored as "rejected", so it is not offered again and stays out of the export).
    pub fn answer_suggestion(&mut self, s: &SuggestedRelationship, accept: bool) -> Result<String> {
        for who in [&s.from_id, &s.to_id] {
            if !exists(self.conn(), "individual", who)? {
                return Err(Error::Invalid("That person or pet is no longer in the Journey.".into()));
            }
        }
        if !crate::people::RELATIONSHIP_KINDS.contains(&s.kind.as_str()) || crate::people::reverse(&s.kind).is_none() {
            return Err(Error::Invalid("That isn't a relationship the app suggests.".into()));
        }
        let status = if accept { "accepted" } else { "rejected" };
        let note = format!("Suggested by the app: {}", s.because);
        self.act(|a| {
            a.create(
                "individual_relationship",
                crate::import::row(json!({"from_id": s.from_id, "to_id": s.to_id, "kind": s.kind, "qualifier": s.qualifier,
                    "certainty": "certain", "source_note": note, "status": status})),
            )
        })
    }
}

// ------------------------------------------------------------ merging

/// Keep `old` as another name for `keep`, and move any aliases `old` had.
pub(crate) fn keep_alias(a: &mut Action, keep: &str, old: &str) -> Result<()> {
    for o in crate::memories::strings(a.conn(), "SELECT old_id FROM alias WHERE item_id = ?1", old)? {
        a.update("alias", crate::import::row(json!({"old_id": o})), "item_id", json!(keep))?;
    }
    a.insert("alias", crate::import::row(json!({"old_id": old, "item_id": keep})))
}

fn rows_of(a: &Action, sql: &str, id: &str) -> Result<Vec<Vec<Value>>> {
    let mut st = a.conn().prepare(sql)?;
    let n = st.column_count();
    let out = st
        .query_map([id], |r| (0..n).map(|i| Ok(crate::history::from_sql(r.get_ref(i)?))).collect::<rusqlite::Result<Vec<Value>>>())?
        .collect::<std::result::Result<_, _>>()?;
    Ok(out)
}

impl Journey {
    /// The item an ID names now: itself, or the item it was merged into.
    pub fn resolve_id(&self, id: &str) -> Result<String> {
        let to: Option<String> = self.conn().query_row("SELECT item_id FROM alias WHERE old_id = ?1", [id], |r| r.get(0)).optional()?;
        Ok(to.unwrap_or_else(|| id.to_string()))
    }

    /// Merge two entries for the same person or pet: every memory, event,
    /// family and relationship of `other` moves to `keep`, `other`'s details
    /// fill any of `keep`'s that are empty, its name is kept among the other
    /// names (one per line) when it differs, and its ID becomes an alias of `keep`. One
    /// action, so undo puts everything back.
    pub fn merge_individuals(&mut self, keep: &str, other: &str) -> Result<()> {
        if keep == other {
            return Err(Error::Invalid("Choose two different entries.".into()));
        }
        let kind = |id: &str| -> Result<Option<String>> { Ok(self.conn().query_row("SELECT kind FROM individual WHERE id = ?1", [id], |r| r.get(0)).optional()?) };
        let (k1, k2) = (kind(keep)?, kind(other)?);
        let (Some(k1), Some(k2)) = (k1, k2) else {
            return Err(Error::Invalid("That person or pet is no longer in the Journey.".into()));
        };
        if k1 != k2 {
            return Err(Error::Invalid("A person and a pet can't be merged.".into()));
        }
        let ext = if k1 == "Pet" { "pet" } else { "person" };
        self.act(|a| {
            // Memories, with their roles; a role keep already has is not doubled.
            for r in rows_of(a, "SELECT memory_id, role, role_note, certainty FROM memory_individual WHERE individual_id = ?1", other)? {
                let (m, role) = (r[0].as_str().unwrap_or_default(), r[1].as_str().unwrap_or_default());
                let has: bool = a.conn().query_row("SELECT EXISTS(SELECT 1 FROM memory_individual WHERE memory_id = ?1 AND individual_id = ?2 AND role = ?3)", [m, keep, role], |r| r.get(0))?;
                a.delete("memory_individual", crate::import::row(json!({"memory_id": m, "individual_id": other, "role": role})))?;
                if !has {
                    a.insert("memory_individual", crate::import::row(json!({"memory_id": m, "individual_id": keep, "role": role, "role_note": r[2], "certainty": r[3]})))?;
                }
            }
            for (table, owner) in [("event_individual", "event_id"), ("family_member", "family_id")] {
                let extra = if table == "event_individual" { ", position" } else { "" };
                for r in rows_of(a, &format!("SELECT {owner}{extra} FROM {table} WHERE individual_id = ?1"), other)? {
                    let o = r[0].as_str().unwrap_or_default();
                    let has: bool = a.conn().query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {owner} = ?1 AND individual_id = ?2)"), [o, keep], |r| r.get(0))?;
                    a.delete(table, crate::import::row(json!({owner: o, "individual_id": other})))?;
                    if !has {
                        let mut row = crate::import::row(json!({owner: o, "individual_id": keep}));
                        if let Some(p) = r.get(1) {
                            row.insert("position".into(), p.clone());
                        }
                        a.insert(table, row)?;
                    }
                }
            }
            // Relationships: moved over, unless they would point at keep itself or repeat one keep has.
            for r in rows_of(a, "SELECT id, from_id, to_id, kind FROM individual_relationship WHERE from_id = ?1 OR to_id = ?1", other)? {
                let (rid, from, to, k) = (r[0].as_str().unwrap_or_default(), r[1].as_str().unwrap_or_default(), r[2].as_str().unwrap_or_default(), r[3].as_str().unwrap_or_default());
                let (nf, nt) = (if from == other { keep } else { from }, if to == other { keep } else { to });
                let dup: bool = a.conn().query_row(
                    "SELECT EXISTS(SELECT 1 FROM individual_relationship WHERE id <> ?4 AND kind = ?3 AND ((from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1)))",
                    [nf, nt, k, rid],
                    |r| r.get(0),
                )?;
                if nf == nt || dup {
                    a.delete_item("individual_relationship", rid)?;
                } else {
                    if from == other {
                        a.update("individual_relationship", id_key(rid), "from_id", json!(keep))?;
                    }
                    if to == other {
                        a.update("individual_relationship", id_key(rid), "to_id", json!(keep))?;
                    }
                }
            }
            for s in crate::memories::strings(a.conn(), "SELECT id FROM story WHERE focus_id = ?1", other)? {
                a.update("story", id_key(&s), "focus_id", json!(keep))?;
            }
            let featured: bool = a.conn().query_row("SELECT EXISTS(SELECT 1 FROM featured WHERE item_id = ?1)", [other], |r| r.get(0))?;
            if featured {
                a.delete("featured", crate::import::row(json!({"item_id": other})))?;
            }
            // Details: other's fill keep's empty ones; other's name joins the other names.
            let fill = |a: &mut Action, table: &str, cols: &[&str]| -> Result<()> {
                for f in cols {
                    let (mine, theirs): (Value, Value) = a.conn().query_row(
                        &format!("SELECT k.{f}, o.{f} FROM {table} k, {table} o WHERE k.id = ?1 AND o.id = ?2"),
                        [keep, other],
                        |r| Ok((crate::history::from_sql(r.get_ref(0)?), crate::history::from_sql(r.get_ref(1)?))),
                    )?;
                    if mine.is_null() && !theirs.is_null() {
                        a.update(table, id_key(keep), f, theirs)?;
                    }
                }
                Ok(())
            };
            fill(a, "individual", &["birth_date", "birth_date_edtf", "death_date", "death_date_edtf", "description", "image_id"])?;
            if ext == "person" {
                fill(a, "person", &["given_name", "family_name", "gender", "living_override"])?;
                let (kname, oname, knames, onames): (String, String, Option<String>, Option<String>) = a.conn().query_row(
                    "SELECT k.name, o.name, kp.other_names, op.other_names FROM individual k, individual o
                     LEFT JOIN person kp ON kp.id = k.id LEFT JOIN person op ON op.id = o.id WHERE k.id = ?1 AND o.id = ?2",
                    [keep, other],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?;
                let mut names: Vec<String> = knames.iter().chain(onames.iter()).flat_map(|s| s.split(['\n', ';'])).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                if !oname.eq_ignore_ascii_case(&kname) {
                    names.push(oname);
                }
                let mut seen = BTreeSet::new();
                names.retain(|n| !n.eq_ignore_ascii_case(&kname) && seen.insert(n.to_lowercase()));
                if !names.is_empty() {
                    a.update("person", id_key(keep), "other_names", json!(names.join("\n")))?;
                }
            } else {
                fill(a, "pet", &["species", "breed", "joined_date_edtf"])?;
            }
            crate::places::unlink_individual_or_family(a, other, "individual")?;
            // The alias first, so undo restores the entry before pointing old links back at it.
            keep_alias(a, keep, other)?;
            a.delete_item("individual", other)
        })
    }
}
