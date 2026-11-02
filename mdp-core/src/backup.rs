//! Back up a whole Journey to one archive file and restore it as a new
//! project. No screens until sprint 7.
//!
//! The archive is a plain .zip, so it opens on any system without the app.
//! Pictures and media are stored uncompressed (they are already compressed),
//! text is deflated. `backup-manifest.json` lists every file with its size
//! and SHA-256, and restore checks each one before the project is put in place.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use crate::folder::{CACHE_DIR, CONTENT_DIR, SNAPSHOT_DIR};
use crate::jobs::{JobContext, Staging};
use crate::journey::CATALOG_FILE;
use crate::{Error, Result};

pub const MANIFEST: &str = "backup-manifest.json";

/// Text files worth compressing; everything else is stored as is.
const COMPRESS: &[&str] = &["sqlite", "jsonl", "jsonld", "json", "txt", "vtt"];

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A file name for a backup of a Journey made now, with the date and time so
/// no two backups share a name: `Rowans_2026-10-16_14-05-09.mdp-backup.zip`.
pub fn suggested_name(journey_name: &str) -> String {
    let safe: String = journey_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect::<String>()
        .trim()
        .to_string();
    let safe = if safe.is_empty() { "Journey".into() } else { safe };
    format!("{safe}_{}.mdp-backup.zip", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"))
}

/// Files that go into a backup, relative to the Journey folder with `/`.
/// The catalog is added separately from a consistent copy; snapshots, the
/// cache and removed copies are left out (the cache is rebuilt, snapshots are
/// local history, removed copies are gone once the app closes).
fn project_files(root: &Path) -> Result<Vec<String>> {
    let mut out = vec![];
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<std::result::Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            let top = rel.split('/').next().unwrap_or("");
            let removed = format!("{CONTENT_DIR}/{}", crate::files::REMOVED_DIR);
            // Removed copies wait in content/removed only for Ctrl+Z, so a backup leaves them out.
            if [SNAPSHOT_DIR, CACHE_DIR].contains(&top) || rel == removed.as_str() || rel.starts_with(CATALOG_FILE) || rel.contains(".partial-") || rel.ends_with(".tmp") {
                continue;
            }
            if p.is_dir() {
                walk(root, &p, out)?;
            } else {
                out.push(rel);
            }
        }
        Ok(())
    }
    walk(root, root, &mut out)?;
    Ok(out)
}

/// Back up the Journey at `root` into `dest` (which must not exist yet).
/// Meant to run inside a background job.
pub fn backup(root: &Path, dest: &Path, ctx: &JobContext) -> Result<PathBuf> {
    if !root.join(CATALOG_FILE).is_file() {
        return Err(Error::Invalid(format!("No Journey found in {}.", root.display())));
    }
    let staging = Staging::new(dest)?;
    // A consistent copy of the catalog, even while the app is open.
    let catalog_copy = Staging::new(&dest.with_extension("catalog.sqlite"))?;
    {
        let conn = Connection::open(root.join(CATALOG_FILE))?;
        crate::snapshot::take(&conn, catalog_copy.path())?;
    }
    let files = project_files(root)?;
    ctx.set_total(files.len() as u64 + 2);

    let mut zip = zip::ZipWriter::new(std::fs::File::create(staging.path())?);
    let mut manifest = vec![];
    let mut add = |zip: &mut zip::ZipWriter<std::fs::File>, name: &str, src: &Path| -> Result<()> {
        ctx.check()?;
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let method = if COMPRESS.contains(&ext.as_str()) { CompressionMethod::Deflated } else { CompressionMethod::Stored };
        let size = std::fs::metadata(src)?.len();
        let opts = SimpleFileOptions::default().compression_method(method).large_file(size >= u32::MAX as u64);
        zip.start_file(name, opts).map_err(zip_err)?;
        let mut f = std::fs::File::open(src)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            zip.write_all(&buf[..n])?;
        }
        manifest.push(json!({"path": name, "size": size, "sha256": hex(&h.finalize())}));
        ctx.advance(format!("Backed up {name}"));
        Ok(())
    };
    add(&mut zip, CATALOG_FILE, catalog_copy.path())?;
    for rel in &files {
        add(&mut zip, rel, &crate::folder::absolute(root, rel))?;
    }
    ctx.check()?;
    let journey: (String, String) = Connection::open(catalog_copy.path())?
        .query_row("SELECT id, name FROM journey LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let doc = json!({
        "app": "Modern Day Pioneer",
        "backupFormat": 1,
        "formatVersion": crate::format_version_text(),
        "journeyId": journey.0,
        "journeyName": journey.1,
        "created": crate::journey::now(),
        "files": manifest,
    });
    zip.start_file(MANIFEST, SimpleFileOptions::default()).map_err(zip_err)?;
    zip.write_all(serde_json::to_string_pretty(&doc).unwrap().as_bytes())?;
    zip.finish().map_err(zip_err)?.sync_all()?;
    ctx.advance("Finished the backup");
    drop(catalog_copy);
    staging.commit()
}

/// Restore a backup into `dest`, a new folder (it must not exist, or be empty).
/// Every file is checked against the manifest before the folder is put in
/// place, so a damaged archive leaves nothing behind.
pub fn restore(archive: &Path, dest: &Path, ctx: &JobContext) -> Result<PathBuf> {
    if dest.is_dir() && std::fs::read_dir(dest)?.next().is_some() {
        return Err(Error::Invalid(format!("{} is not empty.", dest.display())));
    }
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?).map_err(zip_err)?;
    let manifest: Value = {
        let mut f = zip.by_name(MANIFEST).map_err(|_| Error::Invalid("This file is not a Modern Day Pioneer backup.".into()))?;
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        serde_json::from_str(&s).map_err(|e| Error::Invalid(format!("The backup's list of files is damaged: {e}")))?
    };
    crate::check_not_newer(manifest["formatVersion"].as_str(), "backup")?;
    let files = manifest["files"].as_array().cloned().unwrap_or_default();
    ctx.set_total(files.len() as u64);
    let staging = Staging::new_or_empty(dest)?;
    std::fs::create_dir_all(staging.path())?;
    for entry in &files {
        ctx.check()?;
        let name = entry["path"].as_str().unwrap_or_default();
        let expect = entry["sha256"].as_str().unwrap_or_default();
        let mut f = zip.by_name(name).map_err(|_| Error::Invalid(format!("The backup is missing {name}.")))?;
        let rel = f.enclosed_name().ok_or_else(|| Error::Invalid(format!("Unsafe name in backup: {name}")))?;
        let out_path = staging.path().join(rel);
        if let Some(p) = out_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut out = std::fs::File::create(&out_path)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            out.write_all(&buf[..n])?;
        }
        out.sync_all()?;
        if hex(&h.finalize()) != expect {
            return Err(Error::Invalid(format!("{name} in the backup does not match its checksum.")));
        }
        ctx.advance(format!("Restored {name}"));
    }
    for d in [CONTENT_DIR, SNAPSHOT_DIR, CACHE_DIR] {
        std::fs::create_dir_all(staging.path().join(d))?;
    }
    // The Journey's own copies are read-only again once every file has checked out.
    for entry in &files {
        let name = entry["path"].as_str().unwrap_or_default();
        if name.starts_with(&format!("{CONTENT_DIR}/")) && !name.ends_with("READ-ME.txt") {
            crate::import::set_readonly(&crate::folder::absolute(staging.path(), name), true)?;
        }
    }
    staging.commit()
}

/// The Journey's name and when the backup was made, read from its manifest,
/// so restore can suggest a folder and say what it is about to restore.
pub fn describe(archive: &Path) -> Result<(String, String)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?).map_err(zip_err)?;
    let mut f = zip.by_name(MANIFEST).map_err(|_| Error::Invalid("This file is not a Modern Day Pioneer backup.".into()))?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    let m: Value = serde_json::from_str(&s).map_err(|e| Error::Invalid(format!("The backup's list of files is damaged: {e}")))?;
    Ok((m["journeyName"].as_str().unwrap_or("Journey").to_string(), m["created"].as_str().unwrap_or_default().to_string()))
}

fn zip_err(e: zip::result::ZipError) -> Error {
    Error::Invalid(format!("Archive: {e}"))
}
