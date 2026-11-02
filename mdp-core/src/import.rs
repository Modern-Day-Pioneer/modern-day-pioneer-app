//! Import pictures, recordings, videos and text from the device
//!.
//!
//! Originals are only ever opened for reading: never moved, renamed, edited
//! or deleted. An import runs in two parts so the app stays usable:
//!
//! 1. [`prepare`] (a background job, no catalog changes): each file is copied
//!    into a holding folder while its SHA-256 is computed, the copy is read
//!    back and checked against that checksum, and its details (date, size,
//!    GPS, camera) are read from the copy. Files already in the Journey are
//!    set aside as duplicates. Canceling removes the holding folder, so
//!    nothing is left behind.
//! 2. [`Journey::finish_import`] (quick): one action records every prepared
//!    file and moves it from the holding folder to its working place, marked
//!    read-only. If the action fails, the moved copies are removed again.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::folder::{self, CONTENT_DIR};
use crate::jobs::{JobContext, Skipped};
use crate::journey::CATALOG_FILE;
use crate::{Error, Journey, Result, Row};

/// The holding folder inside `managed-content`.
pub const HOLDING_DIR: &str = "holding";

/// Kind and media type for a file extension, or `None` when the app does not
/// import that kind of file.
pub fn classify(ext: &str) -> Option<(&'static str, &'static str)> {
    Some(match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => ("picture", "image/jpeg"),
        "png" => ("picture", "image/png"),
        "gif" => ("picture", "image/gif"),
        "heic" => ("picture", "image/heic"),
        "heif" => ("picture", "image/heif"),
        "avif" => ("picture", "image/avif"),
        "bmp" => ("picture", "image/bmp"),
        "dng" => ("picture", "image/x-adobe-dng"),
        "tif" | "tiff" => ("picture", "image/tiff"),
        "webp" => ("picture", "image/webp"),
        "wav" => ("voice recording", "audio/wav"),
        "flac" => ("voice recording", "audio/flac"),
        "mp3" => ("voice recording", "audio/mpeg"),
        "m4a" => ("voice recording", "audio/mp4"),
        "opus" => ("voice recording", "audio/opus"),
        "ogg" | "oga" => ("voice recording", "audio/ogg"),
        "mp4" | "m4v" => ("video", "video/mp4"),
        "mov" => ("video", "video/quicktime"),
        "txt" => ("text", "text/plain"),
        "pdf" => ("text", crate::documents::PDF),
        "docx" => ("text", crate::documents::DOCX),
        "doc" => ("text", crate::documents::DOC),
        "odt" => ("text", crate::documents::ODT),
        "rtf" => ("text", crate::documents::RTF),
        _ => return None,
    })
}

/// One file prepared in the holding folder, not yet in the catalog.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub holding: PathBuf,
    pub kind: &'static str,
    pub format: String,
    pub ext: String,
    pub sha256: String,
    pub size: u64,
    pub original_name: String,
    pub source_folder: String,
    /// The original's full path, so a plan can find its row again (sprint 11).
    pub source: PathBuf,
    pub details: Details,
}

/// What could be read from the file itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Details {
    /// `YYYY-MM-DD`, from the picture's EXIF date taken.
    pub date: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// Make and model as one line, for example "Google Pixel 11 Pro XL".
    pub camera: Option<String>,
    /// Length of a recording or video, ISO 8601 (`PT3M20S`).
    pub duration: Option<String>,
}

/// A file that is already in the Journey (same checksum), so it is not copied twice.
#[derive(Debug, Clone, PartialEq)]
pub struct Duplicate {
    pub original: String,
    /// The content it matches, or `None` when it matches another file of the same import.
    pub existing_id: Option<String>,
}

/// Part 1's result. Dropping it without finishing removes the holding folder.
pub struct Batch {
    dir: PathBuf,
    pub items: Vec<Prepared>,
    pub duplicates: Vec<Duplicate>,
}

impl Batch {
    /// An empty batch with its own holding folder, for imports that copy
    /// files themselves (an exported Journey, sprint 11).
    pub(crate) fn new(journey_root: &Path) -> Result<Batch> {
        let dir = journey_root.join(CONTENT_DIR).join(HOLDING_DIR).join(crate::ids::new_id());
        std::fs::create_dir_all(&dir)?;
        Ok(Batch { dir, items: Vec::new(), duplicates: Vec::new() })
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        clear_readonly_tree(&self.dir);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImportReport {
    /// IDs of the new content, in the order the files were found.
    pub imported: Vec<String>,
    pub duplicates: Vec<Duplicate>,
}

/// Every importable file under `paths` (files, or folders searched through
/// their subfolders), in a stable order. Other files are reported as skipped;
/// hidden and system files are passed over without a mention.
pub fn gather(paths: &[PathBuf], journey_root: &Path, ctx: &JobContext) -> Result<Vec<PathBuf>> {
    let (files, jsons) = gather_with_json(paths, journey_root, ctx)?;
    for p in jsons.into_iter().filter(|p| !crate::takeout::is_takeout_json(p)) {
        ctx.skip(p.display().to_string(), "Not a picture, recording, video, text or document file the app can bring in.");
    }
    Ok(files)
}

/// [`gather`], with `.json` files handed back instead of reported as
/// skipped, so Google Takeout sidecars can be read (sprint 12).
pub fn gather_with_json(paths: &[PathBuf], journey_root: &Path, ctx: &JobContext) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    gather_all(paths, journey_root, ctx, false)
}

/// [`gather_with_json`]; with `zips_too`, `.zip` files are handed back with
/// the `.json` files instead of reported as skipped (a folder import reads them).
pub fn gather_all(paths: &[PathBuf], journey_root: &Path, ctx: &JobContext, zips_too: bool) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut jsons = Vec::new();
    let root = journey_root.canonicalize().unwrap_or_else(|_| journey_root.to_path_buf());
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = paths.iter().rev().cloned().collect();
    while let Some(p) = stack.pop() {
        ctx.check()?;
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') || ["thumbs.db", "desktop.ini"].contains(&name.to_ascii_lowercase().as_str()) {
            continue;
        }
        if p.canonicalize().map(|c| c.starts_with(&root)).unwrap_or(false) {
            ctx.skip(p.display().to_string(), "It is inside this Journey's own folder.");
            continue;
        }
        if p.is_dir() {
            let mut children: Vec<PathBuf> = match std::fs::read_dir(&p) {
                Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
                Err(e) => {
                    ctx.skip(p.display().to_string(), format!("The folder could not be read ({e})."));
                    continue;
                }
            };
            children.sort();
            stack.extend(children.into_iter().rev());
        } else if p.is_file() {
            let ext = p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
            if classify(&ext).is_some() {
                out.push(p);
            } else if ext.eq_ignore_ascii_case("json") || (zips_too && crate::takeout_zip::is_zip(&p)) {
                jsons.push(p);
            } else if crate::takeout::motion_part(&p).is_some() {
                // Passed over quietly: its picture (….MP.jpg) is beside it.
            } else if crate::takeout_zip::is_zip(&p) {
                ctx.skip(p.display().to_string(), "A zip file: to bring in what is inside, use \u{201c}Bring in a folder\u{201d} and choose the folder holding it.");
            } else {
                ctx.skip(p.display().to_string(), "Not a picture, recording, video, text or document file the app can bring in.");
            }
        } else if crate::takeout_zip::split(&p).is_some() && classify(&p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()).is_some() {
            // A file inside a zip, named by a folder import's plan.
            out.push(p);
        } else {
            ctx.skip(p.display().to_string(), "Not found.");
        }
    }
    Ok((out, jsons))
}

/// Part 1: copy the files into the holding folder and check them. Run it as
/// a background job; it does not change the catalog.
pub fn prepare(journey_root: &Path, paths: &[PathBuf], ctx: &JobContext) -> Result<Batch> {
    ctx.message("Looking for pictures, recordings, videos and text…");
    let files = gather(paths, journey_root, ctx)?;
    let dir = journey_root.join(CONTENT_DIR).join(HOLDING_DIR).join(crate::ids::new_id());
    std::fs::create_dir_all(&dir)?;
    let mut batch = Batch { dir, items: Vec::new(), duplicates: Vec::new() };
    let catalog = Connection::open_with_flags(
        journey_root.join(CATALOG_FILE),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut seen = HashSet::new();
    ctx.set_total(files.len() as u64 + ctx.progress().done);
    for (n, src) in files.iter().enumerate() {
        ctx.check()?;
        let shown = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let label = format!("Copying {shown} ({} of {})", n + 1, files.len());
        match prepare_one(&batch.dir, src, &catalog) {
            Ok(p) => {
                if let Some(existing) = existing_content(&catalog, &p.sha256)? {
                    std::fs::remove_file(&p.holding)?;
                    batch.duplicates.push(Duplicate { original: src.display().to_string(), existing_id: Some(existing) });
                } else if !seen.insert(p.sha256.clone()) {
                    std::fs::remove_file(&p.holding)?;
                    batch.duplicates.push(Duplicate { original: src.display().to_string(), existing_id: None });
                } else {
                    batch.items.push(p);
                }
                ctx.advance(label);
            }
            Err(Error::Canceled) => return Err(Error::Canceled),
            Err(e) => ctx.skip(src.display().to_string(), plain_reason(&e)),
        }
    }
    Ok(batch)
}

pub(crate) fn plain_reason(e: &Error) -> String {
    match e {
        Error::Io(io) => format!("The file could not be copied ({io})."),
        other => other.to_string(),
    }
}

pub(crate) fn existing_content(conn: &Connection, sha: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT content_id FROM content_file WHERE sha256 = ?1 ORDER BY is_copy LIMIT 1", [sha], |r| r.get(0))
        .optional()?)
}

pub(crate) fn prepare_one(dir: &Path, src: &Path, _catalog: &Connection) -> Result<Prepared> {
    let ext = src.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let (kind, format) = classify(&ext).ok_or_else(|| Error::Invalid("Not a file the app can bring in.".into()))?;
    let holding = dir.join(format!("{}.{ext}", crate::ids::new_id()));
    let (sha256, size) = copy_with_checksum(src, &holding)?;
    let (check, _) = checksum(&holding)?;
    if check != sha256 {
        let _ = std::fs::remove_file(&holding);
        return Err(Error::Invalid("The copy did not match the original, so it was not kept.".into()));
    }
    let mut format = format.to_string();
    let details = match kind {
        "picture" => picture_details(&holding),
        "video" | "voice recording" => match crate::convert::find_tool() {
            Some(tool) => {
                let info = crate::convert::probe(&tool, &holding);
                if format == "video/mp4" && info.video_codec.as_deref() == Some("hevc") {
                    format = "video/mp4; codecs=\"hevc\"".into();
                }
                Details {
                    date: info.date,
                    width: if kind == "video" { info.width } else { None },
                    height: if kind == "video" { info.height } else { None },
                    duration: info.seconds.map(crate::convert::iso_duration),
                    ..Details::default()
                }
            }
            None => Details::default(),
        },
        _ => Details::default(),
    };
    Ok(Prepared {
        holding,
        kind,
        format,
        ext,
        sha256,
        size,
        original_name: src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        source_folder: src.parent().map(|p| p.display().to_string()).unwrap_or_default(),
        source: src.to_path_buf(),
        details,
    })
}

/// Copy `src` to `dest`, opening `src` for reading only. Returns the SHA-256
/// and size. `src` can be a file inside a zip (see [`crate::takeout_zip`]).
pub fn copy_with_checksum(src: &Path, dest: &Path) -> Result<(String, u64)> {
    crate::takeout_zip::read_with(src, |input| copy_from(input, dest))
}

fn copy_from(input: &mut dyn Read, dest: &Path) -> Result<(String, u64)> {
    let file = File::create(dest)?;
    let mut out = BufWriter::with_capacity(1 << 20, &file);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        size += n as u64;
    }
    out.flush()?;
    drop(out);
    file.sync_all()?;
    Ok((hex(&hasher.finalize()), size))
}

/// SHA-256 and size of a file.
pub fn checksum(path: &Path) -> Result<(String, u64)> {
    let mut input = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hex(&hasher.finalize()), size))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Date taken, size, GPS and camera from a picture. Anything missing or
/// unreadable is simply left empty.
pub fn picture_details(path: &Path) -> Details {
    let mut d = Details::default();
    if let Ok(s) = imagesize::size(path) {
        d.width = u32::try_from(s.width).ok();
        d.height = u32::try_from(s.height).ok();
    }
    let Ok(file) = File::open(path) else { return d };
    let Ok(exif) = exif::Reader::new().read_from_container(&mut BufReader::new(file)) else { return d };
    let text = |tag| exif.get_field(tag, exif::In::PRIMARY).map(|f| f.display_value().to_string());
    let ascii = |tag| {
        exif.get_field(tag, exif::In::PRIMARY).and_then(|f| match &f.value {
            exif::Value::Ascii(v) => v.first().map(|s| String::from_utf8_lossy(s).trim().trim_matches('\0').to_string()),
            _ => None,
        })
    };
    // The camera's own turn flag (5 to 8 stand the picture on its side): keep the size as it is seen (sprint 8).
    if exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY).and_then(|f| f.value.get_uint(0)).is_some_and(|o| (5..=8).contains(&o)) {
        std::mem::swap(&mut d.width, &mut d.height);
    }
    for tag in [exif::Tag::DateTimeOriginal, exif::Tag::DateTimeDigitized, exif::Tag::DateTime] {
        if let Some(date) = ascii(tag).and_then(|s| exif_date(&s)) {
            d.date = Some(date);
            break;
        }
    }
    let coord = |tag, ref_tag, neg: &str| -> Option<f64> {
        let f = exif.get_field(tag, exif::In::PRIMARY)?;
        let exif::Value::Rational(v) = &f.value else { return None };
        if v.len() < 3 || v.iter().any(|r| r.denom == 0) {
            return None;
        }
        let deg = v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
        let sign = if text(ref_tag).map(|r| r.to_ascii_uppercase().starts_with(neg)).unwrap_or(false) { -1.0 } else { 1.0 };
        Some((deg * sign * 1e6).round() / 1e6)
    };
    let lat = coord(exif::Tag::GPSLatitude, exif::Tag::GPSLatitudeRef, "S");
    let lon = coord(exif::Tag::GPSLongitude, exif::Tag::GPSLongitudeRef, "W");
    if let (Some(lat), Some(lon)) = (lat, lon) {
        if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && !(lat == 0.0 && lon == 0.0) {
            d.latitude = Some(lat);
            d.longitude = Some(lon);
        }
    }
    let make = ascii(exif::Tag::Make).unwrap_or_default();
    let model = ascii(exif::Tag::Model).unwrap_or_default();
    let camera = if !make.is_empty() && model.to_lowercase().starts_with(&make.to_lowercase()) {
        model
    } else {
        format!("{make} {model}").trim().to_string()
    };
    if !camera.is_empty() {
        d.camera = Some(camera);
    }
    d
}

/// `2014:06:02 10:31:00` to `2014-06-02`; blank or zero dates give `None`.
fn exif_date(s: &str) -> Option<String> {
    let s = s.trim();
    if s.len() < 10 {
        return None;
    }
    let (y, m, d) = (&s[0..4], &s[5..7], &s[8..10]);
    let ok = |p: &str| p.chars().all(|c| c.is_ascii_digit());
    if !(ok(y) && ok(m) && ok(d)) || y == "0000" || m == "00" || d == "00" {
        return None;
    }
    Some(format!("{y}-{m}-{d}"))
}

pub(crate) fn set_readonly(path: &Path, readonly: bool) -> Result<()> {
    let mut perm = std::fs::metadata(path)?.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(readonly);
    std::fs::set_permissions(path, perm)?;
    Ok(())
}

pub(crate) fn clear_readonly_tree(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                clear_readonly_tree(&p);
            } else {
                let _ = set_readonly(&p, false);
            }
        }
    }
}

pub(crate) fn row(v: Value) -> Row {
    v.as_object().cloned().unwrap_or_default()
}

/// Record one prepared file as content (under `id` when given, else a new
/// ID) and move it from the holding folder to its working place, read-only.
/// Moved files are listed in `moved` so a failed action can remove them.
pub(crate) fn record_prepared(a: &mut crate::history::Action, root: &Path, author: &str, p: &Prepared, id: Option<&str>, moved: &mut Vec<PathBuf>) -> Result<String> {
    let d = &p.details;
    let fields = row(json!({
        "kind": p.kind, "name": p.original_name, "date": d.date,
        "source_note": p.source_folder, "added_by": author,
    }));
    let id = match id {
        Some(id) => {
            a.create_with_id("content", id, fields)?;
            id.to_string()
        }
        None => a.create("content", fields)?,
    };
    let ext_row = match p.kind {
        "picture" => ("picture", json!({"id": id, "width": d.width, "height": d.height,
            "latitude": d.latitude, "longitude": d.longitude, "camera": d.camera})),
        "video" => ("video", json!({"id": id, "width": d.width, "height": d.height, "duration": d.duration,
            "latitude": d.latitude, "longitude": d.longitude, "camera": d.camera})),
        "voice recording" => ("voice_recording", json!({"id": id, "duration": d.duration})),
        _ => ("text_content", json!({"id": id})),
    };
    let mut r = row(ext_row.1);
    r.retain(|_, v| !v.is_null());
    a.insert(ext_row.0, r)?;
    let rel = folder::working_path(p.kind, &id, d.date.as_deref(), &p.ext);
    let dest = folder::absolute(root, &rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&p.holding, &dest)?;
    moved.push(dest.clone());
    set_readonly(&dest, true)?;
    a.insert("content_file", row(json!({
        "content_id": id, "is_copy": false, "path": rel, "format": p.format,
        "size": p.size, "sha256": p.sha256, "original_name": p.original_name,
    })))?;
    Ok(id)
}

impl Journey {
    /// Part 2: record a prepared batch as one action and move each file to
    /// its working place, read-only.
    pub fn finish_import(&mut self, mut batch: Batch) -> Result<ImportReport> {
        let items = std::mem::take(&mut batch.items);
        let mut report = ImportReport { imported: Vec::new(), duplicates: std::mem::take(&mut batch.duplicates) };
        if items.is_empty() {
            return Ok(report);
        }
        let root = self.root().to_path_buf();
        let author = self.author().to_string();
        let mut moved: Vec<PathBuf> = Vec::new();
        let result = self.act(|a| {
            let mut ids = Vec::new();
            let mut dups = Vec::new();
            for p in &items {
                if let Some(existing) = existing_content(a.conn(), &p.sha256)? {
                    dups.push(Duplicate { original: p.source.display().to_string(), existing_id: Some(existing) });
                    continue;
                }
                ids.push(record_prepared(a, &root, &author, p, None, &mut moved)?);
            }
            Ok((ids, dups))
        });
        match result {
            Ok((ids, dups)) => {
                report.imported = ids;
                report.duplicates.extend(dups);
                Ok(report)
            }
            Err(e) => {
                for m in moved {
                    let _ = set_readonly(&m, false);
                    let _ = std::fs::remove_file(&m);
                }
                Err(e)
            }
        }
    }

    /// Both parts in one call, for tests and the command-line tool.
    pub fn import(&mut self, paths: &[PathBuf], ctx: &JobContext) -> Result<(ImportReport, Vec<Skipped>)> {
        let batch = prepare(self.root(), paths, ctx)?;
        let report = self.finish_import(batch)?;
        Ok((report, ctx.skipped()))
    }
}
