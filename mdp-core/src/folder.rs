//! The managed content folder.
//!
//! Working files are named by permanent ID only, in folders by type and then
//! the year of the content's own date, or `undated`. The readable dated names
//! of the export's file names are given in the export, not here.

use std::path::{Path, PathBuf};

use crate::Result;

pub const CONTENT_DIR: &str = "managed-content";
pub const SNAPSHOT_DIR: &str = "snapshots";
pub const CACHE_DIR: &str = "cache";

const PROJECT_README: &str = "\
This folder is a Modern Day Pioneer Journey.

Open it with the Modern Day Pioneer app. Please don't move, rename or edit the
files inside it; the app looks after them.

To get your pictures, recordings and stories as ordinary files to browse or
share, use Export in the app.

What is here:
  catalog.sqlite    the app's working catalog
  catalog.jsonl     a plain-text copy of the catalog (JSON-LD, one item per line)
  context.jsonld    what the names in catalog.jsonl mean
  managed-content/  the Journey's files, named by ID (managed by the app)
  snapshots/        daily copies of the catalog, kept for 30 days
  cache/            rebuilt by the app; safe to delete
";

const CONTENT_README: &str = "\
These files are managed by the Modern Day Pioneer app.

They are named by ID, not by title, and are marked read-only. Please don't
move, rename or edit them here. Use Export in the app to get readable copies.
";

/// Make the folders and read-me files of a new Journey.
pub fn prepare(root: &Path) -> Result<()> {
    for d in [CONTENT_DIR, SNAPSHOT_DIR, CACHE_DIR] {
        std::fs::create_dir_all(root.join(d))?;
    }
    write_if_missing(&root.join("READ-ME.txt"), PROJECT_README)?;
    write_if_missing(&root.join(CONTENT_DIR).join("READ-ME.txt"), CONTENT_README)?;
    Ok(())
}

fn write_if_missing(path: &Path, text: &str) -> Result<()> {
    if !path.exists() {
        std::fs::write(path, text.replace('\n', "\r\n"))?;
    }
    Ok(())
}

/// The folder for each kind of content.
pub fn type_folder(kind: &str) -> &'static str {
    match kind {
        "picture" => "photos",
        "voice recording" => "audio",
        "video" => "video",
        _ => "text",
    }
}

/// The year folder from a content date (ISO or EDTF), or `undated`.
pub fn year_folder(date: Option<&str>) -> String {
    let d = date.unwrap_or("").trim();
    let y: String = d.chars().take(4).collect();
    if y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) && d.chars().nth(4).map_or(true, |c| !c.is_ascii_digit()) {
        y
    } else {
        "undated".into()
    }
}

/// The working path, relative to the Journey folder, with `/` separators so
/// it reads the same on every system: `managed-content/photos/1962/k7f3m2qa.jpg`.
pub fn working_path(kind: &str, id: &str, date: Option<&str>, ext: &str) -> String {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let name = if ext.is_empty() { id.to_string() } else { format!("{id}.{ext}") };
    format!("{CONTENT_DIR}/{}/{}/{name}", type_folder(kind), year_folder(date))
}

/// The full path of a working path inside the Journey folder. A path from a
/// catalog is never trusted to stay inside: empty, `.` and `..` parts are
/// dropped and drive or folder marks inside a part are made plain, so even a
/// damaged or hand-made catalog can only point at files inside `root`.
pub fn absolute(root: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .fold(root.to_path_buf(), |p, part| p.join(part.replace([':', '\\', '\0'], "_")))
}

fn is_plain_part(part: &str) -> bool {
    !part.is_empty() && part != "." && part != ".." && !part.contains([':', '\\', '\0'])
}

/// Whether `rel` is a plain relative path with `/` separators that stays
/// inside the folder it starts from: no drive, no `..`, no `\\`.
pub fn is_inside(rel: &str) -> bool {
    rel.split('/').all(is_plain_part)
}

/// Whether `rel` is a working path the app could have made: inside
/// `managed-content/` and [`is_inside`].
pub fn is_working_path(rel: &str) -> bool {
    rel.strip_prefix(&format!("{CONTENT_DIR}/")).is_some_and(is_inside)
}

/// A warning when `path` is inside a folder synced by a cloud service.
pub fn cloud_sync_warning(path: &Path) -> Option<String> {
    let text = path.to_string_lossy().replace('\\', "/").to_lowercase();
    let mut roots: Vec<String> = ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .map(|p| p.replace('\\', "/").to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    roots.sort();
    let in_env_root = roots.iter().any(|r| text.starts_with(r.trim_end_matches('/')));
    const NAMES: &[(&str, &str)] = &[
        ("onedrive", "OneDrive"),
        ("dropbox", "Dropbox"),
        ("google drive", "Google Drive"),
        ("googledrive", "Google Drive"),
        ("my drive", "Google Drive"),
        ("icloud drive", "iCloud"),
        ("iclouddrive", "iCloud"),
        ("box", "Box"),
    ];
    let service = text
        .split('/')
        .find_map(|part| NAMES.iter().find(|(n, _)| part == *n || part.starts_with(&format!("{n} -"))).map(|(_, s)| *s))
        .or(if in_env_root { Some("OneDrive") } else { None })?;
    Some(format!(
        "This folder is synced by {service}. Syncing can move or lock the Journey's files while you work, \
         so a folder outside {service} is safer. You can still use it if you prefer."
    ))
}
