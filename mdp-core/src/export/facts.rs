//! Facts written into exported copies.
//!
//! Each format's published standard is used, so a file still says what it is
//! when it is copied, emailed or uploaded on its own:
//! - JPEG: an XMP packet (Dublin Core, IPTC and Photoshop names: title,
//!   description, people, place, credit, copyright, date) and, in the EXIF
//!   block, the date taken. The picture itself is never re-encoded.
//! - MP3: ID3v2.4 (title, speaker, date, description, and the transcript as
//!   lyrics). FLAC: Vorbis comments. WAV: a RIFF INFO list.
//! - MP4 and MOV: FFmpeg copies the streams unchanged with new metadata.
//! - Plain text: a short heading (title, author, date) at the top; WebVTT: a
//!   NOTE with the title.
//!
//! Privacy: unless exact places are included, the place a picture or video
//! was taken is removed from its copy (EXIF GPS, MP4 location), and an
//! original in a format the app cannot rewrite that holds GPS is left out.

use std::path::Path;

use crate::convert::Tool;
use crate::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub id: String,
    pub title: String,
    /// The nearest exact date: `1962`, `1962-06` or `1962-06-02`.
    pub date: Option<String>,
    /// The date as known, in words.
    pub date_words: Option<String>,
    pub description: Option<String>,
    /// Everyone linked to the memories that hold it.
    pub people: Vec<String>,
    /// Those linked as "in the picture" or "on camera".
    pub in_picture: Vec<String>,
    pub place: Option<String>,
    /// The place's coordinates, when the privacy setting lets them out.
    pub coords: Option<(f64, f64)>,
    pub credit: Option<String>,
    pub copyright: Option<String>,
    pub people_note: Option<String>,
    pub speaker_note: Option<String>,
    /// For a recording: its transcript, written into the MP3 copy.
    pub transcript: Option<String>,
    /// Remove GPS and location from pictures and videos.
    pub remove_location: bool,
    /// The picture recorded GPS when it was imported.
    pub has_gps: bool,
    /// Quarter turns clockwise chosen in the app; a JPEG copy gets the EXIF
    /// turn flag for it, without re-encoding (sprint 8).
    pub turn: i64,
}

impl Facts {
    fn description_line(&self) -> Option<String> {
        let mut d = self.description.clone().unwrap_or_default();
        if let Some(n) = self.people_note.as_deref().filter(|n| !n.is_empty()) {
            if !d.is_empty() {
                d.push(' ');
            }
            d.push_str(&format!("Who is where: {n}"));
        }
        Some(d).filter(|d| !d.trim().is_empty())
    }
}

/// Copy `src` to `dest` and write the facts in. Returns `false` when the
/// file was copied but its facts could not be written.
pub fn write_copy(src: &Path, dest: &Path, format: &str, facts: Option<&Facts>, tool: Option<&Tool>) -> Result<bool> {
    let base = format.split(';').next().unwrap_or(format).trim();
    let Some(f) = facts else {
        copy_plain(src, dest)?;
        return Ok(true);
    };
    match base {
        "image/jpeg" => {
            let bytes = std::fs::read(src)?;
            match jpeg(&bytes, f) {
                Some(out) => {
                    std::fs::write(dest, out)?;
                    Ok(true)
                }
                None => {
                    if f.remove_location && f.has_gps {
                        return Err(Error::Invalid("left out: it holds the place it was taken and could not be cleaned".into()));
                    }
                    copy_plain(src, dest)?;
                    Ok(false)
                }
            }
        }
        "image/heic" | "image/heif" | "image/avif" | "image/bmp" | "image/x-adobe-dng" | "image/tiff" | "image/png" | "image/gif" | "image/webp" => {
            if f.remove_location && f.has_gps {
                return Err(Error::Invalid(
                    "the original is left out because it holds the place it was taken (exact places are hidden in Settings); its JPEG copy is exported".into(),
                ));
            }
            // These have no turn flag browsers follow, so a turned one's copy
            // is turned by FFmpeg (lossless for PNG and TIFF). HEIC, AVIF, BMP and DNG show their
            // JPEG copy, which gets the flag.
            let turn = f.turn.rem_euclid(4);
            if turn != 0 && !base.starts_with("image/hei") && base != "image/avif" && base != "image/bmp" && base != "image/x-adobe-dng" {
                if let Some(t) = tool {
                    if turned_copy(t, src, dest, turn, base == "image/gif") {
                        return Ok(true);
                    }
                }
                copy_plain(src, dest)?;
                return Ok(false);
            }
            copy_plain(src, dest)?;
            Ok(true)
        }
        "audio/mpeg" => {
            copy_plain(src, dest)?;
            Ok(mp3(dest, f).is_ok())
        }
        "audio/flac" => {
            copy_plain(src, dest)?;
            Ok(flac(dest, f).is_ok())
        }
        "audio/wav" | "audio/x-wav" | "audio/wave" => {
            let bytes = std::fs::read(src)?;
            match wav(&bytes, f) {
                Some(out) => {
                    std::fs::write(dest, out)?;
                    Ok(true)
                }
                None => {
                    copy_plain(src, dest)?;
                    Ok(false)
                }
            }
        }
        "video/mp4" | "video/quicktime" => match tool {
            Some(t) if video(t, src, dest, f) => Ok(true),
            _ => {
                let _ = std::fs::remove_file(dest);
                copy_plain(src, dest)?;
                Ok(false)
            }
        },
        "text/plain" => {
            let body = super::read_text(src).unwrap_or_default();
            std::fs::write(dest, text_with_heading(&body, f).replace('\n', "\r\n"))?;
            Ok(true)
        }
        "text/vtt" => {
            let body = super::read_text(src).unwrap_or_default();
            std::fs::write(dest, vtt_with_note(&body, f))?;
            Ok(true)
        }
        _ => {
            copy_plain(src, dest)?;
            Ok(true)
        }
    }
}

/// A plain copy, writable (the working copies are read-only).
fn copy_plain(src: &Path, dest: &Path) -> Result<()> {
    std::fs::copy(src, dest)?;
    crate::import::set_readonly(dest, false)?;
    Ok(())
}

// ------------------------------------------------------------------ text

/// The heading of an exported text: title, then author and date.
pub fn text_with_heading(body: &str, f: &Facts) -> String {
    let mut head = vec![f.title.clone()];
    let by: Vec<String> = [f.credit.clone(), f.date_words.clone()].into_iter().flatten().filter(|s| !s.is_empty()).collect();
    if !by.is_empty() {
        head.push(by.join(", "));
    }
    format!("{}\n\n{}\n", head.join("\n"), body.trim_end())
}

fn vtt_with_note(body: &str, f: &Facts) -> String {
    let title = f.title.replace("-->", "to");
    match body.split_once('\n') {
        Some((first, rest)) if first.starts_with("WEBVTT") => format!("{first}\n\nNOTE\n{title}\n\n{}", rest.trim_start_matches('\n')),
        _ => body.to_string(),
    }
}

// ------------------------------------------------------------------ JPEG

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const XMP_NS: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// The XMP packet for a picture.
pub fn xmp_packet(f: &Facts) -> String {
    let alt = |tag: &str, v: &str| format!("<{tag}><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></{tag}>", xml(v));
    let bag = |tag: &str, v: &[String]| {
        format!("<{tag}><rdf:Bag>{}</rdf:Bag></{tag}>", v.iter().map(|x| format!("<rdf:li>{}</rdf:li>", xml(x))).collect::<String>())
    };
    let mut p = String::new();
    p.push_str(&alt("dc:title", &f.title));
    if let Some(d) = f.description_line() {
        p.push_str(&alt("dc:description", &d));
    }
    if let Some(c) = f.credit.as_deref().filter(|c| !c.is_empty()) {
        p.push_str(&format!("<dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>", xml(c)));
        p.push_str(&format!("<photoshop:Credit>{}</photoshop:Credit>", xml(c)));
    }
    if let Some(c) = f.copyright.as_deref().filter(|c| !c.is_empty()) {
        p.push_str(&alt("dc:rights", c));
    }
    let mut subjects = f.people.clone();
    if let Some(pl) = &f.place {
        subjects.push(pl.clone());
    }
    if !subjects.is_empty() {
        p.push_str(&bag("dc:subject", &subjects));
    }
    if !f.in_picture.is_empty() {
        p.push_str(&bag("Iptc4xmpExt:PersonInImage", &f.in_picture));
    }
    if let Some(pl) = &f.place {
        p.push_str(&format!("<Iptc4xmpCore:Location>{}</Iptc4xmpCore:Location>", xml(pl)));
    }
    if let Some((lat, lon)) = f.coords {
        p.push_str(&format!("<exif:GPSLatitude>{}</exif:GPSLatitude><exif:GPSLongitude>{}</exif:GPSLongitude>", xmp_gps(lat, 'N', 'S'), xmp_gps(lon, 'E', 'W')));
    }
    if let Some(d) = &f.date {
        p.push_str(&format!("<photoshop:DateCreated>{}</photoshop:DateCreated>", xml(d)));
    }
    p.push_str(&format!("<dc:identifier>urn:mdp:item:{}</dc:identifier>", xml(&f.id)));
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\" \
xmlns:Iptc4xmpCore=\"http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/\" xmlns:Iptc4xmpExt=\"http://iptc.org/std/Iptc4xmpExt/2008-02-29/\" \
xmlns:exif=\"http://ns.adobe.com/exif/1.0/\">{p}</rdf:Description></rdf:RDF></x:xmpmeta>\n<?xpacket end=\"w\"?>"
    )
}

/// XMP's GPS form: `41,13.38N`.
fn xmp_gps(v: f64, pos: char, neg: char) -> String {
    let a = v.abs();
    let deg = a.trunc();
    format!("{},{:.4}{}", deg as i64, (a - deg) * 60.0, if v < 0.0 { neg } else { pos })
}

/// The EXIF form of a date: `1962:06:02 00:00:00`.
pub fn exif_date(d: &str) -> Option<String> {
    let p: Vec<&str> = d.split('-').collect();
    let ok = |s: &str, n: usize| s.len() == n && s.chars().all(|c| c.is_ascii_digit());
    match p.as_slice() {
        [y] if ok(y, 4) => Some(format!("{y}:01:01 00:00:00")),
        [y, m] if ok(y, 4) && ok(m, 2) => Some(format!("{y}:{m}:01 00:00:00")),
        [y, m, dd] if ok(y, 4) && ok(m, 2) && ok(&dd[..2.min(dd.len())], 2) => Some(format!("{y}:{m}:{} 00:00:00", &dd[..2])),
        _ => None,
    }
}

/// Rewrite a JPEG's information blocks; the picture data is copied byte for byte.
/// `None` when the file is not a JPEG the app can read.
pub fn jpeg(bytes: &[u8], f: &Facts) -> Option<Vec<u8>> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let packet = xmp_packet(f);
    let mut xmp_payload = XMP_NS.to_vec();
    xmp_payload.extend_from_slice(packet.as_bytes());
    if xmp_payload.len() + 2 > 0xFFFF {
        return None;
    }
    let segment = |marker: u8, payload: &[u8]| -> Vec<u8> {
        let mut s = vec![0xFF, marker];
        s.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        s.extend_from_slice(payload);
        s
    };
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2;
    let mut inserted = false;
    let mut saw_exif = false;
    loop {
        if i + 1 >= bytes.len() || bytes[i] != 0xFF {
            return None;
        }
        while i + 1 < bytes.len() && bytes[i + 1] == 0xFF {
            i += 1;
        }
        let marker = *bytes.get(i + 1)?;
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            out.extend_from_slice(&bytes[i..i + 2]);
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([*bytes.get(i + 2)?, *bytes.get(i + 3)?]) as usize;
        if len < 2 || i + 2 + len > bytes.len() {
            return None;
        }
        let payload = &bytes[i + 4..i + 2 + len];
        let is_exif = marker == 0xE1 && payload.starts_with(b"Exif\0\0");
        let is_xmp = marker == 0xE1 && payload.starts_with(XMP_NS);
        if !inserted && !(marker == 0xE0 || is_exif) {
            if !saw_exif {
                if let Some(e) = new_exif(f) {
                    out.extend(segment(0xE1, &e));
                }
            }
            out.extend(segment(0xE1, &xmp_payload));
            inserted = true;
        }
        if is_exif {
            saw_exif = true;
            let mut p = payload.to_vec();
            edit_exif(&mut p[6..], f.date.as_deref(), f.remove_location);
            if f.turn.rem_euclid(4) != 0 {
                let mut tiff = p.split_off(6);
                turn_exif(&mut tiff, f.turn)?;
                p.extend(tiff);
                if p.len() + 2 > 0xFFFF {
                    return None;
                }
            }
            out.extend(segment(0xE1, &p));
        } else if !is_xmp {
            out.extend_from_slice(&bytes[i..i + 2 + len]);
        }
        i += 2 + len;
    }
    if !inserted {
        if !saw_exif {
            if let Some(e) = new_exif(f) {
                out.extend(segment(0xE1, &e));
            }
        }
        out.extend(segment(0xE1, &xmp_payload));
    }
    out.extend_from_slice(&bytes[i..]);
    Some(out)
}

/// A copy turned `turn` quarter turns clockwise, in the same format.
fn turned_copy(t: &Tool, src: &Path, dest: &Path, turn: i64, gif: bool) -> bool {
    let turned = ["transpose=1", "hflip,vflip", "transpose=2"][(turn - 1) as usize];
    let filter = if gif { format!("{turned},split[a][b];[a]palettegen[p];[b][p]paletteuse") } else { turned.to_string() };
    let _ = std::fs::remove_file(dest);
    let ok = crate::convert::command(&t.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(src)
        .args(["-vf", &filter, "-map_metadata", "-1"])
        .args(if gif { &[][..] } else { &["-frames:v", "1", "-update", "1"][..] })
        .arg("-y")
        .arg(dest)
        .status()
        .is_ok_and(|s| s.success());
    ok && std::fs::metadata(dest).is_ok_and(|m| m.len() > 0)
}

/// A small EXIF block with only the date taken, for pictures with none (scans).
fn new_exif(f: &Facts) -> Option<Vec<u8>> {
    let turn = f.turn.rem_euclid(4);
    let Some(d) = f.date.as_deref().and_then(exif_date) else {
        if turn == 0 {
            return None;
        }
        // Only the turn flag: IFD0 with one entry, Orientation.
        let mut e = b"Exif\0\0II\x2a\x00\x08\x00\x00\x00".to_vec();
        e.extend_from_slice(&1u16.to_le_bytes());
        e.extend_from_slice(&0x0112u16.to_le_bytes());
        e.extend_from_slice(&3u16.to_le_bytes());
        e.extend_from_slice(&1u32.to_le_bytes());
        e.extend_from_slice(&(crate::pictures::turned_orientation(1, turn) as u32).to_le_bytes());
        e.extend_from_slice(&0u32.to_le_bytes());
        return Some(e);
    };
    let mut e = b"Exif\0\0II\x2a\x00\x08\x00\x00\x00".to_vec();
    // IFD0: one entry, the pointer to the EXIF IFD at offset 26.
    e.extend_from_slice(&1u16.to_le_bytes());
    e.extend_from_slice(&0x8769u16.to_le_bytes());
    e.extend_from_slice(&4u16.to_le_bytes());
    e.extend_from_slice(&1u32.to_le_bytes());
    e.extend_from_slice(&26u32.to_le_bytes());
    e.extend_from_slice(&0u32.to_le_bytes());
    // EXIF IFD: DateTimeOriginal, its 20 bytes at offset 44.
    e.extend_from_slice(&1u16.to_le_bytes());
    e.extend_from_slice(&0x9003u16.to_le_bytes());
    e.extend_from_slice(&2u16.to_le_bytes());
    e.extend_from_slice(&20u32.to_le_bytes());
    e.extend_from_slice(&44u32.to_le_bytes());
    e.extend_from_slice(&0u32.to_le_bytes());
    e.extend_from_slice(d.as_bytes());
    e.push(0);
    if turn != 0 {
        let mut tiff = e.split_off(6);
        turn_exif(&mut tiff, turn)?;
        e.extend(tiff);
    }
    Some(e)
}

/// Add `turn` quarter turns to the EXIF turn flag (Orientation, in IFD0):
/// change it where the camera wrote one, otherwise add it by writing a
/// copy of IFD0 with the flag at the end of the block (other offsets stay valid).
fn turn_exif(tiff: &mut Vec<u8>, turn: i64) -> Option<()> {
    if tiff.len() < 8 {
        return None;
    }
    let le = &tiff[0..2] == b"II";
    let mut t = Tiff { b: tiff.as_mut_slice(), le };
    let ifd0 = t.u32(4)? as usize;
    let entries = t.entries(ifd0);
    if let Some(&(_, ty, _, at)) = entries.iter().find(|e| e.0 == 0x0112) {
        if ty != 3 {
            return None;
        }
        let old = t.u16(at)?;
        t.set_u16(at, crate::pictures::turned_orientation(old, turn));
        return Some(());
    }
    let n = entries.len();
    let next = t.u32(ifd0 + 2 + 12 * n)?;
    let mut raw: Vec<(u16, Vec<u8>)> = (0..n).map(|k| (entries[k].0, tiff[ifd0 + 2 + 12 * k..ifd0 + 14 + 12 * k].to_vec())).collect();
    let u16b = |v: u16| if le { v.to_le_bytes() } else { v.to_be_bytes() };
    let u32b = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
    let mut entry = Vec::with_capacity(12);
    entry.extend_from_slice(&u16b(0x0112));
    entry.extend_from_slice(&u16b(3));
    entry.extend_from_slice(&u32b(1));
    entry.extend_from_slice(&u16b(crate::pictures::turned_orientation(1, turn)));
    entry.extend_from_slice(&[0, 0]);
    raw.push((0x0112, entry));
    raw.sort_by_key(|r| r.0);
    if tiff.len() % 2 == 1 {
        tiff.push(0);
    }
    let at = u32::try_from(tiff.len()).ok()?;
    tiff.extend_from_slice(&u16b(u16::try_from(raw.len()).ok()?));
    for (_, r) in raw {
        tiff.extend(r);
    }
    tiff.extend_from_slice(&u32b(next));
    let header = u32b(at);
    tiff[4..8].copy_from_slice(&header);
    Some(())
}

struct Tiff<'a> {
    b: &'a mut [u8],
    le: bool,
}

impl Tiff<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        let s = self.b.get(at..at + 2)?;
        Some(if self.le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) })
    }
    fn u32(&self, at: usize) -> Option<u32> {
        let s = self.b.get(at..at + 4)?;
        Some(if self.le { u32::from_le_bytes([s[0], s[1], s[2], s[3]]) } else { u32::from_be_bytes([s[0], s[1], s[2], s[3]]) })
    }
    fn set_u16(&mut self, at: usize, v: u16) {
        let bytes = if self.le { v.to_le_bytes() } else { v.to_be_bytes() };
        if let Some(s) = self.b.get_mut(at..at + 2) {
            s.copy_from_slice(&bytes);
        }
    }
    fn zero(&mut self, at: usize, n: usize) {
        if let Some(s) = self.b.get_mut(at..at + n) {
            s.fill(0);
        }
    }
    /// (tag, type, count, offset of the value field) for each entry of an IFD.
    fn entries(&self, ifd: usize) -> Vec<(u16, u16, u32, usize)> {
        let Some(n) = self.u16(ifd) else { return vec![] };
        (0..n as usize)
            .filter_map(|k| {
                let e = ifd + 2 + 12 * k;
                Some((self.u16(e)?, self.u16(e + 2)?, self.u32(e + 4)?, e + 8))
            })
            .collect()
    }
}

fn type_size(t: u16) -> usize {
    match t {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 1,
    }
}

/// In place: correct the date taken (keeping a more exact one that agrees),
/// and empty the GPS block, zeroing its values, when the place is hidden.
fn edit_exif(tiff: &mut [u8], date: Option<&str>, remove_gps: bool) {
    if tiff.len() < 8 {
        return;
    }
    let le = &tiff[0..2] == b"II";
    let mut t = Tiff { b: tiff, le };
    let Some(ifd0) = t.u32(4).map(|v| v as usize) else { return };
    let mut exif_ifd = None;
    let mut gps_ifd = None;
    for (tag, _, _, at) in t.entries(ifd0) {
        if tag == 0x8769 {
            exif_ifd = t.u32(at).map(|v| v as usize);
        }
        if tag == 0x8825 {
            gps_ifd = Some((t.u32(at).map(|v| v as usize), at));
        }
    }
    if let (Some(d), Some(ifd)) = (date, exif_ifd) {
        if let Some(new) = exif_date(d) {
            // "1962" agrees with "1962:06:02 14:33:10", so the camera's exact time stays.
            let known = d.replace('-', ":");
            for (tag, ty, count, at) in t.entries(ifd) {
                if (tag == 0x9003 || tag == 0x9004) && ty == 2 && count == 20 {
                    if let Some(off) = t.u32(at).map(|v| v as usize) {
                        let old = t.b.get(off..off + 19).map(|s| String::from_utf8_lossy(s).into_owned()).unwrap_or_default();
                        if !old.starts_with(&known) {
                            if let Some(s) = t.b.get_mut(off..off + 20) {
                                s[..19].copy_from_slice(new.as_bytes());
                                s[19] = 0;
                            }
                        }
                    }
                }
            }
        }
    }
    if remove_gps {
        if let Some((Some(g), _)) = gps_ifd {
            for (_, ty, count, at) in t.entries(g) {
                let size = type_size(ty) * count as usize;
                if size > 4 {
                    if let Some(off) = t.u32(at).map(|v| v as usize) {
                        t.zero(off, size);
                    }
                }
                t.zero(at - 8, 12);
            }
            // The block stays, empty, so the pointer to it is still valid.
            t.set_u16(g, 0);
        }
    }
}

// ------------------------------------------------------------------ audio

fn speaker(f: &Facts) -> Option<String> {
    f.credit.clone().filter(|c| !c.is_empty()).or_else(|| f.speaker_note.clone())
}

fn mp3(path: &Path, f: &Facts) -> std::result::Result<(), String> {
    use id3::{frame, Tag, TagLike, Version};
    let mut tag = match Tag::read_from_path(path) {
        Ok(t) => t,
        Err(e) if matches!(e.kind, id3::ErrorKind::NoTag) => Tag::new(),
        Err(e) => return Err(e.to_string()),
    };
    tag.set_title(&f.title);
    if let Some(s) = speaker(f) {
        tag.set_artist(s);
    }
    if let Some(ts) = f.date.as_deref().and_then(|d| d.parse::<id3::Timestamp>().ok()) {
        tag.set_date_recorded(ts);
    }
    tag.remove_comment(None, None);
    if let Some(d) = f.description_line() {
        tag.add_frame(frame::Comment { lang: "eng".into(), description: String::new(), text: d });
    }
    if !f.people.is_empty() {
        tag.add_frame(frame::ExtendedText { description: "People".into(), value: f.people.join(", ") });
    }
    if let Some(p) = &f.place {
        tag.add_frame(frame::ExtendedText { description: "Place".into(), value: p.clone() });
    }
    tag.add_frame(frame::ExtendedText { description: "Modern Day Pioneer ID".into(), value: f.id.clone() });
    if let Some(c) = &f.copyright {
        tag.set_text("TCOP", c.clone());
    }
    tag.remove_all_lyrics();
    if let Some(t) = &f.transcript {
        tag.add_frame(frame::Lyrics { lang: "eng".into(), description: "Transcript".into(), text: t.clone() });
    }
    tag.write_to_path(path, Version::Id3v24).map_err(|e| e.to_string())
}

fn flac(path: &Path, f: &Facts) -> std::result::Result<(), String> {
    let mut tag = metaflac::Tag::read_from_path(path).map_err(|e| e.to_string())?;
    let vc = tag.vorbis_comments_mut();
    vc.set_title(vec![f.title.clone()]);
    if let Some(s) = speaker(f) {
        vc.set_artist(vec![s]);
    }
    if let Some(d) = &f.date {
        vc.set("DATE", vec![d.clone()]);
    }
    if let Some(d) = f.description_line() {
        vc.set("DESCRIPTION", vec![d]);
    }
    if !f.people.is_empty() {
        vc.set("PEOPLE", vec![f.people.join(", ")]);
    }
    if let Some(p) = &f.place {
        vc.set("LOCATION", vec![p.clone()]);
    }
    if let Some(c) = &f.copyright {
        vc.set("COPYRIGHT", vec![c.clone()]);
    }
    vc.set("MDPID", vec![f.id.clone()]);
    tag.save().map_err(|e| e.to_string())
}

/// A WAV with its RIFF INFO list replaced by one holding the facts.
pub fn wav(bytes: &[u8], f: &Facts) -> Option<Vec<u8>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size = u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
        let end = (i + 8 + size).min(bytes.len());
        let is_info = id == b"LIST" && bytes.get(i + 8..i + 12) == Some(b"INFO");
        if !is_info {
            out.extend_from_slice(&bytes[i..end]);
            if size % 2 == 1 && end < bytes.len() {
                out.push(bytes[end]);
            } else if size % 2 == 1 {
                out.push(0);
            }
        }
        i = end + (size % 2);
    }
    let mut info = b"INFO".to_vec();
    let mut add = |id: &[u8; 4], v: &str| {
        if v.is_empty() {
            return;
        }
        let mut data = v.as_bytes().to_vec();
        data.push(0);
        info.extend_from_slice(id);
        info.extend_from_slice(&(data.len() as u32).to_le_bytes());
        info.extend_from_slice(&data);
        if data.len() % 2 == 1 {
            info.push(0);
        }
    };
    add(b"INAM", &f.title);
    if let Some(s) = speaker(f) {
        add(b"IART", &s);
    }
    if let Some(d) = &f.date {
        add(b"ICRD", d);
    }
    if let Some(d) = f.description_line() {
        add(b"ICMT", &d);
    }
    if let Some(c) = &f.copyright {
        add(b"ICOP", c);
    }
    add(b"ISBJ", &f.people.join(", "));
    out.extend_from_slice(b"LIST");
    out.extend_from_slice(&(info.len() as u32).to_le_bytes());
    out.extend_from_slice(&info);
    let total = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&total.to_le_bytes());
    Some(out)
}

// ------------------------------------------------------------------ video

fn video(tool: &Tool, src: &Path, dest: &Path, f: &Facts) -> bool {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into(), "-i".into()];
    args.push(src.display().to_string());
    for a in ["-map", "0:v", "-map", "0:a?", "-c", "copy", "-map_metadata", "0"] {
        args.push(a.into());
    }
    let mut meta = |k: &str, v: &str| {
        args.push("-metadata".into());
        args.push(format!("{k}={v}"));
    };
    meta("title", &f.title);
    if let Some(d) = &f.date {
        meta("date", d);
    }
    if let Some(d) = f.description_line() {
        meta("description", &d);
        meta("comment", &d);
    }
    if let Some(s) = speaker(f) {
        meta("artist", &s);
    }
    if let Some(c) = &f.copyright {
        meta("copyright", c);
    }
    if f.remove_location {
        meta("location", "");
        meta("location-eng", "");
        meta("com.apple.quicktime.location.ISO6709", "");
    }
    args.push(dest.display().to_string());
    let ok = crate::convert::command(&tool.ffmpeg).args(&args).output().map(|o| o.status.success()).unwrap_or(false);
    ok && dest.is_file()
}
