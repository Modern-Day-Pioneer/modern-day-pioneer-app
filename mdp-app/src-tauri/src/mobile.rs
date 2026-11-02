//! Android (sprint 9): what the phone does differently from the PC.
//!
//! The pickers on Android hand back content:// addresses rather than file
//! paths. The app's own Android plugin (PhoneFilesPlugin.kt, in
//! gen/android) reads and writes those through Android's streams: picked
//! files are copied into the app's cache first (the phone's own file is
//! only read, never changed), brought in from there, and the copies are
//! removed afterwards. Backups and exports are made in the app and then
//! copied to the place picked.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::{err, Res};

#[cfg(target_os = "android")]
struct PhoneFiles(tauri::plugin::PluginHandle<tauri::Wry>);

/// The app's Android plugin; does nothing on the PC.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("phone-files")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                let handle = _api.register_android_plugin("org.moderndaypioneer.app", "PhoneFilesPlugin")?;
                _app.manage(PhoneFiles(handle));
            }
            Ok(())
        })
        .build()
}

/// Call the Android plugin. It answers from its own thread, so this may
/// wait; never call it from a synchronous command (those run on the
/// screen's thread).
fn call<T: DeserializeOwned>(_app: &AppHandle, _cmd: &str, _payload: impl Serialize) -> Res<T> {
    #[cfg(target_os = "android")]
    {
        let p = _app.try_state::<PhoneFiles>().ok_or("The phone's file helper isn't ready.")?;
        p.0.run_mobile_plugin(_cmd, _payload).map_err(err)
    }
    #[cfg(not(target_os = "android"))]
    Err("This only works on the phone.".into())
}

/// True for an address only Android can open (its pickers).
pub fn is_picked(p: &str) -> bool {
    p.starts_with("content://")
}

/// A fresh, empty folder in the app's cache for picked copies.
fn scratch(app: &AppHandle, what: &str) -> Res<PathBuf> {
    let dir = app.path().app_cache_dir().map_err(err)?.join(what).join(mdp_core::journey::now().replace([':', '.'], "-"));
    std::fs::create_dir_all(&dir).map_err(err)?;
    Ok(dir)
}

#[derive(Deserialize)]
struct Copied {
    bytes: u64,
    mime: Option<String>,
    name: Option<String>,
}

/// Copy each picked address into the cache; plain paths are kept as they
/// are. Returns the paths to use and the cache folder to remove afterwards.
pub fn local_copies(app: &AppHandle, paths: &[String]) -> Res<(Vec<PathBuf>, Option<PathBuf>)> {
    if !paths.iter().any(|p| is_picked(p)) {
        return Ok((paths.iter().map(PathBuf::from).collect(), None));
    }
    let dir = scratch(app, "picked")?;
    // On the phone the file names say nothing useful (Jared, 2026-10-19), so
    // each copy is named by its kind and the time it was brought in.
    let when = chrono::Local::now().format("%Y-%m-%d %H.%M").to_string();
    let mut out = Vec::new();
    for (n, p) in paths.iter().enumerate() {
        if !is_picked(p) {
            out.push(PathBuf::from(p));
            continue;
        }
        let tmp = dir.join(format!("{n}.part"));
        let got = copy_in(app, p, &tmp)?;
        let ext = got.mime.as_deref().and_then(ext_for_mime).unwrap_or_else(|| sniff_file(&tmp));
        crate::applog::log("import", &format!("Picked {p} ({:?}, {:?}): {} bytes, as .{ext}", got.name, got.mime, got.bytes));
        if got.bytes == 0 {
            return Err(format!("A picked file came through empty ({}). If it is only in the cloud, open it once in Photos and try again.", got.name.unwrap_or_default()));
        }
        let number = if paths.len() > 1 { format!(" ({})", n + 1) } else { String::new() };
        let dest = dir.join(format!("{} {when}{number}.{ext}", kind_word(ext)));
        std::fs::rename(&tmp, &dest).map_err(err)?;
        out.push(dest);
    }
    Ok((out, Some(dir)))
}

fn copy_in(app: &AppHandle, uri: &str, dest: &Path) -> Res<Copied> {
    call(app, "copyIn", serde_json::json!({ "uri": uri, "dest": dest.display().to_string() }))
        .map_err(|e| format!("A picked file could not be read: {e}"))
}

/// Copy `local` out to a picked address (a backup saved with the picker).
pub fn copy_out(app: &AppHandle, local: &Path, uri: &str) -> Res<()> {
    call::<serde_json::Value>(app, "copyOut", serde_json::json!({ "src": local.display().to_string(), "uri": uri }))
        .map(|_| ())
        .map_err(|e| format!("The backup could not be saved there: {e}"))
}

/// Copy the folder `src` into the picked folder `tree` as `name`.
pub fn copy_tree_out(app: &AppHandle, src: &Path, tree: &str, name: &str) -> Res<()> {
    call::<serde_json::Value>(app, "copyTreeOut", serde_json::json!({ "src": src.display().to_string(), "tree": tree, "name": name }))
        .map(|_| ())
}

#[derive(Deserialize, Serialize)]
pub struct Folder {
    pub tree: Option<String>,
    pub name: Option<String>,
}

/// The system folder picker.
pub fn pick_folder(app: &AppHandle) -> Res<Folder> {
    call(app, "pickFolder", serde_json::json!({}))
}

/// The name of a folder picked earlier.
pub fn folder_name(app: &AppHandle, tree: &str) -> Option<String> {
    call::<Folder>(app, "folderName", serde_json::json!({ "tree": tree })).ok().and_then(|f| f.name)
}

/// A copy in the cache for writing, for a picked address; the caller
/// copies it out with `copy_out` when it is finished.
pub fn staging_file(app: &AppHandle, what: &str, file: &str) -> Res<PathBuf> {
    Ok(scratch(app, what)?.join(file))
}

/// Copy a picked file (a backup to restore) into the cache and return its path.
pub fn local_file(app: &AppHandle, uri: &str, file: &str) -> Res<PathBuf> {
    let dest = scratch(app, "restore")?.join(file);
    copy_in(app, uri, &dest)?;
    Ok(dest)
}

/// Copy a picked file into the cache under the name it has on the phone
/// (a speech model file, whose name says which model it is).
pub fn local_file_named(app: &AppHandle, uri: &str) -> Res<PathBuf> {
    let dir = scratch(app, "picked")?;
    let tmp = dir.join("picked.part");
    let got = copy_in(app, uri, &tmp)?;
    let name = got.name.as_deref().and_then(|n| Path::new(n).file_name()).map(|n| n.to_owned()).unwrap_or_else(|| "picked".into());
    let dest = dir.join(name);
    std::fs::rename(&tmp, &dest).map_err(err)?;
    Ok(dest)
}

/// A path as the phone shows it: inside the app's folder, without the
/// long Android part (Jared, 2026-10-19). The PC shows paths as they are.
pub fn shown(app: &AppHandle, path: &str) -> String {
    if !cfg!(mobile) {
        return path.to_string();
    }
    let base = crate::documents(app);
    match Path::new(path).strip_prefix(&base) {
        Ok(rest) => {
            let rest: Vec<String> = rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            format!("On this phone, in {}", rest.join(" › "))
        }
        Err(_) => "Inside the app".to_string(),
    }
}

fn kind_word(ext: &str) -> &'static str {
    match ext {
        "jpg" | "png" | "gif" | "webp" | "heic" | "avif" | "bmp" | "dng" | "tif" => "Photo",
        "mp4" | "mov" | "webm" | "3gp" => "Video",
        "mp3" | "m4a" | "wav" | "flac" | "ogg" | "opus" | "aac" => "Recording",
        "txt" => "Text",
        "pdf" | "doc" | "docx" | "odt" | "rtf" => "Document",
        _ => "File",
    }
}

fn ext_for_mime(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/heic" | "image/heif" => "heic",
        "image/avif" => "avif",
        "image/bmp" | "image/x-ms-bmp" => "bmp",
        "image/tiff" => "tif",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "video/3gpp" => "3gp",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => "m4a",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/aac" => "aac",
        "text/plain" => "txt",
        "image/x-adobe-dng" | "image/dng" => "dng",
        "application/pdf" => "pdf",
        "application/msword" => "doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.oasis.opendocument.text" => "odt",
        "application/rtf" | "text/rtf" => "rtf",
        _ => return None,
    })
}

fn sniff_file(file: &Path) -> &'static str {
    use std::io::Read;
    let mut head = [0u8; 64];
    let got = std::fs::File::open(file).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    sniff(&head[..got])
}

/// The file's kind from its first bytes, as a file name extension.
fn sniff(h: &[u8]) -> &'static str {
    let at = |i: usize, s: &[u8]| h.len() >= i + s.len() && &h[i..i + s.len()] == s;
    if at(0, &[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if at(0, b"\x89PNG") {
        "png"
    } else if at(0, b"GIF8") {
        "gif"
    } else if at(0, b"RIFF") && at(8, b"WEBP") {
        "webp"
    } else if at(0, b"RIFF") && at(8, b"WAVE") {
        "wav"
    } else if at(0, b"BM") && h.len() >= 14 {
        "bmp"
    } else if at(0, b"II*\0") || at(0, b"MM\0*") {
        "tif"
    } else if at(0, b"%PDF") {
        "pdf"
    } else if at(0, b"{\\rtf") {
        "rtf"
    } else if at(0, b"fLaC") {
        "flac"
    } else if at(0, b"OggS") {
        if h.windows(8).any(|w| w == b"OpusHead") { "opus" } else { "ogg" }
    } else if at(0, b"ID3") || (h.len() > 1 && h[0] == 0xFF && h[1] & 0xE0 == 0xE0) {
        "mp3"
    } else if at(4, b"ftyp") {
        let brand = h.get(8..12).unwrap_or(b"");
        match brand {
            b"avif" | b"avis" => "avif",
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" => "heic",
            b"M4A " | b"M4B " => "m4a",
            b"qt  " => "mov",
            _ => "mp4",
        }
    } else if !h.is_empty() && std::str::from_utf8(h).is_ok() {
        "txt"
    } else {
        "bin"
    }
}

/// A small JPEG preview of a picture when FFmpeg isn't there (the phone):
/// made with the image library, following the camera's turn flag and the
/// turn chosen in the app, into the same cache folder FFmpeg's go to.
pub fn picture_thumbnail(j: &mdp_core::Journey, id: &str) -> Option<PathBuf> {
    use image::ImageDecoder;
    let src: String = j
        .conn()
        .query_row(
            "SELECT f.path FROM content_file f JOIN content c ON c.id = f.content_id
             WHERE f.content_id = ?1 AND c.kind = 'picture' ORDER BY f.is_copy DESC LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .ok()?;
    let turn = j.picture_turn(id).unwrap_or(0);
    let dir = j.root().join(mdp_core::folder::CACHE_DIR).join("thumbs");
    let out = if turn == 0 { dir.join(format!("{id}.jpg")) } else { dir.join(format!("{id}-t{turn}.jpg")) };
    if out.is_file() {
        return Some(out);
    }
    let mut decoder = image::ImageReader::open(mdp_core::folder::absolute(j.root(), &src)).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut img = image::DynamicImage::from_decoder(decoder).ok()?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    img = match turn {
        1 => img.rotate90(),
        2 => img.rotate180(),
        3 => img.rotate270(),
        _ => img,
    };
    let small = img.thumbnail(320, 320).to_rgb8();
    std::fs::create_dir_all(&dir).ok()?;
    let tmp = dir.join(format!("{id}.partial.jpg"));
    let mut file = std::io::BufWriter::new(std::fs::File::create(&tmp).ok()?);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 80).encode_image(&small).ok()?;
    drop(file);
    std::fs::rename(&tmp, &out).ok()?;
    Some(out)
}

/// Keep the screen on while a model downloads or a transcript is made: a
/// phone that goes dark stops the app's work (Jared, 2026-10-20).
pub fn keep_awake(app: &AppHandle, on: bool) -> Res<()> {
    call::<serde_json::Value>(app, "keepAwake", serde_json::json!({ "on": on })).map(|_| ())
}

/// The address the phone's screens use for videos, recordings and the
/// preview: a small web server inside the app, on this phone only, that
/// hands out files in pieces as a player asks for them. Android's web view
/// doesn't play videos through the asset protocol (Jared, 2026-10-20).
/// It answers only addresses carrying its secret word, and only for files
/// in the app's own folders.
#[cfg_attr(desktop, allow(dead_code))]
pub fn start_media_server(roots: Vec<PathBuf>) -> Option<String> {
    let server = tiny_http::Server::http("127.0.0.1:0").ok()?;
    let port = server.server_addr().to_ip()?.port();
    let secret: String = (0..4).map(|_| mdp_core::ids::new_id()).collect();
    let prefix = format!("/{secret}/");
    let roots: Vec<PathBuf> = roots.into_iter().filter_map(|r| r.canonicalize().ok()).collect();
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let (prefix, roots) = (prefix.clone(), roots.clone());
            std::thread::spawn(move || serve(req, &prefix, &roots));
        }
    });
    Some(format!("http://127.0.0.1:{port}/{secret}"))
}

#[cfg_attr(desktop, allow(dead_code))]
fn serve(req: tiny_http::Request, prefix: &str, roots: &[PathBuf]) {
    let no = |req: tiny_http::Request, code: u16| {
        let _ = req.respond(tiny_http::Response::empty(code));
    };
    let url = req.url().split(['?', '#']).next().unwrap_or("").to_string();
    let Some(rest) = url.strip_prefix(prefix) else { return no(req, 404) };
    let mut path = PathBuf::from("/").join(percent_decode(rest));
    if path.is_dir() {
        path = path.join("index.html");
    }
    let Ok(path) = path.canonicalize() else { return no(req, 404) };
    if !roots.iter().any(|r| path.starts_with(r)) {
        return no(req, 403);
    }
    let Ok(mut file) = std::fs::File::open(&path) else { return no(req, 404) };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let range = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .and_then(|h| byte_range(h.value.as_str(), len));
    let kind = media_type(&path);
    let mut headers = vec![
        tiny_http::Header::from_bytes("Content-Type", kind).unwrap(),
        tiny_http::Header::from_bytes("Accept-Ranges", "bytes").unwrap(),
        tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap(),
    ];
    let (code, start, count) = match range {
        Some((a, b)) => {
            headers.push(tiny_http::Header::from_bytes("Content-Range", format!("bytes {a}-{b}/{len}")).unwrap());
            (206, a, b - a + 1)
        }
        None => (200, 0, len),
    };
    use std::io::{Read, Seek};
    if file.seek(std::io::SeekFrom::Start(start)).is_err() {
        return no(req, 416);
    }
    let _ = req.respond(tiny_http::Response::new(code.into(), headers, file.take(count), Some(count as usize), None));
}

/// "bytes=a-b", "bytes=a-" or "bytes=-n" as an inclusive range within `len`.
#[cfg_attr(desktop, allow(dead_code))]
fn byte_range(h: &str, len: u64) -> Option<(u64, u64)> {
    let (a, b) = h.strip_prefix("bytes=")?.split(',').next()?.trim().split_once('-')?;
    let last = len.checked_sub(1)?;
    let (a, b) = match (a.parse::<u64>().ok(), b.parse::<u64>().ok()) {
        (Some(a), Some(b)) => (a, b.min(last)),
        (Some(a), None) => (a, last),
        (None, Some(n)) => (len.saturating_sub(n), last),
        (None, None) => return None,
    };
    (a <= b).then_some((a, b))
}

#[cfg_attr(desktop, allow(dead_code))]
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg_attr(desktop, allow(dead_code))]
fn media_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("mp4" | "m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        Some("3gp") => "video/3gpp",
        Some("mp3") => "audio/mpeg",
        Some("m4a" | "aac") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("ogg" | "oga" | "opus") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json" | "jsonld") => "application/json",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("vtt") => "text/vtt",
        _ => "application/octet-stream",
    }
}

/// A recording as 16 kHz mono sound for whisper.cpp when FFmpeg isn't there
/// (the phone): read with the Symphonia library, which knows the phone's own
/// WAV recordings and the usual voice memo kinds (M4A, MP3, Ogg, FLAC).
pub fn samples_16k(file: &Path) -> Result<Vec<f32>, String> {
    use symphonia::core::{audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint};
    let unreadable = |e: &dyn std::fmt::Display| format!("The recording could not be read ({e}).");
    let src = std::fs::File::open(file).map_err(|e| unreadable(&e))?;
    let mut hint = Hint::new();
    if let Some(ext) = file.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, MediaSourceStream::new(Box::new(src), Default::default()), &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| unreadable(&e))?;
    let mut format = probed.format;
    let track = format.default_track().ok_or("The recording has no sound in it.")?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| unreadable(&e))?;
    let (mut mono, mut rate) = (Vec::new(), 0u32);
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(unreadable(&e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(a) => a,
            // A damaged packet is skipped, as FFmpeg does.
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(unreadable(&e)),
        };
        let spec = *audio.spec();
        rate = spec.rate;
        let channels = spec.channels.count().max(1);
        let mut buf = SampleBuffer::<f32>::new(audio.capacity() as u64, spec);
        buf.copy_interleaved_ref(audio);
        mono.extend(buf.samples().chunks_exact(channels).map(|c| c.iter().sum::<f32>() / channels as f32));
    }
    if rate == 0 {
        return Err("The recording has no sound in it.".into());
    }
    Ok(resample(&mono, rate, 16_000))
}

/// Change the sample rate by straight-line steps between samples, after
/// averaging over each output step when shrinking (enough for speech).
fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let step = from as f64 / to as f64;
    let n = (input.len() as f64 / step) as usize;
    (0..n)
        .map(|i| {
            let start = i as f64 * step;
            if step > 1.0 {
                let a = (start as usize).min(input.len() - 1);
                let b = ((start + step) as usize).clamp(a + 1, input.len());
                input[a..b].iter().sum::<f32>() / (b - a) as f32
            } else {
                let a = start as usize;
                let t = (start - a as f64) as f32;
                let next = input.get(a + 1).copied().unwrap_or(input[a]);
                input[a] * (1.0 - t) + next * t
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_from_first_bytes() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), "jpg");
        assert_eq!(sniff(b"\0\0\0\x18ftypheic\0\0"), "heic");
        assert_eq!(sniff(b"\0\0\0\x1cftypavif\0\0"), "avif");
        assert_eq!(sniff(b"BM\x36\0\0\0\0\0\0\0\x36\0\0\0"), "bmp");
        assert_eq!(sniff(b"%PDF-1.7\n"), "pdf");
        assert_eq!(sniff(b"\0\0\0\x18ftypM4A \0\0"), "m4a");
        assert_eq!(sniff(b"\0\0\0\x18ftypisom\0\0"), "mp4");
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt "), "wav");
        assert_eq!(sniff(b"Grandma's story"), "txt");
        assert_eq!(sniff(b""), "bin");
    }

    #[test]
    fn names_by_kind() {
        assert_eq!(ext_for_mime("image/jpeg"), Some("jpg"));
        assert_eq!(kind_word("jpg"), "Photo");
        assert_eq!(kind_word("mp4"), "Video");
        assert_eq!(kind_word("m4a"), "Recording");
    }

    #[test]
    fn picture_previews_without_ffmpeg() {
        let dir = std::env::temp_dir().join(format!("mdp-thumb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let j = mdp_core::sample::create(&dir).unwrap();
        let id: String = j.conn().query_row("SELECT id FROM content WHERE kind = 'picture' LIMIT 1", [], |r| r.get(0)).unwrap();
        let out = picture_thumbnail(&j, &id).expect("a preview");
        let img = image::open(&out).unwrap();
        assert!(img.width() <= 320 && img.height() <= 320);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recordings_read_without_ffmpeg() {
        let wav = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mdp-core/tests/fixtures/hello.wav");
        let samples = samples_16k(&wav).unwrap();
        assert!(samples.len() > 8000, "{} samples", samples.len());
        assert!(samples.iter().any(|s| s.abs() > 0.01));
        assert_eq!(resample(&[0.0; 48_000], 48_000, 16_000).len(), 16_000);
        assert_eq!(resample(&[0.0; 8_000], 8_000, 16_000).len(), 16_000);
    }

    /// The phone's whole voice path on this computer: the model downloaded
    /// from Hugging Face as the app does it (checked against the published
    /// size and checksum), then a recording read without FFmpeg and heard.
    /// Needs the internet (or the model already in the temp folder; the cloud
    /// proxy needs curl): cargo test --release -- --ignored phone_transcribes
    #[test]
    #[ignore]
    fn phone_transcribes() {
        let file = std::env::var("MDP_TEST_MODEL").unwrap_or_else(|_| "ggml-small.en.bin".into());
        let path = std::env::temp_dir().join(&file);
        if !path.is_file() {
            crate::voice::fetch_model_for_test(&file, &path).unwrap();
        }
        let wav = std::env::var("MDP_TEST_WAV").map(PathBuf::from)
            .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mdp-core/tests/fixtures/hello.wav"));
        let samples = samples_16k(&wav).unwrap();
        let ctx = whisper_rs::WhisperContext::new_with_params(&path, whisper_rs::WhisperContextParameters::default()).unwrap();
        let mut st = ctx.create_state().unwrap();
        let mut params = whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        st.full(params, &samples).unwrap();
        let text: String = (0..st.full_n_segments()).filter_map(|i| st.get_segment(i)).map(|s| s.to_str_lossy().unwrap().to_string()).collect();
        println!("Heard: {text}");
        assert!(!text.trim().is_empty());
    }

    #[test]
    fn media_server_hands_out_pieces() {
        use std::io::{Read, Write};
        let dir = std::env::temp_dir().join(format!("mdp-media-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a b")).unwrap();
        std::fs::write(dir.join("a b/clip.mp4"), b"0123456789").unwrap();
        let base = start_media_server(vec![dir.clone()]).unwrap();
        let get = |path: &str, range: Option<&str>| {
            let rest = base.strip_prefix("http://").unwrap();
            let (host, secret) = rest.split_once('/').unwrap();
            let mut s = std::net::TcpStream::connect(host).unwrap();
            let range = range.map(|r| format!("Range: {r}\r\n")).unwrap_or_default();
            write!(s, "GET /{secret}{path} HTTP/1.1\r\nHost: x\r\n{range}Connection: close\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let p = format!("{}/a%20b/clip.mp4", dir.canonicalize().unwrap().display());
        let whole = get(&p, None);
        assert!(whole.starts_with("HTTP/1.1 200") && whole.contains("video/mp4") && whole.ends_with("0123456789"), "{whole}");
        let piece = get(&p, Some("bytes=2-4"));
        assert!(piece.starts_with("HTTP/1.1 206") && piece.contains("bytes 2-4/10") && piece.ends_with("234"), "{piece}");
        assert!(get("/etc/passwd", None).starts_with("HTTP/1.1 403"));
        assert_eq!(byte_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(byte_range("bytes=8-", 10), Some((8, 9)));
        assert_eq!(byte_range("bytes=5-2", 10), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
