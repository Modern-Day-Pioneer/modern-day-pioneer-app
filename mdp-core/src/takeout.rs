//! Google Takeout sidecars (release 2, sprint 12).
//!
//! Google Photos exports each photo with a JSON file beside it holding what
//! was typed or tagged in Google Photos: the date taken, a description, a
//! location and the people tagged. Takeout names these files in several
//! ways, all handled here:
//!
//! - `IMG_1234.jpg.json`
//! - `IMG_1234.jpg.supplemental-metadata.json`, often cut short
//!   (`IMG_1234.jpg.supplemental-meta.json`, `IMG_1234.jpg.supp.json`)
//!   because Takeout keeps whole names to 51 characters
//! - long photo names cut short too (`a_very_long_photo_name_from_a_ph.json`)
//! - copies with the same name: `IMG_1234(1).jpg` goes with
//!   `IMG_1234.jpg(1).json` or `IMG_1234.jpg.supplemental-metadata(1).json`
//!
//! `-edited` copies (Google's saved edits) are left out of a folder import
//! when the original is beside them, and the sidecar is copied next to the
//! photo's copy in the Journey (the export keeps everything Google gave).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{Local, TimeZone};
use serde_json::Value;

/// What a sidecar says about its photo.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sidecar {
    /// `YYYY-MM-DD` on this computer's clock, from `photoTakenTime`.
    pub date: Option<String>,
    pub description: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// Names tagged in Google Photos, in the file's order.
    pub people: Vec<String>,
}

const SUPPLEMENTAL: &str = "supplemental-metadata";
/// Names shorter than this are never taken to be cut short.
const CUT_SHORT: usize = 30;

/// The `(n)` copy number at the end of `s`, and `s` without it.
fn copy_number(s: &str) -> (&str, Option<&str>) {
    if let Some(body) = s.strip_suffix(')') {
        if let Some(open) = body.rfind('(') {
            let n = &body[open + 1..];
            if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                return (&s[..open], Some(&body[open + 1..]));
            }
        }
    }
    (s, None)
}

/// The photo name a sidecar's file name points at (possibly cut short), and its copy number.
fn sidecar_key(json_name: &str) -> Option<(String, Option<String>)> {
    let lower = json_name.to_lowercase();
    let stem = &json_name[..lower.strip_suffix(".json")?.len()];
    let (stem, n) = copy_number(stem);
    let mut base = stem.to_string();
    // `.supplemental-metadata`, or the start of it.
    if let Some(dot) = base.rfind('.') {
        let tail = base[dot + 1..].to_lowercase();
        if !tail.is_empty() && SUPPLEMENTAL.starts_with(&tail) && base[..dot].contains('.') {
            base.truncate(dot);
        }
    }
    let base = base.trim_end_matches('.').to_string();
    if base.is_empty() {
        return None;
    }
    Some((base, n.map(str::to_string)))
}

/// Whether a sidecar keyed `key` (from [`sidecar_key`]) belongs to the photo `photo_name`.
#[cfg(test)]
fn matches(key: &(String, Option<String>), photo_name: &str) -> bool {
    match_kind(key, photo_name).is_some()
}

/// `Some(false)` for a full-name match, `Some(true)` when the sidecar's name
/// was cut short and only the start of the photo's name matches.
fn match_kind(key: &(String, Option<String>), photo_name: &str) -> Option<bool> {
    let (base, n) = key;
    let p = photo_name.to_lowercase();
    let b = base.to_lowercase();
    // `IMG_1234(1).jpg`: the copy number moves to the end of the sidecar's name.
    let (photo_plain, photo_n) = match p.rfind('.') {
        Some(dot) => {
            let (stem, pn) = copy_number(&p[..dot]);
            (format!("{stem}{}", &p[dot..]), pn.map(str::to_string))
        }
        None => {
            let (stem, pn) = copy_number(&p);
            (stem.to_string(), pn.map(str::to_string))
        }
    };
    let same_copy = |want: &Option<String>| match (n, want) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    };
    // Exact, or the photo's name with its copy number moved.
    let photo_stem = photo_plain.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| photo_plain.clone());
    if (b == p && n.is_none()) || (b == photo_plain && same_copy(&photo_n)) || (b == photo_stem && same_copy(&photo_n)) {
        return Some(false);
    }
    // Cut short by Takeout.
    (b.chars().count() >= CUT_SHORT && (p.starts_with(&b) && n.is_none() || photo_plain.starts_with(&b) && same_copy(&photo_n))).then_some(true)
}

/// Read a sidecar, on disk or inside a zip; `None` when it is not a Google Photos one.
pub fn read(path: &Path) -> Option<Sidecar> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => String::from_utf8(crate::takeout_zip::read_small(path, 1 << 20)?).ok()?,
    };
    let v: Value = serde_json::from_str(&text).ok()?;
    let o = v.as_object()?;
    if !o.contains_key("photoTakenTime") && !o.contains_key("creationTime") && !o.contains_key("geoData") {
        return None;
    }
    let stamp = |k: &str| -> Option<i64> {
        let t = o.get(k)?.get("timestamp")?;
        t.as_str().and_then(|s| s.trim().parse().ok()).or_else(|| t.as_i64())
    };
    let date = stamp("photoTakenTime")
        .filter(|t| *t > 0)
        .and_then(|t| Local.timestamp_opt(t, 0).single())
        .map(|d| d.format("%Y-%m-%d").to_string());
    let place = |k: &str| -> Option<(f64, f64)> {
        let g = o.get(k)?;
        let (la, lo) = (g.get("latitude")?.as_f64()?, g.get("longitude")?.as_f64()?);
        (la != 0.0 || lo != 0.0).then_some((la, lo))
    };
    let (latitude, longitude) = place("geoData").or_else(|| place("geoDataExif")).map(|(a, b)| (Some(a), Some(b))).unwrap_or((None, None));
    let description = o.get("description").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(|s| s.chars().take(4000).collect());
    let mut people = Vec::new();
    for p in o.get("people").and_then(Value::as_array).into_iter().flatten() {
        if let Some(name) = p.get("name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
            if !people.iter().any(|q: &String| q == name) {
                people.push(name.to_string());
            }
        }
    }
    Some(Sidecar { date, description, latitude, longitude, people })
}

/// The sidecar among `jsons` (files in the photo's own folder) for `photo`.
/// `others` are the names of the other files in that folder: a sidecar whose
/// name was cut short is left out when it fits more than one of them, rather
/// than giving one photo another photo's date and people.
pub fn find<'a>(photo: &Path, jsons: &'a [PathBuf], others: &[String]) -> Option<&'a PathBuf> {
    let name = photo.file_name()?.to_string_lossy().into_owned();
    let mut best: Option<(&PathBuf, usize, (String, Option<String>), bool)> = None;
    for j in jsons {
        let Some(key) = j.file_name().and_then(|n| sidecar_key(&n.to_string_lossy())) else { continue };
        if let Some(cut) = match_kind(&key, &name) {
            // The longest name is the closest match.
            let len = key.0.len();
            if best.as_ref().is_none_or(|b| len > b.1) {
                best = Some((j, len, key, cut));
            }
        }
    }
    let (j, _, key, cut) = best?;
    // Google's `-edited` copy of the same photo doesn't count as another photo.
    let edited = format!("{}-edited", Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default());
    if cut && others.iter().any(|o| *o != name && !o.to_lowercase().starts_with(&edited) && match_kind(&key, o) == Some(true)) {
        return None;
    }
    Some(j)
}

/// Names of the files in `dir` that are not sidecars.
fn names_in(files: &[PathBuf]) -> Vec<String> {
    files
        .iter()
        .filter(|p| !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect()
}

/// `.json` files by folder.
pub fn by_folder(jsons: &[PathBuf]) -> HashMap<PathBuf, Vec<PathBuf>> {
    let mut out: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for j in jsons {
        out.entry(j.parent().map(Path::to_path_buf).unwrap_or_default()).or_default().push(j.clone());
    }
    out
}

/// Sidecars by the photo they belong to.
pub fn pair(photos: &[PathBuf], jsons: &[PathBuf]) -> HashMap<PathBuf, PathBuf> {
    let folders = by_folder(jsons);
    let siblings = by_folder(photos);
    photos
        .iter()
        .filter_map(|p| {
            let here = folders.get(p.parent()?)?;
            let others = siblings.get(p.parent()?).map(|v| names_in(v)).unwrap_or_default();
            find(p, here, &others).map(|j| (p.clone(), j.clone()))
        })
        .collect()
}

/// `p` without the folder or zip of its Takeout part: Google splits a large
/// Takeout into parts (`takeout-…-001`, `-002`, …), each holding a `Takeout`
/// folder, and a photo and its sidecar can land in different parts.
pub fn part_key(p: &Path) -> PathBuf {
    let comps: Vec<_> = p.components().collect();
    match comps.iter().position(|c| c.as_os_str().eq_ignore_ascii_case("Takeout")) {
        Some(i) if i > 0 => comps.iter().enumerate().filter(|(n, _)| *n != i - 1).map(|(_, c)| c.as_os_str()).collect(),
        _ => p.to_path_buf(),
    }
}

/// [`pair`] across the parts of a Takeout, on disk or inside zips.
pub fn pair_across(photos: &[PathBuf], jsons: &[PathBuf]) -> HashMap<PathBuf, PathBuf> {
    let mut json_at: HashMap<PathBuf, PathBuf> = HashMap::new();
    for j in jsons {
        json_at.entry(part_key(j)).or_insert_with(|| j.clone());
    }
    let photo_keys: Vec<PathBuf> = photos.iter().map(|p| part_key(p)).collect();
    let json_keys: Vec<PathBuf> = json_at.keys().cloned().collect();
    let found = pair(&photo_keys, &json_keys);
    photos.iter().zip(&photo_keys).filter_map(|(p, k)| found.get(k).and_then(|j| json_at.get(j)).map(|j| (p.clone(), j.clone()))).collect()
}

/// Whether a `.json` file belongs to a Google Takeout (a sidecar, or an
/// album's `metadata.json`), so it is passed over without a mention.
pub fn is_takeout_json(p: &Path) -> bool {
    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    p.components().any(|c| c.as_os_str().eq_ignore_ascii_case("Takeout")) || name.contains(".supplemental-") || name.contains(".supp")
}

/// Pixel motion photos come out of Google Takeout as a picture
/// (`PXL_….MP.jpg`) and its moving part as a file with no usual ending
/// (`PXL_….MP`, `….MP.COVER`). The picture is brought in; this says why the
/// rest is passed over.
pub fn motion_part(p: &Path) -> Option<&'static str> {
    let name = p.file_name()?.to_string_lossy().to_lowercase();
    (name.ends_with(".mp") || name.ends_with(".mp.cover")).then_some("The moving part of a Pixel motion photo; its picture is brought in.")
}

/// Finds sidecars on disk, reading each folder once.
#[derive(Default)]
pub struct Finder {
    /// Per folder: its sidecars, and the names of its other files.
    folders: HashMap<PathBuf, (Vec<PathBuf>, Vec<String>)>,
}

impl Finder {
    /// The sidecar beside `photo`, if there is one.
    pub fn beside(&mut self, photo: &Path) -> Option<PathBuf> {
        let dir = photo.parent()?.to_path_buf();
        let (jsons, others) = self.folders.entry(dir.clone()).or_insert_with(|| {
            let all: Vec<PathBuf> = std::fs::read_dir(&dir).map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect()).unwrap_or_default();
            let jsons = all.iter().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))).cloned().collect();
            (jsons, names_in(&all))
        });
        find(photo, jsons, others).cloned()
    }
}

/// Google's saved edit of a photo (`IMG_1234-edited.jpg`), when the original
/// is among `files`.
pub fn is_edited_copy(p: &Path, files: &std::collections::HashSet<PathBuf>) -> bool {
    let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else { return false };
    let lower = stem.to_lowercase();
    let Some(cut) = lower.rfind("-edited") else { return false };
    let rest = &lower[cut + "-edited".len()..];
    if !(rest.is_empty() || copy_number(rest).0.is_empty()) {
        return false;
    }
    let original = &stem[..cut];
    let ext = p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = p.parent().unwrap_or(Path::new(""));
    files.contains(&dir.join(format!("{original}.{ext}")))
        || files.iter().any(|f| f.parent() == Some(dir) && f.file_stem().is_some_and(|s| s.to_string_lossy() == original))
}

/// Distance in kilometres between two points (close enough for "is this the same place").
pub fn km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, lo1, la2, lo2) = (a.0.to_radians(), a.1.to_radians(), b.0.to_radians(), b.1.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2) + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(json: &str, photo: &str) -> bool {
        matches(&sidecar_key(json).unwrap(), photo)
    }

    #[test]
    fn names() {
        assert!(m("IMG_1234.jpg.json", "IMG_1234.jpg"));
        assert!(m("IMG_1234.jpg.supplemental-metadata.json", "IMG_1234.jpg"));
        assert!(m("IMG_1234.jpg.supplemental-me.json", "IMG_1234.jpg"));
        assert!(m("IMG_1234.jpg.s.json", "IMG_1234.jpg"));
        assert!(m("IMG_1234.jpg(1).json", "IMG_1234(1).jpg"));
        assert!(m("IMG_1234.jpg.supplemental-metadata(1).json", "IMG_1234(1).jpg"));
        assert!(!m("IMG_1234.jpg(1).json", "IMG_1234.jpg"));
        assert!(!m("IMG_1234.jpg.json", "IMG_1234(1).jpg"));
        assert!(!m("IMG_12.jpg.json", "IMG_1234.jpg"));
        assert!(m("Grandma and Grandpa at the lake in sum.json", "Grandma and Grandpa at the lake in summer 1972.jpg"));
        assert!(!m("short.json", "short and more.jpg"));
        assert!(m("IMG_1234.json", "IMG_1234.jpg"));
    }

    #[test]
    fn cut_short_name_fitting_two_photos_is_left_out() {
        let json = vec![PathBuf::from("d/Grandma and Grandpa at the lake in sum.json")];
        let a = "Grandma and Grandpa at the lake in summer 1972.jpg";
        let b = "Grandma and Grandpa at the lake in summer 1973.jpg";
        assert!(find(&Path::new("d").join(a), &json, &[a.into()]).is_some());
        assert!(find(&Path::new("d").join(a), &json, &[a.into(), b.into()]).is_none());
        let e = "Grandma and Grandpa at the lake in summer 1972-edited.jpg";
        assert!(find(&Path::new("d").join(a), &json, &[a.into(), e.into()]).is_some());
        // A full-name match is kept even beside a similar name.
        let json = vec![PathBuf::from("d/IMG_1234.jpg.json")];
        assert!(find(Path::new("d/IMG_1234.jpg"), &json, &["IMG_1234.jpg".into(), "IMG_1234.mov".into()]).is_some());
    }
}
