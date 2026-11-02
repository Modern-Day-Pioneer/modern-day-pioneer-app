//! Voice: recordings made in the app and their transcripts.
//!
//! - A recording made in the app arrives as a WAV written while recording
//!   ([`WavFile`]). [`Journey::add_recording`] keeps it as a FLAC original
//!   with an MP3 copy (the WAV itself when FLAC can't be made), and puts it
//!   in its memory, or in waiting when it was made outside one.
//! - A transcript belongs to its recording: a text item
//!   linked "transcript of", with a `.txt` and a timed `.vtt` file. It is
//!   never put in a memory or listed in waiting on its own.
//! - Correcting writes new versions of both files beside the old ones, which
//!   stay; history names every version's files, and undo goes back.
//!
//! The speech model and whisper.cpp live in the app; this module only needs
//! the lines they produce, so every rule here is tested without them.

use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::convert::{find_tool, Tool};
use crate::folder;
use crate::import::{checksum, row, set_readonly, HOLDING_DIR};
use crate::{id_key, Error, Journey, Result};

/// A pause this long (seconds) starts a new paragraph in the `.txt`.
pub const PARAGRAPH_PAUSE: f64 = 2.0;

/// The `.txt` and `.vtt` of a transcript are read the same way by the export.
pub const NOT_ATTACHED: &str =
    "NOT EXISTS (SELECT 1 FROM content_relationship r WHERE r.from_id = content.id AND r.kind = 'transcript of')";

/// A mono 16-bit WAV written as samples arrive, so a long recording never
/// sits in memory. The header's sizes are filled in by [`WavFile::finish`].
pub struct WavFile {
    file: std::io::BufWriter<std::fs::File>,
    rate: u32,
    samples: u64,
}

impl WavFile {
    pub fn create(path: &Path, rate: u32) -> Result<WavFile> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut w = WavFile { file: std::io::BufWriter::new(std::fs::File::create(path)?), rate, samples: 0 };
        w.header(0)?;
        Ok(w)
    }

    fn header(&mut self, data_bytes: u32) -> Result<()> {
        let f = &mut self.file;
        f.write_all(b"RIFF")?;
        f.write_all(&(36 + data_bytes).to_le_bytes())?;
        f.write_all(b"WAVEfmt ")?;
        f.write_all(&16u32.to_le_bytes())?;
        f.write_all(&1u16.to_le_bytes())?; // PCM
        f.write_all(&1u16.to_le_bytes())?; // mono
        f.write_all(&self.rate.to_le_bytes())?;
        f.write_all(&(self.rate * 2).to_le_bytes())?;
        f.write_all(&2u16.to_le_bytes())?;
        f.write_all(&16u16.to_le_bytes())?;
        f.write_all(b"data")?;
        f.write_all(&data_bytes.to_le_bytes())?;
        Ok(())
    }

    /// Add samples between -1.0 and 1.0.
    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        for s in samples {
            self.file.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
        }
        self.samples += samples.len() as u64;
        Ok(())
    }

    pub fn seconds(&self) -> f64 {
        self.samples as f64 / self.rate as f64
    }

    /// Fill in the sizes and close the file. Returns its length in seconds.
    pub fn finish(mut self) -> Result<f64> {
        let bytes = (self.samples * 2).min(u32::MAX as u64 - 36) as u32;
        self.file.seek(SeekFrom::Start(0))?;
        self.header(bytes)?;
        self.file.flush()?;
        Ok(self.seconds())
    }
}

/// Fill in the sizes of a WAV whose recording was cut off (the app closed
/// or the PC stopped), so the samples written so far can still be kept.
pub fn repair_wav(path: &Path) -> Result<()> {
    let len = std::fs::metadata(path)?.len();
    if len <= 44 {
        return Err(Error::Invalid("The interrupted recording is empty.".into()));
    }
    let data = ((len - 44) & !1).min(u32::MAX as u64 - 36) as u32;
    let mut f = std::fs::OpenOptions::new().write(true).open(path)?;
    f.seek(SeekFrom::Start(4))?;
    f.write_all(&(36 + data).to_le_bytes())?;
    f.seek(SeekFrom::Start(40))?;
    f.write_all(&data.to_le_bytes())?;
    Ok(())
}

/// Length in seconds of a PCM WAV, read from its header.
pub fn wav_seconds(path: &Path) -> Result<f64> {
    let b = std::fs::read(path)?;
    let bad = || Error::Invalid("This recording is not a WAV file the app can read.".into());
    if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(bad());
    }
    let (mut i, mut rate, mut block) = (12usize, 0u32, 0u16);
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = i + 8;
        if id == b"fmt " && body + 16 <= b.len() {
            rate = u32::from_le_bytes([b[body + 4], b[body + 5], b[body + 6], b[body + 7]]);
            block = u16::from_le_bytes([b[body + 12], b[body + 13]]);
        } else if id == b"data" {
            if rate == 0 || block == 0 {
                return Err(bad());
            }
            let len = len.min(b.len() - body);
            return Ok(len as f64 / block as f64 / rate as f64);
        }
        i = body + len + (len & 1);
    }
    Err(bad())
}

/// One timed line of a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// Seconds from the start of the recording.
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// The plain text: lines joined with spaces, and a new paragraph after a pause.
pub fn to_text(lines: &[Line]) -> String {
    let mut out = String::new();
    let mut last_end: Option<f64> = None;
    for l in lines {
        let t = l.text.trim();
        if t.is_empty() {
            continue;
        }
        match last_end {
            Some(e) if l.start - e >= PARAGRAPH_PAUSE => out.push_str("\r\n\r\n"),
            Some(_) => out.push(' '),
            None => {}
        }
        out.push_str(t);
        last_end = Some(l.end);
    }
    out.push_str("\r\n");
    out
}

fn vtt_time(t: f64) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, (ms / 60_000) % 60, (ms / 1000) % 60, ms % 1000)
}

/// WebVTT captions, one cue per line.
pub fn to_vtt(lines: &[Line]) -> String {
    let mut v = String::from("WEBVTT\r\n\r\n");
    let mut n = 0;
    for l in lines {
        let t = l.text.trim();
        if t.is_empty() {
            continue;
        }
        n += 1;
        // A cue's text may not contain "-->" or a blank line.
        let t = t.replace("-->", "->").replace(['\r', '\n'], " ");
        v.push_str(&format!("{n}\r\n{} --> {}\r\n{t}\r\n\r\n", vtt_time(l.start), vtt_time(l.end.max(l.start))));
    }
    v
}

fn parse_time(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    let secs: f64 = parts.last()?.replace(',', ".").parse().ok()?;
    let mut total = secs;
    let mut mult = 60.0;
    for p in parts.iter().rev().skip(1) {
        total += p.parse::<f64>().ok()? * mult;
        mult *= 60.0;
    }
    Some(total)
}

/// Read the cues of a WebVTT file back into lines.
pub fn parse_vtt(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r')).peekable();
    while let Some(l) = lines.next() {
        let Some((a, b)) = l.split_once("-->") else { continue };
        let end_txt = b.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (parse_time(a), parse_time(end_txt)) else { continue };
        let mut text = Vec::new();
        while let Some(t) = lines.peek() {
            if t.trim().is_empty() {
                break;
            }
            text.push(t.trim().to_string());
            lines.next();
        }
        out.push(Line { start, end, text: text.join(" ") });
    }
    out
}

/// A recording's samples as whisper.cpp wants them: 16 kHz mono, -1.0 to 1.0.
/// FFmpeg reads any recording the Journey holds.
pub fn samples_16k(tool: &Tool, path: &Path) -> Result<Vec<f32>> {
    let mut c = std::process::Command::new(&tool.ffmpeg);
    c.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "f32le", "pipe:1"])
        .output()?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(Error::Invalid(format!("The recording could not be read ({}).", msg.lines().next().unwrap_or("unknown reason"))));
    }
    Ok(out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

/// A recording's transcript, for the screens.
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptView {
    pub id: String,
    pub recording: String,
    /// "automatic" or "checked".
    pub status: String,
    /// Who made it, for the screens (sprint 5 feedback): "automatic",
    /// "corrected" (changed by hand, not marked checked), "checked" or "typed".
    pub made: String,
    /// The speech model, or the person who corrected it.
    pub transcribed_by: Option<String>,
    pub lines: Vec<Line>,
    pub text: String,
    /// How many versions have been saved, this one included.
    pub version: i64,
}

/// How a transcript was made, from its review status and `transcribed_by`
/// ("whisper.cpp medium.en", "whisper.cpp medium.en (corrected by Jared)",
/// "... (checked by Jared)", or "Jared (typed by hand)").
pub fn made_by(status: Option<&str>, by: Option<&str>) -> &'static str {
    let by = by.unwrap_or_default();
    if is_typed(by) {
        "typed"
    } else if status == Some("checked") {
        "checked"
    } else if by.contains("(corrected by") {
        "corrected"
    } else {
        "automatic"
    }
}

fn is_typed(by: &str) -> bool {
    by.to_lowercase().contains("typed by hand")
}

/// The model (or person) without any "(checked by ...)" or "(corrected by ...)".
fn base_by(by: &str) -> &str {
    by.split(" (checked by").next().unwrap_or(by).split(" (corrected by").next().unwrap_or(by)
}

fn local_title() -> String {
    let now = chrono::Local::now();
    format!("Recording, {}", now.format("%-d %B %Y, %-I:%M %p"))
}

fn kind_of(conn: &rusqlite::Connection, id: &str) -> Result<String> {
    conn.query_row("SELECT kind FROM content WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::Invalid("That recording is no longer in the Journey.".into()))
}

/// The transcript of a recording, if it has one.
pub(crate) fn transcript_id(conn: &rusqlite::Connection, recording: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT from_id FROM content_relationship WHERE to_id = ?1 AND kind = 'transcript of' ORDER BY rowid LIMIT 1",
            [recording],
            |r| r.get(0),
        )
        .optional()?)
}

/// Write a file read-only and return (sha256, size).
fn write_readonly(path: &Path, text: &str) -> Result<(String, u64)> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, text)?;
    set_readonly(path, true)?;
    checksum(path)
}

fn remove_written(paths: &[PathBuf]) {
    for p in paths {
        let _ = set_readonly(p, false);
        let _ = std::fs::remove_file(p);
    }
}

impl Journey {
    /// Keep a finished recording (a WAV written by the app) as one action:
    /// a FLAC original with an MP3 copy, both read-only, in `memory` at the
    /// end, or waiting when `memory` is `None`.
    /// The WAV is used up: moved in when kept, otherwise deleted.
    pub fn add_recording(&mut self, wav: &Path, memory: Option<&str>, title: Option<&str>) -> Result<String> {
        let seconds = wav_seconds(wav)?;
        if let Some(m) = memory {
            if !crate::memories::exists(self.conn(), "memory", m)? {
                return Err(Error::Invalid("That memory is no longer in the Journey.".into()));
            }
        }
        let root = self.root().to_path_buf();
        let holding = root.join(folder::CONTENT_DIR).join(HOLDING_DIR).join(crate::ids::new_id());
        std::fs::create_dir_all(&holding)?;
        let made = make_files(find_tool().as_ref(), wav, &holding);
        let result = made.and_then(|files| self.record_files(&root, &files, seconds, memory, title));
        let _ = std::fs::remove_dir_all(&holding);
        if result.is_ok() && wav.is_file() {
            let _ = std::fs::remove_file(wav);
        }
        result
    }

    fn record_files(&mut self, root: &Path, files: &[(PathBuf, &'static str, &'static str, bool)], seconds: f64, memory: Option<&str>, title: Option<&str>) -> Result<String> {
        let title = title.map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).unwrap_or_else(local_title);
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let author = self.author().to_string();
        let mut moved: Vec<PathBuf> = Vec::new();
        let result = self.act(|a| {
            let id = a.create("content", row(json!({"kind": "voice recording", "name": title, "date": today,
                "date_edtf": today, "added_by": author, "source_note": "Recorded in the app"})))?;
            a.insert("voice_recording", row(json!({"id": id, "duration": crate::convert::iso_duration(seconds), "language": "en"})))?;
            for (src, ext, format, is_copy) in files {
                let stem = if *is_copy { format!("{id}-copy") } else { id.clone() };
                let rel = folder::working_path("voice recording", &stem, Some(&today), ext);
                let dest = folder::absolute(root, &rel);
                if let Some(p) = dest.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::rename(src, &dest)?;
                moved.push(dest.clone());
                set_readonly(&dest, true)?;
                let (sha, size) = checksum(&dest)?;
                a.insert("content_file", row(json!({"content_id": id, "is_copy": is_copy, "path": rel, "format": format,
                    "size": size as i64, "sha256": sha, "original_name": if *is_copy { Value::Null } else { json!(format!("{title}.{ext}")) }})))?;
            }
            if let Some(m) = memory {
                crate::memories::put_in(a, m, &id)?;
                crate::memories::refresh_status(a, m)?;
            }
            Ok(id)
        });
        if result.is_err() {
            remove_written(&moved);
        }
        result
    }

    /// The file to transcribe: the original (lossless when made in the app).
    pub fn recording_file(&self, id: &str) -> Result<PathBuf> {
        let kind = kind_of(self.conn(), id)?;
        if kind != "voice recording" && kind != "video" {
            return Err(Error::Invalid("Only recordings and videos can be transcribed.".into()));
        }
        let p: Option<String> = self
            .conn()
            .query_row("SELECT path FROM content_file WHERE content_id = ?1 ORDER BY is_copy LIMIT 1", [id], |r| r.get(0))
            .optional()?;
        p.map(|p| folder::absolute(self.root(), &p)).ok_or_else(|| Error::Invalid("This recording has no file.".into()))
    }

    /// Recordings with no transcript yet, oldest first.
    pub fn untranscribed(&self) -> Result<Vec<String>> {
        let mut st = self.conn().prepare(
            "SELECT id FROM content WHERE kind = 'voice recording'
               AND NOT EXISTS (SELECT 1 FROM content_relationship r WHERE r.to_id = content.id AND r.kind = 'transcript of')
             ORDER BY date_added, rowid",
        )?;
        let v = st.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
        Ok(v)
    }

    pub fn transcript_of(&self, recording: &str) -> Result<Option<TranscriptView>> {
        let Some(tid) = transcript_id(self.conn(), recording)? else { return Ok(None) };
        let (status, by): (Option<String>, Option<String>) = self
            .conn()
            .query_row("SELECT review_status, transcribed_by FROM text_content WHERE id = ?1", [&tid], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .unwrap_or((None, None));
        let files: Vec<(String, String)> = {
            let mut st = self.conn().prepare("SELECT path, format FROM content_file WHERE content_id = ?1")?;
            let v = st.query_map([&tid], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
            v
        };
        let read = |fmt: &str| -> Option<String> {
            files.iter().find(|f| f.1 == fmt).and_then(|f| std::fs::read_to_string(folder::absolute(self.root(), &f.0)).ok())
        };
        let text = read("text/plain").unwrap_or_default().replace("\r\n", "\n");
        let mut lines = read("text/vtt").map(|v| parse_vtt(&v)).unwrap_or_default();
        if lines.is_empty() && !text.trim().is_empty() {
            lines.push(Line { start: 0.0, end: 0.0, text: text.trim().replace('\n', " ") });
        }
        let version: i64 = self.conn().query_row(
            "SELECT count(*) FROM history WHERE table_name = 'content_file' AND field IS NULL AND old_value IS NULL
               AND item_id LIKE ?1 AND new_value LIKE '%text/plain%'",
            [format!("%{tid}%")],
            |r| r.get(0),
        )?;
        Ok(Some(TranscriptView {
            id: tid,
            recording: recording.to_string(),
            made: made_by(status.as_deref(), by.as_deref()).to_string(),
            status: status.unwrap_or_else(|| "automatic".into()),
            transcribed_by: by,
            lines,
            text,
            version: version.max(1),
        }))
    }

    /// Save an automatic transcript made by `model` (for example
    /// "whisper.cpp medium.en"). A recording that already has one gets a new
    /// version of it, marked automatic again.
    pub fn save_transcript(&mut self, recording: &str, lines: &[Line], model: &str) -> Result<String> {
        let kind = kind_of(self.conn(), recording)?;
        if kind != "voice recording" && kind != "video" {
            return Err(Error::Invalid("Only recordings and videos have transcripts.".into()));
        }
        if let Some(t) = transcript_id(self.conn(), recording)? {
            self.write_version(&t, lines, "automatic", model)?;
            return Ok(t);
        }
        self.new_transcript(recording, lines, "automatic", model, "Transcribed in the app")
    }

    /// A transcript typed by hand for a recording with none (sprint 5
    /// feedback): it is the person's own words, so it counts as checked and
    /// says "Typed by hand". Paragraphs become lines with no times.
    pub fn type_transcript(&mut self, recording: &str, text: &str) -> Result<String> {
        let kind = kind_of(self.conn(), recording)?;
        if kind != "voice recording" && kind != "video" {
            return Err(Error::Invalid("Only recordings and videos have transcripts.".into()));
        }
        if transcript_id(self.conn(), recording)?.is_some() {
            return Err(Error::Invalid("This recording already has a transcript. Correct that one instead.".into()));
        }
        let lines: Vec<Line> = text
            .replace("\r\n", "\n")
            .split("\n\n")
            .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|p| !p.is_empty())
            .map(|text| Line { start: 0.0, end: 0.0, text })
            .collect();
        if lines.is_empty() {
            return Err(Error::Invalid("The transcript is empty.".into()));
        }
        let author = self.author().to_string();
        let by = if author.trim().is_empty() { "Typed by hand".to_string() } else { format!("{author} (typed by hand)") };
        self.new_transcript(recording, &lines, "checked", &by, "Typed in the app")
    }

    fn new_transcript(&mut self, recording: &str, lines: &[Line], status: &str, by: &str, note: &str) -> Result<String> {
        let (name, date): (String, Option<String>) =
            self.conn().query_row("SELECT name, date FROM content WHERE id = ?1", [recording], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let root = self.root().to_path_buf();
        let author = self.author().to_string();
        let mut written: Vec<PathBuf> = Vec::new();
        let result = self.act(|a| {
            let id = a.create("content", row(json!({"kind": "text", "name": format!("Transcript: {name}"), "date": date,
                "added_by": author, "source_note": note})))?;
            a.insert("text_content", row(json!({"id": id, "language": "en", "transcribed_by": by, "review_status": status})))?;
            for (ext, format, is_copy, body) in [("txt", "text/plain", false, to_text(lines)), ("vtt", "text/vtt", true, to_vtt(lines))] {
                let rel = folder::working_path("text", &id, date.as_deref(), ext);
                let abs = folder::absolute(&root, &rel);
                let (sha, size) = write_readonly(&abs, &body)?;
                written.push(abs);
                a.insert("content_file", row(json!({"content_id": id, "is_copy": is_copy, "path": rel, "format": format,
                    "size": size as i64, "sha256": sha, "original_name": format!("Transcript of {name}.{ext}")})))?;
            }
            a.insert("content_relationship", row(json!({"from_id": id, "to_id": recording, "kind": "transcript of"})))?;
            Ok(id)
        });
        if result.is_err() {
            remove_written(&written);
        }
        result
    }

    /// Save a corrected transcript: new `.txt` and
    /// `.vtt` files beside the earlier ones, which stay; marked "checked"
    /// unless `checked` is false; one action, so undo goes back.
    pub fn correct_transcript(&mut self, transcript: &str, lines: &[Line], checked: bool) -> Result<()> {
        let is_transcript: bool = self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM content_relationship WHERE from_id = ?1 AND kind = 'transcript of')",
            [transcript],
            |r| r.get(0),
        )?;
        if !is_transcript {
            return Err(Error::Invalid("That transcript is no longer in the Journey.".into()));
        }
        if lines.iter().all(|l| l.text.trim().is_empty()) {
            return Err(Error::Invalid("The transcript would be empty. To start again, make a new transcript instead.".into()));
        }
        let author = self.author().to_string();
        let status = if checked { "checked" } else { "automatic" };
        let by: Option<String> = self.conn().query_row("SELECT transcribed_by FROM text_content WHERE id = ?1", [transcript], |r| r.get(0))?;
        let recording: String =
            self.conn().query_row("SELECT to_id FROM content_relationship WHERE from_id = ?1 AND kind = 'transcript of'", [transcript], |r| r.get(0))?;
        let changed = self.transcript_of(&recording)?.is_none_or(|t| {
            t.lines.len() != lines.len() || t.lines.iter().zip(lines).any(|(a, b)| a.text.trim() != b.text.trim())
        });
        let who = if author.trim().is_empty() { "hand".to_string() } else { author.clone() };
        // A person's hand is named beside the model that made the draft:
        // "checked by" when marked checked, "corrected by" when only changed.
        let by = match by {
            Some(b) if is_typed(&b) => b,
            Some(b) if checked => format!("{} (checked by {who})", base_by(&b)),
            Some(b) if changed || b.contains("(corrected by") => format!("{} (corrected by {who})", base_by(&b)),
            Some(b) => base_by(&b).to_string(),
            None => author,
        };
        self.write_version(transcript, lines, status, &by)
    }

    /// Write the next version of a transcript's files and point its rows at them.
    fn write_version(&mut self, tid: &str, lines: &[Line], status: &str, by: &str) -> Result<()> {
        let root = self.root().to_path_buf();
        let date: Option<String> = self.conn().query_row("SELECT date FROM content WHERE id = ?1", [tid], |r| r.get(0))?;
        let old: Vec<(String, String, i64, Option<String>)> = {
            let mut st = self.conn().prepare("SELECT path, format, is_copy, original_name FROM content_file WHERE content_id = ?1")?;
            let v = st.query_map([tid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<std::result::Result<_, _>>()?;
            v
        };
        // The next free version number, so no earlier file is ever overwritten.
        let mut n = 2;
        while ["txt", "vtt"].iter().any(|e| folder::absolute(&root, &folder::working_path("text", &format!("{tid}-{n}"), date.as_deref(), e)).exists()) {
            n += 1;
        }
        let mut written: Vec<PathBuf> = Vec::new();
        let result = self.act(|a| {
            for (path, _, _, _) in &old {
                a.delete("content_file", row(json!({"content_id": tid, "path": path})))?;
            }
            for (ext, format, is_copy, body) in [("txt", "text/plain", false, to_text(lines)), ("vtt", "text/vtt", true, to_vtt(lines))] {
                let rel = folder::working_path("text", &format!("{tid}-{n}"), date.as_deref(), ext);
                let abs = folder::absolute(&root, &rel);
                let (sha, size) = write_readonly(&abs, &body)?;
                written.push(abs);
                let original = old.iter().find(|o| o.1 == format).and_then(|o| o.3.clone());
                a.insert("content_file", row(json!({"content_id": tid, "is_copy": is_copy, "path": rel, "format": format,
                    "size": size as i64, "sha256": sha, "original_name": original})))?;
            }
            a.update("text_content", id_key(tid), "review_status", json!(status))?;
            a.update("text_content", id_key(tid), "transcribed_by", json!(by))?;
            Ok(())
        });
        if result.is_err() {
            remove_written(&written);
        }
        result
    }
}

/// Make the FLAC original and MP3 copy in `holding`. Without FFmpeg, or if
/// FLAC fails, the WAV is kept as the original.
fn make_files(tool: Option<&Tool>, wav: &Path, holding: &Path) -> Result<Vec<(PathBuf, &'static str, &'static str, bool)>> {
    let mut out = Vec::new();
    let run = |tool: &Tool, dest: &Path, args: &[&str]| -> bool {
        let mut c = std::process::Command::new(&tool.ffmpeg);
        c.stdin(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        c.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(wav).args(args).arg(dest);
        c.status().map(|s| s.success()).unwrap_or(false) && dest.is_file()
    };
    let flac = holding.join("original.flac");
    match tool {
        Some(t) if run(t, &flac, &["-c:a", "flac", "-compression_level", "8"]) => out.push((flac, "flac", "audio/flac", false)),
        _ => {
            let kept = holding.join("original.wav");
            std::fs::copy(wav, &kept)?;
            out.push((kept, "wav", "audio/wav", false));
        }
    }
    let mp3 = holding.join("copy.mp3");
    if let Some(t) = tool {
        if run(t, &mp3, &["-c:a", "libmp3lame", "-q:a", "2"]) {
            out.push((mp3, "mp3", "audio/mpeg", true));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- quiet start

/// Sound below this level (RMS, about -40 dB) counts as quiet.
const QUIET_LEVEL: f32 = 0.01;
/// A quiet start shorter than this is left alone.
pub const QUIET_LEAST: f64 = 1.5;
/// How much is kept before the first sound, so speech is never clipped.
pub const QUIET_KEEP: f64 = 0.5;

/// Where a recording should start playing: half a second before the first
/// steady sound, when the recording begins with more than 1.5 seconds of
/// quiet; otherwise 0. A click or a bump (under 0.2 seconds of sound) does
/// not count as the start. `samples` are mono at `rate` Hz.
pub fn quiet_start(samples: &[f32], rate: u32) -> f64 {
    let win = (rate as usize / 20).max(1); // 50 ms
    let loud: Vec<bool> = samples
        .chunks(win)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt() >= QUIET_LEVEL)
        .collect();
    let first = (0..loud.len()).find(|&i| loud[i..].iter().take(4).filter(|l| **l).count() >= 3 && loud[i]);
    let Some(i) = first else { return 0.0 };
    let t = (i * win) as f64 / rate as f64;
    if t < QUIET_LEAST {
        return 0.0;
    }
    ((t - QUIET_KEEP) * 10.0).floor() / 10.0
}

impl Journey {
    /// Recordings whose start has not been checked for quiet yet, with the file to read.
    pub fn recordings_to_check(&self) -> Result<Vec<(String, PathBuf)>> {
        let ids: Vec<String> = {
            let mut st = self.conn().prepare("SELECT id FROM voice_recording WHERE quiet_start IS NULL ORDER BY id")?;
            st.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?
        };
        let mut out = Vec::new();
        for id in ids {
            if let Ok(p) = self.recording_file(&id) {
                out.push((id, p));
            }
        }
        Ok(out)
    }

    /// Record where a recording's sound starts (0 for no quiet start). This
    /// is found by the app, not done by the person, so it is not undoable on
    /// its own; "Use the full recording" is.
    pub fn set_quiet_start(&mut self, id: &str, seconds: f64) -> Result<()> {
        if kind_of(self.conn(), id)? != "voice recording" {
            return Err(Error::Invalid("Only recordings have a quiet start.".into()));
        }
        let before = self.undo_stack.len();
        self.act(|a| a.update("voice_recording", crate::id_key(id), "quiet_start", json!((seconds.max(0.0) * 10.0).round() / 10.0)))?;
        self.undo_stack.truncate(before);
        Ok(())
    }

    /// Play the whole recording (true) or skip its quiet start again (false).
    pub fn set_play_full(&mut self, id: &str, full: bool) -> Result<()> {
        if kind_of(self.conn(), id)? != "voice recording" {
            return Err(Error::Invalid("Only recordings have a quiet start.".into()));
        }
        self.act(|a| a.update("voice_recording", crate::id_key(id), "play_full", json!(full as i64)))
    }

    /// (where to start playing, the quiet start found) for a recording: the
    /// first is 0 when there is no quiet start or the full recording was chosen.
    pub fn play_from(&self, id: &str) -> Result<(f64, f64)> {
        let r: Option<(Option<f64>, i64)> = self
            .conn()
            .query_row("SELECT quiet_start, play_full FROM voice_recording WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(match r {
            Some((Some(q), full)) if q > 0.0 => (if full == 1 { 0.0 } else { q }, q),
            _ => (0.0, 0.0),
        })
    }
}
