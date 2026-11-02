//! Zip files in a folder import (Google Takeout comes as several zips).
//!
//! The zips are read where they are and never unpacked to disk: a file
//! inside one is named by the zip's path followed by its path inside, as if
//! the zip were a folder (`D:\Downloads\takeout-001.zip\Takeout\Google
//! Photos\Trip\IMG_1234.jpg`). Copying one into the Journey streams it
//! straight from the zip, so a Takeout needs no extra free space.
//!
//! Google splits a large Takeout into parts and may put a photo in one part
//! and its sidecar in another, at the same path inside. Sidecars are paired
//! across all the zips in a folder by that inside path.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use crate::{Error, Result};

type Archive = ZipArchive<BufReader<File>>;

thread_local! {
    /// The zip read last: opening one reads its whole list of files, so a
    /// run of files from the same zip opens it once.
    static OPEN: RefCell<Option<(PathBuf, Archive)>> = const { RefCell::new(None) };
}

fn with_zip<T>(zip: &Path, f: impl FnOnce(&mut Archive) -> Result<T>) -> Result<T> {
    OPEN.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().is_none_or(|(p, _)| p != zip) {
            let archive = ZipArchive::new(BufReader::new(File::open(zip)?)).map_err(|e| Error::Invalid(format!("The zip file {} could not be read ({e}).", zip.display())))?;
            *slot = Some((zip.to_path_buf(), archive));
        }
        f(&mut slot.as_mut().expect("just opened").1)
    })
}

pub fn is_zip(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// The zip and the path inside it, when `p` names a file inside a zip.
pub fn split(p: &Path) -> Option<(PathBuf, String)> {
    let mut inside: Vec<String> = Vec::new();
    let mut cur = p;
    while let Some(parent) = cur.parent() {
        inside.push(cur.file_name()?.to_string_lossy().into_owned());
        if is_zip(parent) && parent.is_file() {
            inside.reverse();
            return Some((parent.to_path_buf(), inside.join("/")));
        }
        cur = parent;
    }
    None
}

/// One file inside a zip.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The zip's path, then the path inside it.
    pub path: PathBuf,
    /// The path inside the zip, the same in every part of a Takeout.
    pub inside: String,
    pub size: u64,
}

/// Every file inside `zip` (folders and unsafe names left out).
pub fn list(zip: &Path) -> Result<Vec<Entry>> {
    with_zip(zip, |a| {
        let mut out = Vec::new();
        for i in 0..a.len() {
            let f = a.by_index_raw(i).map_err(|e| Error::Invalid(format!("The zip file {} could not be read ({e}).", zip.display())))?;
            if f.is_dir() {
                continue;
            }
            let Ok(name) = f.name() else { continue };
            let name = name.replace('\\', "/");
            let parts: Vec<&str> = name.split('/').filter(|s| !s.is_empty()).collect();
            if parts.is_empty() || parts.iter().any(|s| *s == ".." || *s == "." || s.contains(':')) {
                continue;
            }
            let mut path = zip.to_path_buf();
            for s in &parts {
                path.push(s);
            }
            out.push(Entry { path, inside: parts.join("/"), size: f.size() });
        }
        Ok(out)
    })
}

/// Read a small file from inside a zip (a sidecar); `None` past `limit` bytes.
pub fn read_small(p: &Path, limit: u64) -> Option<Vec<u8>> {
    let (zip, inside) = split(p)?;
    with_zip(&zip, |a| {
        let f = a.by_name(&inside).map_err(|e| Error::Invalid(e.to_string()))?;
        if f.size() > limit {
            return Err(Error::Invalid("too large".into()));
        }
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.take(limit).read_to_end(&mut buf)?;
        Ok(buf)
    })
    .ok()
}

/// The size of a file, inside a zip or not.
pub fn size(p: &Path) -> Option<u64> {
    if let Ok(m) = std::fs::metadata(p) {
        return Some(m.len());
    }
    let (zip, inside) = split(p)?;
    with_zip(&zip, |a| Ok(a.by_name(&inside).map_err(|e| Error::Invalid(e.to_string()))?.size())).ok()
}

/// Open a file for reading, inside a zip or not, and hand it to `f`.
pub fn read_with<T>(p: &Path, f: impl FnOnce(&mut dyn Read) -> Result<T>) -> Result<T> {
    match split(p) {
        Some((zip, inside)) => with_zip(&zip, |a| {
            let mut file = a.by_name(&inside).map_err(|e| Error::Invalid(format!("{inside} could not be read from {} ({e}).", zip.display())))?;
            f(&mut file)
        }),
        None => f(&mut BufReader::with_capacity(1 << 20, File::open(p)?)),
    }
}

/// Sidecars for files inside zips, paired across all the parts of a Takeout.
pub fn pair(media: &[Entry], jsons: &[Entry]) -> HashMap<PathBuf, PathBuf> {
    let m: Vec<PathBuf> = media.iter().map(|e| e.path.clone()).collect();
    let j: Vec<PathBuf> = jsons.iter().map(|e| e.path.clone()).collect();
    crate::takeout::pair_across(&m, &j)
}

/// The Takeout a part belongs to: `takeout-20261101T214823Z-1-001.zip` is
/// part 001 of `takeout-20261101T214823Z-1`. Any other zip is its own.
fn takeout_of(zip: &Path) -> String {
    let stem = zip.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    match stem.rsplit_once('-') {
        Some((base, part)) if !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()) => base.to_string(),
        _ => stem,
    }
}

/// The sidecar for each of `sources` that are inside zips. The other parts
/// of the same Takeouts (zips beside them with the same name but the part
/// number) are looked through once; other zips in the folder are not.
pub fn sidecars_for(sources: &[PathBuf]) -> HashMap<PathBuf, PathBuf> {
    let mut wanted: Vec<(PathBuf, String)> = sources.iter().filter_map(|s| split(s)).filter_map(|(z, _)| Some((z.parent()?.to_path_buf(), takeout_of(&z)))).collect();
    wanted.sort();
    wanted.dedup();
    let mut folders: Vec<PathBuf> = wanted.iter().map(|(d, _)| d.clone()).collect();
    folders.dedup();
    let (mut media, mut jsons) = (Vec::new(), Vec::new());
    for dir in folders {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut zips: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| is_zip(p) && p.is_file() && wanted.iter().any(|(d, t)| *d == dir && *t == takeout_of(p)))
            .collect();
        zips.sort();
        for z in zips {
            for e in list(&z).unwrap_or_default() {
                if Path::new(&e.inside).extension().is_some_and(|x| x.eq_ignore_ascii_case("json")) {
                    jsons.push(e);
                } else {
                    media.push(e);
                }
            }
        }
    }
    pair(&media, &jsons)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn parts_of_one_takeout() {
        let t = |p: &str| super::takeout_of(Path::new(p));
        assert_eq!(t("takeout-20261101T214823Z-1-001.zip"), "takeout-20261101t214823z-1");
        assert_eq!(t("takeout-20261101T214823Z-1-002.zip"), t("takeout-20261101T214823Z-1-001.zip"));
        assert_ne!(t("takeout-20250101T000000Z-1-001.zip"), t("takeout-20261101T214823Z-1-001.zip"));
        assert_eq!(t("holiday photos.zip"), "holiday photos");
    }
}
