//! Bulk folder import and import plans (release 2, sprint 11).
//!
//! - [`scan`] looks through a folder and its subfolders without copying
//!   anything and makes a [`Plan`]: one row per file with a title, a date,
//!   people, a place, a memory and a story guessed from the folder and file
//!   names, each guess showing where it came from. Nothing is added yet.
//! - The plan can be saved as a CSV "import plan" ([`to_csv`]) and opened
//!   again ([`from_csv`]); a script or a spreadsheet can write one too
//!   ([`sample_plan`] gives a blank one with a read-me).
//! - The import itself runs like any other ([`crate::import::prepare`]), and
//!   [`Journey::finish_plan`] then records the files and makes the memories
//!   in one action, so one undo takes the whole import back.
//!
//! Rules (agreed 2026-10-24): one file per memory unless rows share a
//! memory name; dates from the file name first, then its folder, then
//! parent folders, ahead of the photo's own date (scans carry the scan
//! date); people and places already in the Journey are suggested, never
//! added silently. Originals are only read and every file enters the
//! Journey through a memory.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::import::{classify, Batch};
use crate::jobs::JobContext;
use crate::sorting::{file_name_date, file_name_title};
use crate::{dates, id_key, Error, Journey, Result};

/// One file in a plan. `people` holds names separated by `;`, each
/// optionally with its role in brackets: `June Rowan (in the picture)`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct PlanRow {
    /// Relative to the plan's base folder with `/`, or a full path.
    pub file: String,
    pub include: bool,
    pub title: String,
    /// Typed as known: `1970`, `June 1970`, `1980 to 1985`, `1970s`.
    pub date: String,
    /// Where the date came from, in words, for the preview.
    pub date_source: String,
    pub people: String,
    pub place: String,
    /// Blank: the file is its own memory. Rows with the same text share one.
    pub memory: String,
    pub story: String,
    pub description: String,
    /// picture, voice recording, video or text.
    pub kind: String,
    /// Anything the person should know about this row.
    pub note: String,
    /// What the names in `people` still need (worked out again by
    /// [`check_names`] whenever a name is matched).
    pub people_note: String,
    /// Google Takeout's location for this file.
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// `place` was filled from the location (Places found), not typed.
    pub place_from_location: bool,
    /// What the location still needs (worked out again by [`check_places`]).
    pub place_note: String,
    /// A suggested group (same folder, date and people), never applied on its own.
    pub group: String,
    /// Google Takeout's sidecar for this file, found while looking through
    /// (it can be in another part of the Takeout). Not in import plans.
    pub sidecar: String,
}

/// A folder's or CSV's rows, with the folder relative paths start from.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Plan {
    pub base: String,
    pub rows: Vec<PlanRow>,
    /// What to do with names found in the rows (Names found).
    pub names: Vec<NameChoice>,
    /// What to do with the places Google's locations are at (Places found).
    pub places: Vec<PlaceChoice>,
}

/// What to do with one group of nearby locations, for every file in it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct PlaceChoice {
    /// The group's [`FoundPlace::key`].
    pub key: String,
    /// `place` (a place already in the Journey, by `name`), `new` (add a
    /// place called `name` at the group's location), or `leave`.
    pub action: String,
    pub name: String,
}

/// Files whose Google locations are within a kilometer of each other.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FoundPlace {
    /// The first file in the group, which names it.
    pub key: String,
    pub files: usize,
    pub latitude: f64,
    pub longitude: f64,
    /// `known` (a place in this Journey within a kilometer), `town` (the
    /// nearest town is suggested) or `none` (nothing near).
    pub status: String,
    /// The place in this Journey, or the town suggested.
    pub suggestion: String,
    /// How far the suggested town is, in km.
    pub km: f64,
    pub chosen: bool,
}

/// How far apart two locations can be and still be one place (km).
const SAME_PLACE_KM: f64 = 1.0;
/// How far away a town can be and still be suggested (km).
const TOWN_KM: f64 = 30.0;

/// Group the files with Google locations by place, name each group (a place
/// in this Journey nearby, or with `towns` the nearest town from the list
/// built in), and fill the rows' place from the choices made.
pub fn check_places(plan: &mut Plan, known: &Known, towns: bool) -> Vec<FoundPlace> {
    let mut found: Vec<FoundPlace> = Vec::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for (i, r) in plan.rows.iter().enumerate() {
        let (Some(la), Some(lo)) = (r.latitude, r.longitude) else { continue };
        if !(r.place.trim().is_empty() || r.place_from_location) {
            continue;
        }
        match found.iter().position(|f| crate::takeout::km((f.latitude, f.longitude), (la, lo)) <= SAME_PLACE_KM) {
            Some(g) => {
                found[g].files += 1;
                members[g].push(i);
            }
            None => {
                let (status, suggestion, km) = match known.near(la, lo) {
                    Some(n) => ("known", n, 0.0),
                    None => match towns.then(|| crate::towns::nearest(la, lo, TOWN_KM)).flatten() {
                        Some((t, d)) => ("town", t.label(), d),
                        None => ("none", String::new(), 0.0),
                    },
                };
                found.push(FoundPlace { key: r.file.clone(), files: 1, latitude: la, longitude: lo, status: status.into(), suggestion, km, chosen: false });
                members.push(vec![i]);
            }
        }
    }
    for (f, rows) in found.iter_mut().zip(&members) {
        let choice = plan.places.iter().find(|c| c.key == f.key && matches!(c.action.as_str(), "place" | "new" | "leave"));
        f.chosen = choice.is_some();
        let (place, note) = match choice {
            Some(c) if c.action == "leave" => (String::new(), String::new()),
            Some(c) => (c.name.trim().to_string(), String::new()),
            None if f.status == "known" => (f.suggestion.clone(), String::new()),
            None if f.status == "town" => (String::new(), format!("Google Photos has a location near {}; choose under Places found.", f.suggestion)),
            None => (String::new(), "Google Photos has a location that is not near any place in this Journey; choose under Places found.".into()),
        };
        for &i in rows {
            let r = &mut plan.rows[i];
            r.place = place.clone();
            r.place_from_location = true;
            r.place_note = note.clone();
        }
    }
    // Groups that need a choice first, then the most files.
    found.sort_by(|a, b| (a.chosen || a.status == "known").cmp(&(b.chosen || b.status == "known")).then(b.files.cmp(&a.files)));
    found
}

/// What to do with one name found in a plan, for every file that has it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct NameChoice {
    pub name: String,
    /// `person` (the individual in `id`), `new` (add a person with this
    /// name), or `leave` (leave the name out).
    pub action: String,
    pub id: String,
    /// With `person`: keep the name as one of their other names, so the
    /// next import matches it on its own.
    pub remember: bool,
}

/// One name found in a plan's People.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FoundName {
    pub name: String,
    /// How many files have it.
    pub files: usize,
    /// `known` (one individual has it), `new` (nobody in the Journey yet)
    /// or `several` (more than one individual has it).
    pub status: String,
    /// The individuals with this name or other name, as (id, name).
    pub matches: Vec<(String, String)>,
    /// A choice was made for it.
    pub chosen: bool,
}

fn choice_for<'a>(names: &'a [NameChoice], name: &str) -> Option<&'a NameChoice> {
    let n = norm(name);
    names.iter().find(|c| norm(&c.name) == n && matches!(c.action.as_str(), "person" | "new" | "leave"))
}

/// Every name in the plan's People, once, and each row's `people_note`
/// worked out again from the Journey and the choices made, so a note goes
/// away once its name is matched.
pub fn check_names(plan: &mut Plan, known: &Known) -> Vec<FoundName> {
    let mut found: Vec<FoundName> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for row in plan.rows.iter_mut() {
        let (mut new, mut several) = (Vec::new(), Vec::new());
        let mut seen = Vec::new();
        for entry in split_list(&row.people) {
            let (name, _) = name_and_role(&entry);
            let key = norm(&name);
            if key.trim().is_empty() || seen.contains(&key) {
                continue;
            }
            seen.push(key.clone());
            let matches = known.matches(&name);
            let chosen = choice_for(&plan.names, &name).is_some();
            let i = *at.entry(key).or_insert_with(|| {
                found.push(FoundName {
                    name: name.clone(),
                    files: 0,
                    status: match matches.len() { 0 => "new", 1 => "known", _ => "several" }.into(),
                    matches: matches.clone(),
                    chosen,
                });
                found.len() - 1
            });
            found[i].files += 1;
            if chosen {
                continue;
            }
            match matches.len() {
                0 => new.push(name),
                1 => {}
                _ => several.push(name),
            }
        }
        let mut notes = Vec::new();
        if !several.is_empty() {
            notes.push(format!("More than one person is called {}; choose who under Names found.", several.join(", ")));
        }
        if !new.is_empty() {
            let from = if row.sidecar.is_empty() { "Not in this Journey yet" } else { "Tagged in Google Photos but not in this Journey yet" };
            notes.push(format!("{from}: {}.", new.join(", ")));
        }
        row.people_note = notes.join(" ");
    }
    // Names that need a choice first, then the rest, each by name.
    let rank = |f: &FoundName| if f.chosen || f.status == "known" { 1 } else { 0 };
    found.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    found
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ScanOptions {
    /// Use a picture's own date ahead of dates in names (off: scans).
    pub photo_dates_first: bool,
    /// Put each folder's memories into a story named after the folder.
    pub story_per_folder: bool,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// A date read from a file or folder name, typed as known and readable by
/// [`dates::parse`]: ranges (`1980-1985`, `1980 to 1985`) become
/// `1980 to 1985`, `June 1970` becomes `1970-06`, then the file-name patterns
/// (full dates, `1970-06`, `1970s`, `1970`).
pub fn name_date(name: &str) -> Option<String> {
    let ok = |d: String| dates::parse(&d).ok().flatten().map(|_| d);
    static RANGE: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = re(&RANGE, r"(?i)(?:^|\D)((?:18|19|20)\d\d)\s*(?:-|–|—|_|\bto\b|\bthrough\b)\s*((?:18|19|20)\d\d)(?:\D|$)").captures(name) {
        let (a, b): (i32, i32) = (c[1].parse().ok()?, c[2].parse().ok()?);
        if a < b {
            return ok(format!("{a} to {b}"));
        }
    }
    static MONTH: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = re(&MONTH, r"(?i)\b(jan|feb|mar|apr|may|jun|jul|aug|sep|sept|oct|nov|dec)[a-z]*\.?[ _-]+((?:18|19|20)\d\d)(?:\D|$)").captures(name) {
        let m = MONTHS.iter().position(|x| c[1].to_lowercase().starts_with(x))? + 1;
        return ok(format!("{}-{m:02}", &c[2]));
    }
    file_name_date(&format!("{name}.x")).and_then(ok)
}

/// A file name without copy marks and scanner numbering (`copy (2)`,
/// `scan001`, `Copy of`), for the title.
fn without_noise(stem: &str) -> String {
    static NOISE: OnceLock<Regex> = OnceLock::new();
    let t = re(&NOISE, r"(?i)\bcopy of\b|\s*-\s*copy\b|\bcopy\b|\(\d+\)|\bscan[_ -]?\d+\b").replace_all(stem, " ");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn stem_of(file: &str) -> &str {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    name.rsplit_once('.').map(|(s, _)| s).filter(|s| !s.is_empty()).unwrap_or(name)
}

/// A title for a file: its name without dates, camera codes and noise, or
/// else its folder's name, or else the file name itself.
pub fn guess_title(file: &str) -> String {
    let parts: Vec<&str> = file.split(['/', '\\']).collect();
    let stem = stem_of(file);
    if let Some(t) = file_name_title(&format!("{}.x", without_noise(stem))) {
        return t;
    }
    for folder in parts.iter().rev().skip(1) {
        if let Some(t) = file_name_title(&format!("{}.x", without_noise(folder))) {
            return t;
        }
    }
    stem.to_string()
}

/// Lower case letters and digits with single spaces, padded, for whole-word matching.
pub(crate) fn norm(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() { c.to_lowercase().next().unwrap_or(c) } else { ' ' }).collect();
    format!(" {} ", t.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// People, pets and places already in the Journey, for suggesting.
pub struct Known {
    /// Normalized full name or other name: the individuals it names.
    names: HashMap<String, Vec<(String, String)>>,
    /// Normalized given name: the people with it.
    given: HashMap<String, Vec<(String, String)>>,
    places: HashMap<String, Vec<String>>,
    /// Places with a location, for Google Takeout's coordinates.
    located: Vec<(String, f64, f64)>,
}

impl Known {
    pub fn read(j: &Journey) -> Result<Known> {
        let mut k = Known { names: HashMap::new(), given: HashMap::new(), places: HashMap::new(), located: Vec::new() };
        let mut st = j.conn().prepare(
            "SELECT i.id, i.name, p.given_name, p.other_names FROM individual i LEFT JOIN person p USING (id) ORDER BY i.name, i.id",
        )?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?)))?;
        for row in rows {
            let (id, name, given, other) = row?;
            let mut keys = vec![name.clone()];
            keys.extend(other.unwrap_or_default().split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
            for key in keys {
                let n = norm(&key);
                if n.trim().len() >= 3 {
                    let v = k.names.entry(n).or_default();
                    if !v.iter().any(|(i, _)| *i == id) {
                        v.push((id.clone(), name.clone()));
                    }
                }
            }
            if let Some(g) = given.filter(|g| norm(g).trim().len() >= 3) {
                k.given.entry(norm(&g)).or_default().push((id.clone(), name.clone()));
            }
        }
        let mut st = j.conn().prepare("SELECT name FROM place ORDER BY name, id")?;
        for name in st.query_map([], |r| r.get::<_, String>(0))? {
            let name = name?;
            let n = norm(&name);
            if n.trim().len() >= 3 {
                k.places.entry(n).or_default().push(name);
            }
        }
        let mut st = j.conn().prepare("SELECT name, latitude, longitude FROM place WHERE latitude IS NOT NULL AND longitude IS NOT NULL ORDER BY name, id")?;
        for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?)))? {
            k.located.push(row?);
        }
        Ok(k)
    }

    /// The closest place within a kilometre of a point.
    fn near(&self, lat: f64, lon: f64) -> Option<String> {
        self.located
            .iter()
            .map(|(n, a, b)| (n, crate::takeout::km((lat, lon), (*a, *b))))
            .filter(|(_, d)| *d <= 1.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| n.clone())
    }

    /// The individuals with this name or other name, as (id, name).
    /// A first name alone counts when no full or other name matches.
    pub fn matches(&self, name: &str) -> Vec<(String, String)> {
        let n = norm(name);
        self.names.get(&n).or_else(|| self.given.get(&n)).cloned().unwrap_or_default()
    }

    /// A tagged name: the person it is in this Journey, `Err` with the name
    /// when several people have it, or `Ok(None)` when nobody does.
    fn tagged(&self, name: &str) -> std::result::Result<Option<String>, String> {
        // Google often tags people by first name ("Diana"): one person with it is them.
        match self.matches(name).as_slice() {
            [one] => Ok(Some(one.1.clone())),
            [_, _, ..] => Err(name.to_string()),
            _ => Ok(None),
        }
    }

    /// People named in these folder and file names, and the names that
    /// belong to more than one person (asked about, not guessed).
    fn people(&self, segments: &[String]) -> (Vec<String>, Vec<String>) {
        let mut found: Vec<(String, String)> = Vec::new();
        let mut unsure = Vec::new();
        for seg in segments {
            let hay = norm(seg);
            let add = |found: &mut Vec<(String, String)>, who: &[(String, String)], key: &str, unsure: &mut Vec<String>| {
                match who {
                    [one] => {
                        if !found.iter().any(|(i, _)| *i == one.0) {
                            found.push(one.clone());
                        }
                    }
                    [_, _, ..] => {
                        // "ada" as written in names: "Ada".
                        let k = key.split_whitespace().map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join(" ");
                        if !unsure.contains(&k) {
                            unsure.push(k);
                        }
                    }
                    [] => {}
                }
            };
            for (key, who) in &self.names {
                if hay.contains(key.as_str()) {
                    add(&mut found, who, key, &mut unsure);
                }
            }
            // A first name alone counts only when one person has it.
            for (key, who) in &self.given {
                if !hay.contains(key.as_str()) || self.names.get(key).is_some_and(|n| n.len() > 1) {
                    continue;
                }
                // Shared by several people: asked about, unless one of them is already named in full.
                if who.len() == 1 || !who.iter().any(|w| found.iter().any(|f| f.0 == w.0)) {
                    add(&mut found, who, key, &mut unsure);
                }
            }
        }
        // Sorted by name, so the same files always give the same list.
        found.sort_by(|a, b| a.1.cmp(&b.1));
        (found.into_iter().map(|(_, n)| n).collect(), unsure)
    }

    fn place(&self, segments: &[String]) -> Option<String> {
        for seg in segments.iter().rev() {
            let hay = norm(seg);
            let mut hits: Vec<&String> = self.places.iter().filter(|(k, v)| v.len() == 1 && hay.contains(k.as_str())).map(|(_, v)| &v[0]).collect();
            // The longest name wins: "The Rowan house" over "Rowan".
            hits.sort_by_key(|n| std::cmp::Reverse(n.len()));
            if let Some(h) = hits.first() {
                return Some((*h).clone());
            }
        }
        None
    }
}

fn rel_path(base: &Path, p: &Path) -> String {
    match p.strip_prefix(base) {
        Ok(r) => r.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"),
        Err(_) => p.display().to_string(),
    }
}

/// The full path of a row's file.
pub fn resolve(base: &str, file: &str) -> PathBuf {
    let f = Path::new(file);
    if f.is_absolute() || file.starts_with('/') || file.contains(":\\") {
        return f.to_path_buf();
    }
    let mut p = PathBuf::from(base);
    for part in file.split(['/', '\\']).filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}

/// Make a plan for a folder and its subfolders (nothing is copied). Files
/// the app can't bring in are reported through `ctx` as skipped.
pub fn scan(j: &Journey, folder: &Path, opts: ScanOptions, ctx: &JobContext) -> Result<Plan> {
    scan_with(&Known::read(j)?, j.root(), folder, opts, ctx)
}

/// [`scan`] with the names read beforehand, so the Journey isn't held while
/// a large folder is looked through.
pub fn scan_with(known: &Known, journey_root: &Path, folder: &Path, opts: ScanOptions, ctx: &JobContext) -> Result<Plan> {
    if !folder.is_dir() {
        return Err(Error::Invalid(format!("{} is not a folder.", folder.display())));
    }
    ctx.message("Looking through the folder…");
    scan_paths(known, journey_root, folder, &[folder.to_path_buf()], opts, ctx)
}

/// A plan for zip files chosen one by one (such as the parts of a Google
/// Takeout), read where they are. Paths in the plan start from their folder.
pub fn scan_zips(known: &Known, journey_root: &Path, zips: &[PathBuf], opts: ScanOptions, ctx: &JobContext) -> Result<Plan> {
    let Some(first) = zips.first() else { return Err(Error::Invalid("Choose at least one zip file.".into())) };
    if let Some(z) = zips.iter().find(|z| !crate::takeout_zip::is_zip(z) || !z.is_file()) {
        return Err(Error::Invalid(format!("{} is not a zip file.", z.display())));
    }
    let folder = first.parent().unwrap_or(Path::new("")).to_path_buf();
    // Zips from different folders: the paths start from the folder they share.
    let folder = zips.iter().fold(folder, |f, z| f.ancestors().find(|a| z.starts_with(a)).map(Path::to_path_buf).unwrap_or_default());
    ctx.message("Looking inside the zip files…");
    scan_paths(known, journey_root, &folder, zips, opts, ctx)
}

fn scan_paths(known: &Known, journey_root: &Path, folder: &Path, paths: &[PathBuf], opts: ScanOptions, ctx: &JobContext) -> Result<Plan> {
    let (mut files, found) = crate::import::gather_all(paths, journey_root, ctx, true)?;
    let (zips, jsons): (Vec<PathBuf>, Vec<PathBuf>) = found.into_iter().partition(|p| crate::takeout_zip::is_zip(p));
    // Zips (a Google Takeout comes as several) are read where they are.
    let (mut zip_media, mut zip_jsons) = (Vec::new(), Vec::new());
    for z in &zips {
        ctx.check()?;
        ctx.message(format!("Looking inside {}…", z.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        match crate::takeout_zip::list(z) {
            Ok(entries) => {
                for e in entries {
                    let ext = Path::new(&e.inside).extension().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                    if classify(&ext).is_some() {
                        zip_media.push(e);
                    } else if ext.eq_ignore_ascii_case("json") {
                        zip_jsons.push(e);
                    } else if crate::takeout::motion_part(&e.path).is_none() && !e.inside.split('/').last().is_some_and(|n| n.starts_with('.')) {
                        ctx.skip(e.path.display().to_string(), "Not a picture, recording, video, text or document file the app can bring in.");
                    }
                }
            }
            Err(e) => ctx.skip(z.display().to_string(), e.to_string()),
        }
    }
    let mut jsons = jsons;
    jsons.extend(zip_jsons.into_iter().map(|e| e.path));
    let in_zip: HashSet<PathBuf> = zip_media.iter().map(|e| e.path.clone()).collect();
    files.extend(zip_media.into_iter().map(|e| e.path));
    // Google Takeout (sprint 12): skip Google's edited copies, pair each photo with its sidecar.
    let all: HashSet<PathBuf> = files.iter().cloned().collect();
    let files: Vec<PathBuf> = files
        .into_iter()
        .filter(|p| {
            let edited = crate::takeout::is_edited_copy(p, &all);
            if edited {
                ctx.skip(p.display().to_string(), "An edited copy saved by Google Photos; the original beside it is brought in.");
            }
            !edited
        })
        .collect();
    // Paired across the parts of a Takeout, whether unzipped or not.
    let sidecars = crate::takeout::pair_across(&files, &jsons);
    let paired: HashSet<&Path> = sidecars.values().map(|j| j.as_path()).collect();
    let takeout_folders: HashSet<&Path> = sidecars.values().filter_map(|j| j.parent()).collect();
    for j in &jsons {
        // Album details and other Takeout files are passed over quietly.
        if !paired.contains(j.as_path()) && !j.parent().is_some_and(|d| takeout_folders.contains(d)) && !crate::takeout::is_takeout_json(j) {
            ctx.skip(j.display().to_string(), "Not a picture, recording, video, text or document file the app can bring in.");
        }
    }
    let base_name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // A .txt with the same name as a picture or recording is its description.
    let ext = |p: &Path| p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let stems: HashSet<PathBuf> = files.iter().filter(|p| ext(p) != "txt").map(|p| p.with_extension("")).collect();
    let mut descriptions: HashMap<PathBuf, String> = HashMap::new();
    for p in files.iter().filter(|p| ext(p) == "txt") {
        if stems.contains(&p.with_extension("")) {
            if let Some(t) = crate::export::read_text(p) {
                descriptions.insert(p.with_extension(""), t.trim().chars().take(4000).collect());
            }
        }
    }
    let mut rows = Vec::new();
    ctx.message("Analyzing content…");
    ctx.set_total(files.len() as u64);
    for p in &files {
        ctx.check()?;
        if ext(p) == "txt" && descriptions.contains_key(&p.with_extension("")) {
            continue;
        }
        let rel = rel_path(folder, p);
        let kind = classify(&ext(p)).map(|k| k.0).unwrap_or_default().to_string();
        // Closest first: the file, its folder, then parent folders and the chosen folder itself.
        let mut segments: Vec<String> = rel.split('/').map(str::to_string).collect();
        segments.insert(0, base_name.clone());
        let file_name = segments.last().cloned().unwrap_or_default();
        let google = sidecars.get(p).and_then(|j| crate::takeout::read(j));
        let mut date = None;
        for (i, seg) in segments.iter().enumerate().rev() {
            let text = if i == segments.len() - 1 { stem_of(seg).to_string() } else { seg.clone() };
            if let Some(d) = name_date(&text) {
                let source = if i == segments.len() - 1 { "file name".to_string() } else { format!("folder \u{201c}{seg}\u{201d}") };
                date = Some((d, source));
                break;
            }
        }
        // Google's date beats folder dates ("Photos from 2019"), not a date in the file name.
        if let Some(d) = google.as_ref().and_then(|g| g.date.clone()) {
            if date.as_ref().is_none_or(|(_, s)| s != "file name") {
                date = Some((d, "Google Photos".into()));
            }
        }
        if kind == "picture" && (date.is_none() || opts.photo_dates_first) && !in_zip.contains(p) {
            if let Some(d) = crate::import::picture_details(p).date {
                date = Some((d, "photo facts".into()));
            }
        }
        let names: Vec<String> = segments.iter().enumerate().map(|(i, s)| if i == segments.len() - 1 { stem_of(s).to_string() } else { s.clone() }).collect();
        let (mut people, mut unsure) = known.people(&names);
        let notes: Vec<String> = Vec::new();
        let place = known.place(&names);
        let mut located: Option<(f64, f64)> = None;
        if let Some(g) = &google {
            // Tagged people go in the preview's People; anyone not in the
            // Journey is matched under Names found, or added only if the
            // person ticks "Add people…".
            for name in &g.people {
                match known.tagged(name) {
                    Ok(Some(n)) if !people.contains(&n) => people.push(n),
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        if !people.contains(name) {
                            people.push(name.clone());
                        }
                    }
                    Err(n) if !unsure.contains(&n) => unsure.push(n),
                    Err(_) => {}
                }
            }
            // Named under Places found (check_places).
            located = g.latitude.zip(g.longitude);
        }
        // A name several people have goes in People too: Names found asks who it is.
        for n in unsure {
            if !people.iter().any(|p| norm(p) == norm(&n)) {
                people.push(n);
            }
        }
        let note = notes.join(" ");
        let folder_name = if segments.len() > 2 { segments[segments.len() - 2].clone() } else { String::new() };
        rows.push(PlanRow {
            include: true,
            title: guess_title(&rel),
            date: date.as_ref().map(|d| d.0.clone()).unwrap_or_default(),
            date_source: date.map(|d| d.1).unwrap_or_default(),
            people: people.join("; "),
            place: place.unwrap_or_default(),
            story: if opts.story_per_folder { folder_name.clone() } else { String::new() },
            description: descriptions
                .get(&p.with_extension(""))
                .cloned()
                .or_else(|| google.as_ref().and_then(|g| g.description.clone()))
                .unwrap_or_default(),
            kind,
            note,
            file: rel,
            memory: String::new(),
            group: String::new(),
            people_note: String::new(),
            latitude: located.map(|l| l.0),
            longitude: located.map(|l| l.1),
            place_from_location: false,
            place_note: String::new(),
            sidecar: sidecars.get(p).map(|j| j.display().to_string()).unwrap_or_default(),
        });
        ctx.advance_item(file_name);
    }
    suggest_groups(&mut rows);
    let mut plan = Plan { base: folder.display().to_string(), rows, names: vec![], places: vec![] };
    check_names(&mut plan, known);
    check_places(&mut plan, known, true);
    Ok(plan)
}

/// Rows with the same folder, date and people get a suggested group name
/// (the folder's title, or the date): offered in the preview, never applied.
pub fn suggest_groups(rows: &mut [PlanRow]) {
    let key = |r: &PlanRow| {
        let folder = r.file.rsplit_once('/').map(|(f, _)| f.to_string()).unwrap_or_default();
        (folder, r.date.clone(), r.people.clone())
    };
    let mut count: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for r in rows.iter() {
        *count.entry(key(r)).or_default() += 1;
    }
    for r in rows.iter_mut() {
        let k = key(r);
        if count[&k] < 2 {
            r.group.clear();
            continue;
        }
        let folder_title = k.0.rsplit('/').next().and_then(|f| file_name_title(&format!("{}.x", without_noise(f))));
        let date_words = dates::parse(&k.1).ok().flatten().map(|d| d.words);
        r.group = match (folder_title, date_words) {
            (Some(t), _) => t,
            (None, Some(w)) => w,
            (None, None) => k.0.rsplit('/').next().unwrap_or("These files").to_string(),
        };
    }
}

// ------------------------------------------------------------ CSV

/// The plan's columns, in order (blank sample and saved plans alike).
pub const COLUMNS: [&str; 9] = ["file", "include", "title", "date", "people", "place", "memory", "story", "description"];

fn csv_field(s: &str) -> String {
    if s.contains([',', ';', '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// The plan as CSV text, with paths relative to the CSV's own folder where
/// they can be. Starts with a byte order mark so spreadsheets read it as UTF-8.
pub fn to_csv(plan: &Plan, csv_path: &Path) -> String {
    let dir = csv_path.parent().unwrap_or(Path::new(""));
    let mut out = String::from("\u{feff}");
    out.push_str(&COLUMNS.join(","));
    out.push_str("\r\n");
    for r in &plan.rows {
        let file = rel_path(dir, &resolve(&plan.base, &r.file));
        let cells: [&str; 9] = [&file, if r.include { "yes" } else { "no" }, &r.title, &r.date, &r.people, &r.place, &r.memory, &r.story, &r.description];
        out.push_str(&cells.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

/// Split CSV text into records (RFC 4180 quoting; `,` or `;` between fields).
fn records(text: &str) -> Vec<Vec<String>> {
    let text = text.trim_start_matches('\u{feff}');
    let first = text.lines().next().unwrap_or_default();
    let sep = if first.matches(';').count() > first.matches(',').count() { ';' } else { ',' };
    let mut out = Vec::new();
    let mut rec = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            '\r' => {}
            '\n' => {
                rec.push(std::mem::take(&mut field));
                out.push(std::mem::take(&mut rec));
            }
            c if c == sep => rec.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !rec.is_empty() {
        rec.push(field);
        out.push(rec);
    }
    out.retain(|r| r.iter().any(|f| !f.trim().is_empty()));
    out
}

/// Read an import plan written by the app, by hand or by a script. Columns
/// are found by their heading (any order, any case; unknown ones are
/// ignored); only `file` is needed. Relative paths start from the CSV's folder.
pub fn from_csv(text: &str, csv_path: &Path) -> Result<Plan> {
    let recs = records(text);
    let Some((head, body)) = recs.split_first() else {
        return Err(Error::Invalid("The import plan is empty.".into()));
    };
    let col: HashMap<String, usize> = head.iter().enumerate().map(|(i, h)| (h.trim().to_lowercase(), i)).collect();
    let Some(&file_col) = col.get("file") else {
        return Err(Error::Invalid("The import plan needs a column headed \"file\". See the sample plan's read-me.".into()));
    };
    let base = csv_path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let mut rows = Vec::new();
    for r in body {
        let get = |name: &str| col.get(name).and_then(|i| r.get(*i)).map(|s| s.trim().to_string()).unwrap_or_default();
        let file = r.get(file_col).map(|s| s.trim().to_string()).unwrap_or_default();
        if file.is_empty() {
            continue;
        }
        let path = resolve(&base, &file);
        let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let kind = classify(&ext).map(|k| k.0).unwrap_or_default().to_string();
        let include = !matches!(get("include").to_lowercase().as_str(), "no" | "n" | "false" | "0" | "skip");
        let mut note = String::new();
        if kind.is_empty() {
            note = "Not a picture, recording, video, text or document file the app can bring in.".into();
        } else if !path.is_file() {
            note = "The file was not found.".into();
        } else if let Err(e) = dates::parse(&get("date")) {
            note = e.to_string();
        }
        let title = get("title");
        rows.push(PlanRow {
            include: include && (note.is_empty() || note.starts_with("The date")),
            title: if title.is_empty() { guess_title(&file) } else { title },
            date_source: if get("date").is_empty() { String::new() } else { "import plan".into() },
            date: get("date"),
            people: get("people"),
            place: get("place"),
            memory: get("memory"),
            story: get("story"),
            description: get("description"),
            kind,
            note,
            file,
            group: String::new(),
            sidecar: String::new(),
            ..PlanRow::default()
        });
    }
    suggest_groups(&mut rows);
    Ok(Plan { base, rows, names: vec![], places: vec![] })
}

/// A blank import plan to fill in, with sample rows from the Rowan family,
/// and a short read-me explaining each column.
pub fn sample_plan() -> (String, String) {
    let rows = [
        ["Grandpa's box/1952 Christmas.jpg", "yes", "Christmas at the Rowan house", "December 1952", "Walter Rowan; June Rowan (in the picture)", "The Rowan house", "Christmas 1952", "Holidays", ""],
        ["Grandpa's box/1952 Christmas tree.jpg", "yes", "", "December 1952", "", "The Rowan house", "Christmas 1952", "Holidays", ""],
        ["Trips/1980-1985 Lake Powell/boat.jpg", "yes", "On the boat", "1980 to 1985", "Ellen Rowan", "", "", "", "Taken on one of the summer trips."],
        ["Recordings/june-sunday-mornings.mp3", "yes", "June on Sunday mornings", "about 1995", "June Rowan (speaking)", "", "", "", ""],
        ["Grandpa's box/scan001.jpg", "no", "", "", "", "", "", "", "Not ticked, so not brought in."],
    ];
    let plan = Plan {
        base: String::new(),
        rows: rows
            .iter()
            .map(|r| PlanRow {
                file: r[0].into(),
                include: r[1] == "yes",
                title: r[2].into(),
                date: r[3].into(),
                people: r[4].into(),
                place: r[5].into(),
                memory: r[6].into(),
                story: r[7].into(),
                description: r[8].into(),
                ..PlanRow::default()
            })
            .collect(),
        names: vec![],
        places: vec![],
    };
    let csv = to_csv(&plan, Path::new("import-plan.csv"));
    let readme = "\
IMPORT PLAN: HOW TO FILL IT IN

An import plan is a plain CSV file that a spreadsheet (Excel, LibreOffice,
Google Sheets) or a script (Python, PowerShell) can write. In the app, choose
Add > Bring in a folder with a preview > Open an import plan. Nothing is added
until you check the preview and choose Bring in.

One row per file. The first row holds the column names; columns can be in any
order and only \"file\" is needed. People and places are separated by
semicolons (;).

file         The file's path. Paths not starting with a drive or / start from
             the folder this CSV is in, for example: Grandpa's box/1952.jpg
include      yes or no. Blank means yes.
title        The memory's title. Blank: guessed from the file name.
date         As you know it: 1970, June 1970, 1970-06-14, 1970s,
             1980 to 1985, about 1950, before 1960.
people       People and pets already in the Journey, by name. Add the role in
             brackets if you like: June Rowan (in the picture). Names not in
             the Journey are only added if you tick \"Add people and places
             that are not in this Journey yet\" in the preview.
place        One place, by name, the same way.
memory       Blank: the file becomes its own memory. Rows with the same text
             here become one memory with that title.
story        A story to put the memory in. A story that doesn't exist yet is
             made.
description  A few words about the memory.

The app makes its own copies; the files named here are only read, never
changed. Files already in the Journey are not copied again, so running the
same plan twice brings in only new files.
";
    (csv, readme.replace('\n', "\r\n"))
}

// ------------------------------------------------------------ the import

/// The full paths of the ticked rows' files, in order.
pub fn plan_files(plan: &Plan) -> Vec<PathBuf> {
    plan.rows.iter().filter(|r| r.include).map(|r| resolve(&plan.base, &r.file)).collect()
}

/// Room for this many bytes, plus a margin, where the Journey is kept.
pub fn check_space(journey_root: &Path, needed: u64) -> Result<()> {
    let Ok(free) = fs4::available_space(journey_root) else { return Ok(()) };
    let margin = 200 * 1024 * 1024;
    if free < needed.saturating_add(margin) {
        let gb = |b: u64| format!("{:.1} GB", b as f64 / 1e9);
        return Err(Error::Invalid(format!(
            "There isn't enough free space for these files: they need about {} and the drive holding the Journey has {} free. Free some space or untick some files.",
            gb(needed.saturating_add(margin)),
            gb(free)
        )));
    }
    Ok(())
}

/// The total size of the files, for [`check_space`].
pub fn total_size(files: &[PathBuf]) -> u64 {
    files.iter().filter_map(|p| crate::takeout_zip::size(p)).sum()
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PlanReport {
    pub imported: usize,
    pub memories: Vec<String>,
    pub duplicates: usize,
    /// People, places and dates that could not be used, in plain words.
    pub notes: Vec<String>,
    /// Each original brought in and the ID of its new content, so its
    /// preview picture can become its thumbnail (sprint 18).
    #[serde(skip)]
    pub made: Vec<(PathBuf, String)>,
}

/// Split `June Rowan (in the picture)` into the name and the role.
fn name_and_role(s: &str) -> (String, Option<String>) {
    let s = s.trim();
    if let Some((name, rest)) = s.rsplit_once('(') {
        if let Some(role) = rest.strip_suffix(')') {
            return (name.trim().to_string(), Some(role.trim().to_lowercase()));
        }
    }
    (s.to_string(), None)
}

fn split_list(s: &str) -> Vec<String> {
    s.split(';').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

/// The person or pet with this name or other name, if exactly one.
pub(crate) fn find_individual(c: &rusqlite::Connection, name: &str) -> Result<Vec<String>> {
    let n = norm(name);
    let mut st = c.prepare("SELECT i.id, i.name, p.other_names FROM individual i LEFT JOIN person p USING (id)")?;
    let mut out = Vec::new();
    for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))? {
        let (id, nm, other) = r?;
        if norm(&nm) == n || other.unwrap_or_default().split([',', ';']).any(|o| norm(o) == n) {
            out.push(id);
        }
    }
    Ok(out)
}

fn find_by_name(c: &rusqlite::Connection, table: &str, name: &str) -> Result<Vec<String>> {
    let n = norm(name);
    let mut st = c.prepare(&format!("SELECT id, name FROM {table}"))?;
    let mut out = Vec::new();
    for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (id, nm) = r?;
        if norm(&nm) == n {
            out.push(id);
        }
    }
    Ok(out)
}

struct Group {
    title: String,
    date: String,
    place: String,
    story: String,
    people: Vec<String>,
    descriptions: Vec<String>,
    contents: Vec<(String, String)>,
}

impl Journey {
    /// Record a prepared plan: the files, then their memories (one per row,
    /// or one per memory name), with dates, places, people, descriptions and
    /// stories, all in one action. With `add_missing`, people and places
    /// named in the plan that are not in the Journey are added; otherwise
    /// they are left out and listed in the report.
    pub fn finish_plan(&mut self, mut batch: Batch, plan: &Plan, add_missing: bool) -> Result<PlanReport> {
        let items = std::mem::take(&mut batch.items);
        let mut report = PlanReport { duplicates: batch.duplicates.len(), ..PlanReport::default() };
        let root = self.root().to_path_buf();
        let author = self.author().to_string();
        let mut moved: Vec<PathBuf> = Vec::new();
        let mut finder = crate::takeout::Finder::default();
        // A plan from the preview already names each file's sidecar. Only a
        // plan written by hand has the Takeout's parts looked through here,
        // and only the parts of the Takeouts its files come from (never every
        // zip in a Downloads folder; this runs while the Journey is held).
        let mut zip_sidecars = if plan.rows.iter().any(|r| !r.sidecar.is_empty()) {
            HashMap::new()
        } else {
            let sources: Vec<PathBuf> = items.iter().map(|p| p.source.clone()).collect();
            crate::takeout_zip::sidecars_for(&sources)
        };
        for r in plan.rows.iter().filter(|r| r.sidecar.to_lowercase().ends_with(".json")) {
            zip_sidecars.insert(resolve(&plan.base, &r.file), PathBuf::from(&r.sidecar));
        }
        let result = self.act(|a| {
            let mut made: HashMap<PathBuf, (String, String)> = HashMap::new();
            for p in &items {
                if crate::import::existing_content(a.conn(), &p.sha256)?.is_some() {
                    report.duplicates += 1;
                    continue;
                }
                let id = crate::import::record_prepared(a, &root, &author, p, None, &mut moved)?;
                // Google Takeout's sidecar goes beside the copy, read-only.
                let side = zip_sidecars.get(&p.source).cloned().or_else(|| finder.beside(&p.source));
                if let Some(side) = side {
                    let rel: String = a.conn().query_row("SELECT path FROM content_file WHERE content_id = ?1 AND is_copy = 0", [&id], |r| r.get(0))?;
                    let dest = crate::folder::absolute(&root, &format!("{rel}.json"));
                    match crate::takeout_zip::split(&side) {
                        Some(_) => std::fs::write(&dest, crate::takeout_zip::read_small(&side, 1 << 20).unwrap_or_default())?,
                        None => {
                            std::fs::copy(&side, &dest)?;
                        }
                    }
                    moved.push(dest.clone());
                    crate::import::set_readonly(&dest, true)?;
                }
                made.insert(p.source.clone(), (id, p.kind.to_string()));
            }
            report.imported = made.len();
            report.made = made.iter().map(|(src, (id, _))| (src.clone(), id.clone())).collect();
            // Rows to memories, in the plan's order.
            let mut groups: Vec<Group> = Vec::new();
            let mut by_name: HashMap<String, usize> = HashMap::new();
            for r in plan.rows.iter().filter(|r| r.include) {
                let Some((id, kind)) = made.get(&resolve(&plan.base, &r.file)) else { continue };
                let key = r.memory.trim().to_lowercase();
                let at = match by_name.get(&key).filter(|_| !key.is_empty()) {
                    Some(i) => *i,
                    None => {
                        let title = [r.memory.trim(), r.title.trim()].into_iter().find(|t| !t.is_empty()).map(str::to_string).unwrap_or_else(|| guess_title(&r.file));
                        groups.push(Group { title, date: String::new(), place: String::new(), story: String::new(), people: vec![], descriptions: vec![], contents: vec![] });
                        if !key.is_empty() {
                            by_name.insert(key, groups.len() - 1);
                        }
                        groups.len() - 1
                    }
                };
                let g = &mut groups[at];
                g.contents.push((id.clone(), kind.clone()));
                for (field, value) in [(&mut g.date, &r.date), (&mut g.place, &r.place), (&mut g.story, &r.story)] {
                    if field.is_empty() {
                        *field = value.trim().to_string();
                    }
                }
                for p in split_list(&r.people) {
                    if !g.people.contains(&p) {
                        g.people.push(p);
                    }
                }
                if !r.description.trim().is_empty() && !g.descriptions.contains(&r.description.trim().to_string()) {
                    g.descriptions.push(r.description.trim().to_string());
                }
            }
            let mut notes: Vec<String> = Vec::new();
            let mut note = |n: String| {
                if !notes.contains(&n) {
                    notes.push(n);
                }
            };
            let mut stories: HashMap<String, String> = HashMap::new();
            let mut memories = Vec::new();
            // Places chosen as "new" under Places found, with the location of
            // the first file that has one.
            let mut new_places: HashMap<String, Option<(f64, f64)>> = HashMap::new();
            for c in plan.places.iter().filter(|c| c.action == "new" && !c.name.trim().is_empty()) {
                let at = plan.rows.iter().find(|r| r.place.trim() == c.name.trim() && r.latitude.is_some()).and_then(|r| r.latitude.zip(r.longitude));
                new_places.entry(norm(&c.name)).or_insert(at);
            }
            // People added for a name chosen as "new", once each.
            let mut added: HashMap<String, String> = HashMap::new();
            for g in &groups {
                let id = crate::memories::create_memory(a, &g.title)?;
                for (c, _) in &g.contents {
                    crate::memories::put_in(a, &id, c)?;
                }
                crate::memories::refresh_status(a, &id)?;
                match dates::parse(&g.date) {
                    Ok(Some(d)) => {
                        a.update("memory", id_key(&id), "date_edtf", json!(d.edtf))?;
                        a.update("memory", id_key(&id), "date", json!(d.nearest))?;
                    }
                    Ok(None) => {}
                    Err(_) => note(format!("The date \u{201c}{}\u{201d} could not be read, so \u{201c}{}\u{201d} has no date yet.", g.date, g.title)),
                }
                if !g.descriptions.is_empty() {
                    a.update("memory", id_key(&id), "caption", json!(g.descriptions.join("\n\n")))?;
                }
                if !g.place.is_empty() {
                    let found = find_by_name(a.conn(), "place", &g.place)?;
                    let place = match found.as_slice() {
                        [one] => Some(one.clone()),
                        [] if add_missing || new_places.contains_key(&norm(&g.place)) => {
                            let at = new_places.get(&norm(&g.place)).copied().flatten();
                            let id = a.create("place", crate::import::row(json!({"name": g.place})))?;
                            if let Some((la, lo)) = at {
                                a.update("place", id_key(&id), "latitude", json!(la))?;
                                a.update("place", id_key(&id), "longitude", json!(lo))?;
                            }
                            Some(id)
                        }
                        [] => {
                            note(format!("The place \u{201c}{}\u{201d} is not in this Journey, so it was left out.", g.place));
                            None
                        }
                        _ => {
                            note(format!("More than one place is called \u{201c}{}\u{201d}, so it was left out.", g.place));
                            None
                        }
                    };
                    if let Some(p) = place {
                        a.update("memory", id_key(&id), "place_id", json!(p))?;
                    }
                }
                let first_kind = g.contents.first().map(|c| c.1.as_str()).unwrap_or("picture");
                for entry in &g.people {
                    let (name, role) = name_and_role(entry);
                    // A choice under Names found wins over the name as written.
                    let who = match choice_for(&plan.names, &name).map(|c| (c.action.as_str(), c.id.clone())) {
                        Some(("leave", _)) => continue,
                        Some(("person", id)) if individual_exists(a.conn(), &id)? => id,
                        Some(("new", _)) => match added.get(&norm(&name)) {
                            Some(id) => id.clone(),
                            None => {
                                let id = add_person(a, &name)?;
                                added.insert(norm(&name), id.clone());
                                id
                            }
                        },
                        _ => {
                            let found = find_individual(a.conn(), &name)?;
                            match found.as_slice() {
                                [one] => one.clone(),
                                // A first name alone is the one person with it, as the preview said.
                                [] if given_ids(a.conn(), &name)?.len() == 1 => given_ids(a.conn(), &name)?.remove(0),
                                // A first name several people have is never added as someone new.
                                [] if given_ids(a.conn(), &name)?.len() > 1 => {
                                    note(format!("More than one person is called \u{201c}{name}\u{201d}; add the right one to \u{201c}{}\u{201d} by hand.", g.title));
                                    continue;
                                }
                                [] if add_missing => add_person(a, &name)?,
                                [] => {
                                    note(format!("\u{201c}{name}\u{201d} is not in this Journey, so was left out."));
                                    continue;
                                }
                                _ => {
                                    note(format!("More than one person is called \u{201c}{name}\u{201d}; add the right one to \u{201c}{}\u{201d} by hand.", g.title));
                                    continue;
                                }
                            }
                        }
                    };
                    let role = role
                        .filter(|r| crate::memories::ROLES.iter().any(|(_, rs)| rs.contains(&r.as_str())))
                        .unwrap_or_else(|| crate::memories::default_role(first_kind).to_string());
                    let taken: bool = a.conn().query_row(
                        "SELECT EXISTS(SELECT 1 FROM memory_individual WHERE memory_id = ?1 AND individual_id = ?2 AND role = ?3)",
                        [&id, &who, &role],
                        |r| r.get(0),
                    )?;
                    if !taken {
                        a.insert("memory_individual", crate::import::row(json!({"memory_id": id, "individual_id": who, "role": role, "certainty": "certain"})))?;
                    }
                }
                if !g.story.is_empty() {
                    let key = g.story.to_lowercase();
                    let story = match stories.get(&key) {
                        Some(s) => s.clone(),
                        None => {
                            let s = match find_by_name(a.conn(), "story", &g.story)?.first() {
                                Some(s) => s.clone(),
                                None => a.create("story", crate::import::row(json!({"name": g.story})))?,
                            };
                            stories.insert(key, s.clone());
                            s
                        }
                    };
                    crate::stories::append_memory(a, &story, &id)?;
                }
                memories.push(id);
            }
            // Names kept as other names, so the next import matches them.
            for c in plan.names.iter().filter(|c| c.action == "person" && c.remember) {
                remember_name(a, &c.id, &c.name)?;
            }
            report.memories = memories;
            report.notes = notes;
            Ok(())
        });
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

/// The people whose first name is `name`.
fn given_ids(c: &rusqlite::Connection, name: &str) -> Result<Vec<String>> {
    let n = norm(name);
    let mut st = c.prepare("SELECT id, given_name FROM person WHERE given_name IS NOT NULL")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (id, g) = row?;
        if norm(&g) == n {
            out.push(id);
        }
    }
    Ok(out)
}

fn individual_exists(c: &rusqlite::Connection, id: &str) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM individual WHERE id = ?1)", [id], |r| r.get(0))?)
}

/// Add `name` to a person's other names, unless it is already their name
/// or one of them (pets have no other names).
fn remember_name(a: &mut crate::Action, id: &str, name: &str) -> Result<()> {
    let row: Option<(String, Option<String>)> = a
        .conn()
        .query_row("SELECT i.name, p.other_names FROM individual i JOIN person p USING (id) WHERE i.id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok();
    let Some((own, other)) = row else { return Ok(()) };
    let other = other.unwrap_or_default();
    let n = norm(name);
    if norm(&own) == n || other.split([',', ';']).any(|o| norm(o) == n) {
        return Ok(());
    }
    let joined = if other.trim().is_empty() { name.trim().to_string() } else { format!("{}, {}", other.trim(), name.trim()) };
    a.update("person", id_key(id), "other_names", json!(joined))
}

/// Add a person with just a name, as the quick-add does.
pub(crate) fn add_person(a: &mut crate::Action, name: &str) -> Result<String> {
    let id = a.create("individual", crate::import::row(json!({"kind": "Person", "name": name})))?;
    let words: Vec<&str> = name.split_whitespace().collect();
    let (given, family) = match words.as_slice() {
        [one] => (Some(*one), None),
        [first, .., last] => (Some(*first), Some(*last)),
        [] => (None, None),
    };
    a.insert("person", crate::import::row(json!({"id": id, "given_name": given, "family_name": family})))?;
    Ok(id)
}

/// Whether an exported history, rather than a plain folder, was picked.
pub fn is_export(folder: &Path) -> bool {
    crate::export::is_export(folder)
}

