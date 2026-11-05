//! Previews in Bring in a folder (sprint 18), before anything is
//! copied in, so people can tell which file a row means.
//!
//! Everything is read straight from the originals and never changes them. What a preview needs is kept in a throwaway folder,
//! `cache/preview/`:
//!
//! - pictures and videos: a small JPEG made by FFmpeg, as rows come into view;
//! - recordings and videos: played from the original (read only); a file
//!   inside a zip (a Google Takeout) is first pulled out on its own, into
//!   the preview folder, and nothing else in the zip is unpacked;
//! - text and documents: their first lines.
//!
//! When the files are brought in, each preview picture becomes the new
//! item's thumbnail ([`reuse`]), so it isn't made twice. The folder is
//! emptied on cancel, after an import, when another Journey opens and at
//! app start ([`clear`]). Copying in still goes through the holding folder
//! after the person confirms.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::folder::CACHE_DIR;
use crate::import::classify;
use crate::{takeout_zip, Error, Result};

pub const PREVIEW_DIR: &str = "preview";

/// The largest file pulled out of a zip for a preview (a long video in a
/// Takeout would otherwise fill the disk just to be looked at).
pub const ZIP_LIMIT: u64 = 2 << 30;

/// How many lines of a text or document a preview shows.
pub const LINES: usize = 3;

/// The preview folder of the Journey at `root`.
pub fn dir(root: &Path) -> PathBuf {
    root.join(CACHE_DIR).join(PREVIEW_DIR)
}

/// Empty the preview folder. Nothing in it is needed: it is made again.
pub fn clear(root: &Path) {
    let d = dir(root);
    if d.exists() {
        crate::import::clear_readonly_tree(&d);
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// What the preview of a file is named by: its path, size and time
/// changed (a zip's own time for a file inside it), so a changed file
/// gets a new preview.
fn key(src: &Path) -> String {
    let outer = takeout_zip::split(src).map(|(z, _)| z).unwrap_or_else(|| src.to_path_buf());
    let changed = std::fs::metadata(&outer)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let size = takeout_zip::size(src).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(src.to_string_lossy().as_bytes());
    h.update(format!("|{size}|{changed}").as_bytes());
    let hex = format!("{:x}", h.finalize());
    hex[..24].to_string()
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// The kind of file (`picture`, `voice recording`, `video`, `text`) and its
/// media type, or an error for a file the app doesn't bring in.
fn kind_of(src: &Path) -> Result<(&'static str, &'static str)> {
    classify(&ext_of(src)).ok_or_else(|| Error::Invalid(format!("{} is not a file the app brings in.", src.display())))
}

/// A file that can be opened as it is: an ordinary file is the original
/// itself; a file inside a zip is pulled out on its own into the preview
/// folder (once). The original is only read.
pub fn readable(root: &Path, src: &Path) -> Result<PathBuf> {
    kind_of(src)?;
    if takeout_zip::split(src).is_none() {
        if !src.is_file() {
            return Err(Error::Invalid(format!("{} could not be found.", src.display())));
        }
        return Ok(src.to_path_buf());
    }
    let d = dir(root);
    // Named apart from the preview picture (`{key}.jpg`).
    let out = d.join(format!("{}-file.{}", key(src), ext_of(src)));
    if out.is_file() {
        return Ok(out);
    }
    if takeout_zip::size(src).ok_or_else(|| Error::Invalid(format!("{} could not be found in its zip file.", src.display())))? > ZIP_LIMIT {
        return Err(Error::Invalid("This file is too large to preview from inside a zip file. It can still be brought in.".into()));
    }
    std::fs::create_dir_all(&d)?;
    let tmp = out.with_extension("partial");
    let copied = takeout_zip::read_with(src, |r| {
        let mut f = std::fs::File::create(&tmp)?;
        std::io::copy(r, &mut f)?;
        Ok(())
    });
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &out)?;
    Ok(out)
}

/// A small JPEG of a picture or video (its first frame), made on first use
/// in the preview folder. `None` for recordings and text, or when FFmpeg
/// is missing or can't read the file.
pub fn thumbnail(root: &Path, src: &Path) -> Result<Option<PathBuf>> {
    let (kind, _) = kind_of(src)?;
    if kind != "picture" && kind != "video" {
        return Ok(None);
    }
    let d = dir(root);
    let k = key(src);
    let out = d.join(format!("{k}.jpg"));
    if out.is_file() {
        return Ok(Some(out));
    }
    let Some(tool) = crate::convert::find_tool() else { return Ok(None) };
    let mut input = readable(root, src)?;
    std::fs::create_dir_all(&d)?;
    // A camera RAW (DNG): the JPEG the camera stored inside it.
    let raw = d.join(format!("{k}-raw.jpg"));
    if ext_of(src) == "dng" {
        if crate::dng::preview(&input, &raw).is_err() {
            return Ok(None);
        }
        input = raw.clone();
    }
    let tmp = d.join(format!("{k}.partial.jpg"));
    let made = crate::convert::small_jpeg(&tool, &input, &tmp, 0)?;
    let _ = std::fs::remove_file(&raw);
    if !made {
        return Ok(None);
    }
    std::fs::rename(&tmp, &out)?;
    Ok(Some(out))
}

/// The first few lines of a text file or document, for its row. `None`
/// when it has no words that can be read before it is brought in.
pub fn first_lines(root: &Path, src: &Path) -> Result<Option<String>> {
    let (kind, format) = kind_of(src)?;
    if kind != "text" {
        return Ok(None);
    }
    let text = if format == "text/plain" {
        // Only the start is read: a text file can be very long.
        let mut buf = Vec::new();
        takeout_zip::read_with(src, |r| {
            use std::io::Read;
            r.take(16 * 1024).read_to_end(&mut buf)?;
            Ok(())
        })?;
        let t = match String::from_utf8(buf) {
            Ok(t) => t,
            Err(e) => {
                // Cut through a letter, or a file in an older encoding.
                let b = e.into_bytes();
                match std::str::from_utf8(&b) {
                    Ok(t) => t.to_string(),
                    Err(u) if u.error_len().is_none() => String::from_utf8_lossy(&b[..u.valid_up_to()]).into_owned(),
                    Err(_) => b.iter().map(|&c| c as char).collect(),
                }
            }
        };
        Some(t.trim_start_matches('\u{feff}').to_string())
    } else {
        crate::documents::first_text(&readable(root, src)?, format)
    };
    Ok(text.map(|t| lines(&t)).filter(|t| !t.is_empty()))
}

/// The first [`LINES`] lines with words in them, each kept short.
fn lines(text: &str) -> String {
    text.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .take(LINES)
        .map(|l| if l.chars().count() > 160 { format!("{}…", l.chars().take(160).collect::<String>().trim_end()) } else { l })
        .collect::<Vec<_>>()
        .join("\n")
}

/// After an import: each file's preview picture becomes the thumbnail of
/// the item made from it (`made` pairs the original's path with the new
/// ID). Returns how many were reused.
pub fn reuse(root: &Path, made: &[(PathBuf, String)]) -> usize {
    let thumbs = root.join(CACHE_DIR).join("thumbs");
    let d = dir(root);
    let mut n = 0;
    for (src, id) in made {
        let from = d.join(format!("{}.jpg", key(src)));
        let to = thumbs.join(format!("{id}.jpg"));
        if !from.is_file() || to.exists() {
            continue;
        }
        if std::fs::create_dir_all(&thumbs).is_ok() && (std::fs::rename(&from, &to).is_ok() || std::fs::copy(&from, &to).is_ok()) {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    #[test]
    fn lines_are_few_and_short() {
        let t = format!("\n\n  Dear June,\r\n\nWe   arrived safely.\n{}\nmore\n", "x".repeat(300));
        let out = super::lines(&t);
        let l: Vec<&str> = out.lines().collect();
        assert_eq!(l.len(), 3);
        assert_eq!(l[0], "Dear June,");
        assert_eq!(l[1], "We arrived safely.");
        assert!(l[2].ends_with('…') && l[2].chars().count() == 161);
    }
}
