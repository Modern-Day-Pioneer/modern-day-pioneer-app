//! Copies in an export format for files outside the export list
//!: HEIC and AVIF pictures get a JPEG copy, BMP a PNG copy, Opus, Ogg and
//! M4A recordings an MP3 copy, and HEVC or MOV videos an H.264 MP4 copy. The
//! original stays as it was; the copy is a second file of the same content.
//! DNG pictures get the JPEG preview stored inside them (see `dng`), and
//! documents get page pictures and their text (see `documents`).
//!
//! The work is done by FFmpeg (an LGPL build that ships with the app). The
//! same tool reads the length, size and recording date of recordings and
//! videos at import. Without it, files are still imported and kept; they
//! just wait for their copy.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::json;

use crate::folder::{self, CONTENT_DIR};
use crate::import::{checksum, set_readonly, HOLDING_DIR};
use crate::jobs::JobContext;
use crate::{Error, Journey, Result};

#[derive(Debug, Clone)]
pub struct Tool {
    pub ffmpeg: PathBuf,
    /// The H.264 encoder this build has: libx264, libopenh264 or h264_mf.
    pub h264: String,
}

pub(crate) fn command(exe: &Path) -> Command {
    let mut c = Command::new(exe);
    c.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // no console window
    }
    c
}

fn try_tool(exe: PathBuf) -> Option<Tool> {
    let out = command(&exe).args(["-hide_banner", "-encoders"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let list = String::from_utf8_lossy(&out.stdout);
    let has = |name: &str| list.split_whitespace().any(|w| w == name);
    let h264 = ["libx264", "libopenh264", "h264_mf"].into_iter().find(|e| has(e))?.to_string();
    if !has("libmp3lame") || !has("mjpeg") {
        return None;
    }
    Some(Tool { ffmpeg: exe, h264 })
}

/// Find FFmpeg: `MDP_FFMPEG`, then beside the app, then on the PATH.
pub fn find_tool() -> Option<Tool> {
    static TOOL: OnceLock<Option<Tool>> = OnceLock::new();
    TOOL.get_or_init(|| {
        let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        let mut tries: Vec<PathBuf> = Vec::new();
        if let Ok(p) = std::env::var("MDP_FFMPEG") {
            tries.push(p.into());
        }
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            tries.push(dir.join(name));
            tries.push(dir.join("resources").join(name));
        }
        tries.push(name.into());
        tries.into_iter().find_map(try_tool)
    })
    .clone()
}

/// What FFmpeg says about a recording or video.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaInfo {
    pub seconds: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    /// `YYYY-MM-DD` from the file's creation time.
    pub date: Option<String>,
}

pub fn probe(tool: &Tool, path: &Path) -> MediaInfo {
    let mut info = MediaInfo::default();
    let Ok(out) = command(&tool.ffmpeg).arg("-hide_banner").arg("-i").arg(path).output() else { return info };
    let text = String::from_utf8_lossy(&out.stderr);
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("Duration: ") {
            let d = rest.split(',').next().unwrap_or("");
            let parts: Vec<f64> = d.split(':').filter_map(|p| p.trim().parse().ok()).collect();
            if parts.len() == 3 {
                info.seconds = Some(parts[0] * 3600.0 + parts[1] * 60.0 + parts[2]);
            }
        } else if l.starts_with("creation_time") && info.date.is_none() {
            let v = l.split_once(':').map(|x| x.1.trim()).unwrap_or("");
            if v.len() >= 10 && !v.starts_with("1970") && !v.starts_with("1904") && !v.starts_with("0000") {
                info.date = Some(v[..10].to_string());
            }
        } else if l.starts_with("Stream #") {
            if let Some((_, rest)) = l.split_once("Video: ") {
                if info.video_codec.is_none() && !rest.contains("(attached pic)") && !l.contains("attached pic") {
                    info.video_codec = rest.split([' ', ',']).next().map(str::to_string);
                    for part in rest.split(", ") {
                        let size = part.split_whitespace().next().unwrap_or("");
                        if let Some((w, h)) = size.split_once('x') {
                            if let (Ok(w), Ok(h)) = (w.parse(), h.parse()) {
                                info.width = Some(w);
                                info.height = Some(h);
                                break;
                            }
                        }
                    }
                }
            } else if let Some((_, rest)) = l.split_once("Audio: ") {
                if info.audio_codec.is_none() {
                    info.audio_codec = rest.split([' ', ',']).next().map(str::to_string);
                }
            }
        }
    }
    info
}

/// ISO 8601 length, for example `PT3M20S`.
pub fn iso_duration(seconds: f64) -> String {
    let s = seconds.round().max(0.0) as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    let mut out = String::from("PT");
    if h > 0 {
        out.push_str(&format!("{h}H"));
    }
    if m > 0 {
        out.push_str(&format!("{m}M"));
    }
    if s > 0 || (h == 0 && m == 0) {
        out.push_str(&format!("{s}S"));
    }
    out
}

/// DNG (camera RAW) pictures.
pub const DNG: &str = "image/x-adobe-dng";

/// The export-format copy a file needs (extension, media type), if any.
pub fn target(format: &str, video_codec: Option<&str>) -> Option<(&'static str, &'static str)> {
    match format {
        "image/heic" | "image/heif" | "image/avif" => Some(("jpg", "image/jpeg")),
        "image/bmp" => Some(("png", "image/png")),
        DNG => Some(("jpg", "image/jpeg")),
        // The app's window can't draw TIFF (issue #27, sprint 12).
        "image/tiff" => Some(("jpg", "image/jpeg")),
        "audio/opus" | "audio/ogg" | "audio/mp4" => Some(("mp3", "audio/mpeg")),
        "video/quicktime" => Some(("mp4", "video/mp4")),
        f if f.starts_with("video/mp4") && (f.contains("hevc") || video_codec.is_some_and(|c| c != "h264")) => {
            Some(("mp4", "video/mp4"))
        }
        _ => None,
    }
}

/// Write the copy of `src` to `dest` (whose extension picks the format).
pub fn convert_file(tool: &Tool, src: &Path, dest: &Path, info: &MediaInfo) -> Result<()> {
    let ext = dest.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mut c = command(&tool.ffmpeg);
    c.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(src);
    match ext.as_str() {
        "jpg" => {
            c.args(["-frames:v", "1", "-update", "1", "-q:v", "2", "-map_metadata", "0"]);
        }
        "png" => {
            c.args(["-frames:v", "1", "-update", "1", "-map_metadata", "0"]);
        }
        "mp3" => {
            c.args(["-vn", "-c:a", "libmp3lame", "-q:a", "2", "-map_metadata", "0"]);
        }
        "mp4" => {
            c.args(["-map", "0:v:0", "-map", "0:a?", "-map_metadata", "0", "-movflags", "+faststart"]);
            if info.video_codec.as_deref() == Some("h264") {
                c.args(["-c:v", "copy"]);
            } else {
                c.args(["-c:v", &tool.h264, "-pix_fmt", "yuv420p"]);
                if tool.h264 == "libx264" {
                    c.args(["-crf", "20", "-preset", "medium"]);
                } else {
                    c.args(["-b:v", "8M"]);
                }
            }
            if info.audio_codec.as_deref() == Some("aac") {
                c.args(["-c:a", "copy"]);
            } else {
                c.args(["-c:a", "aac", "-b:a", "160k"]);
            }
        }
        _ => return Err(Error::Invalid(format!("No copy is made as .{ext}."))),
    }
    c.arg(dest);
    let out = c.output()?;
    if !out.status.success() || !dest.is_file() {
        let msg = String::from_utf8_lossy(&out.stderr);
        let first = msg.lines().find(|l| !l.trim().is_empty()).unwrap_or("unknown reason");
        return Err(Error::Invalid(format!("The copy could not be made ({first}).")));
    }
    Ok(())
}

/// One original that still needs its copy.
#[derive(Debug, Clone)]
pub struct Task {
    pub content_id: String,
    pub kind: String,
    pub date: Option<String>,
    pub path: String,
    pub format: String,
    pub name: String,
}

/// A finished copy in the holding folder; a document has several.
#[derive(Debug, Clone)]
pub struct Done {
    task: Task,
    files: Vec<DoneFile>,
}

#[derive(Debug, Clone)]
struct DoneFile {
    holding: PathBuf,
    /// The working file is named `<id>-<suffix>.<ext>`.
    suffix: String,
    ext: &'static str,
    format: &'static str,
    sha256: String,
    size: u64,
}

pub struct Batch {
    dir: PathBuf,
    pub done: Vec<Done>,
}

impl Drop for Batch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Part 1 (background job): make the copies in a holding folder.
pub fn prepare(root: &Path, tasks: &[Task], tool: &Tool, ctx: &JobContext) -> Result<Batch> {
    let dir = root.join(CONTENT_DIR).join(HOLDING_DIR).join(crate::ids::new_id());
    std::fs::create_dir_all(&dir)?;
    let mut batch = Batch { dir, done: Vec::new() };
    ctx.set_total(tasks.len() as u64);
    for (n, t) in tasks.iter().enumerate() {
        ctx.check()?;
        let src = folder::absolute(root, &t.path);
        let made: Result<Vec<crate::documents::Made>> = if crate::documents::is_document(&t.format) {
            ctx.message(format!("Making readable copies of {}…", t.name));
            crate::documents::copies(&src, &t.format, &batch.dir, &t.content_id)
        } else {
            let info = if t.kind == "picture" || t.kind == "text" { MediaInfo::default() } else { probe(tool, &src) };
            let Some((ext, format)) = target(&t.format, info.video_codec.as_deref()) else {
                ctx.advance(format!("{} needs no copy", t.name));
                continue;
            };
            let holding = batch.dir.join(format!("{}.{ext}", t.content_id));
            let made = if t.format == DNG { crate::dng::preview(&src, &holding) } else { convert_file(tool, &src, &holding, &info) };
            made.map(|_| vec![crate::documents::Made { path: holding, ext, format, suffix: "copy".into() }])
        };
        let files = made.and_then(|m| {
            m.into_iter()
                .map(|m| checksum(&m.path).map(|(sha256, size)| DoneFile { holding: m.path, suffix: m.suffix, ext: m.ext, format: m.format, sha256, size }))
                .collect::<Result<Vec<_>>>()
        });
        match files {
            Ok(files) => {
                batch.done.push(Done { task: t.clone(), files });
                ctx.advance(format!("Made a copy of {} ({} of {})", t.name, n + 1, tasks.len()));
            }
            Err(e) => ctx.skip(t.name.clone(), e.to_string()),
        }
    }
    Ok(batch)
}

impl Journey {
    /// Originals in a format outside the export list that have no copy yet.
    /// HEVC videos in MP4 are marked at import as `video/mp4; codecs="hevc"`.
    pub fn conversions_needed(&self) -> Result<Vec<Task>> {
        let mut st = self.conn().prepare(
            "SELECT c.id, c.kind, c.date, f.path, f.format, c.name FROM content c JOIN content_file f ON f.content_id = c.id
             WHERE f.is_copy = 0
               AND (f.format IN ('image/heic', 'image/heif', 'image/avif', 'image/bmp', 'image/x-adobe-dng', 'image/tiff', 'audio/opus', 'audio/ogg', 'audio/mp4', 'video/quicktime')
                    OR f.format LIKE 'video/mp4;%hevc%' OR f.format LIKE 'application/%')
               AND NOT EXISTS (SELECT 1 FROM content_file g WHERE g.content_id = c.id AND g.is_copy = 1)
               AND NOT EXISTS (SELECT 1 FROM content_relationship r WHERE r.from_id = c.id AND r.kind = 'edited version of')
             ORDER BY c.id",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Task { content_id: r.get(0)?, kind: r.get(1)?, date: r.get(2)?, path: r.get(3)?, format: r.get(4)?, name: r.get(5)? })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Part 2: record the copies as one action and put them in place, read-only.
    pub fn finish_conversions(&mut self, batch: Batch) -> Result<usize> {
        if batch.done.is_empty() {
            return Ok(0);
        }
        let root = self.root().to_path_buf();
        let mut moved = Vec::new();
        let result = self.act(|a| {
            let mut n = 0;
            for d in &batch.done {
                let exists: bool = a.conn().query_row(
                    "SELECT EXISTS(SELECT 1 FROM content WHERE id = ?1)
                       AND NOT EXISTS(SELECT 1 FROM content_file WHERE content_id = ?1 AND is_copy = 1)",
                    [&d.task.content_id],
                    |r| r.get(0),
                )?;
                if !exists {
                    continue;
                }
                for f in &d.files {
                    let rel = folder::working_path(&d.task.kind, &format!("{}-{}", d.task.content_id, f.suffix), d.task.date.as_deref(), f.ext);
                    let dest = folder::absolute(&root, &rel);
                    if let Some(p) = dest.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    std::fs::rename(&f.holding, &dest)?;
                    moved.push(dest.clone());
                    set_readonly(&dest, true)?;
                    a.insert("content_file", crate::import::row(json!({
                        "content_id": d.task.content_id, "is_copy": true, "path": rel, "format": f.format,
                        "size": f.size, "sha256": f.sha256,
                    })))?;
                }
                n += 1;
            }
            Ok(n)
        });
        if result.is_err() {
            for m in moved {
                let _ = set_readonly(&m, false);
                let _ = std::fs::remove_file(&m);
            }
        }
        result
    }

    /// Both parts in one call. Returns how many copies were made, or `None`
    /// when FFmpeg is not available.
    pub fn convert_all(&mut self, ctx: &JobContext) -> Result<Option<usize>> {
        let Some(tool) = find_tool() else { return Ok(None) };
        let tasks = self.conversions_needed()?;
        let batch = prepare(self.root(), &tasks, &tool, ctx)?;
        Ok(Some(self.finish_conversions(batch)?))
    }
}

/// A small JPEG preview of a picture or video in `cache/thumbs/`, made on
/// first use. Deleting the cache loses nothing; previews are made again.
pub fn thumbnail(j: &Journey, id: &str) -> Result<Option<PathBuf>> {
    let dir = j.root().join(crate::folder::CACHE_DIR).join("thumbs");
    // FFmpeg already follows the camera's own turn flag; a turn chosen in the app is added here (sprint 8).
    let turn = j.picture_turn(id)?;
    let out = if turn == 0 { dir.join(format!("{id}.jpg")) } else { dir.join(format!("{id}-t{turn}.jpg")) };
    if out.is_file() {
        return Ok(Some(out));
    }
    // Prefer the export copy (a JPEG for HEIC and AVIF, a PNG for BMP), then the original.
    let src: Option<(String, String)> = rusqlite::OptionalExtension::optional(j.conn().query_row(
        "SELECT f.path, c.kind FROM content_file f JOIN content c ON c.id = f.content_id
         WHERE f.content_id = ?1 AND c.kind IN ('picture', 'video') ORDER BY f.is_copy DESC LIMIT 1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    let (Some((path, _)), Some(tool)) = (src, find_tool()) else { return Ok(None) };
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("{id}.partial.jpg"));
    let status = command(&tool.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(folder::absolute(j.root(), &path))
        .args(["-frames:v", "1", "-update", "1", "-vf"])
        .arg(match turn {
            1 => "transpose=1,scale='min(320,iw)':-2",
            2 => "hflip,vflip,scale='min(320,iw)':-2",
            3 => "transpose=2,scale='min(320,iw)':-2",
            _ => "scale='min(320,iw)':-2",
        })
        .args(["-q:v", "4"])
        .arg(&tmp)
        .status()?;
    if !status.success() || !tmp.is_file() {
        let _ = std::fs::remove_file(&tmp);
        return Ok(None);
    }
    std::fs::rename(&tmp, &out)?;
    Ok(Some(out))
}
