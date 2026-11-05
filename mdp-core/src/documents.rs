//! Documents (PDF, Word, OpenDocument, RTF) are kept as they are and get
//! readable copies, so an export needs no other program to show them: page
//! pictures (JPEG) and the plain text. PDF pages are drawn by PDFium, the PDF
//! library of Chromium (BSD licence), which ships beside the app like FFmpeg.
//! Word files get page pictures only when Word or LibreOffice is on the PC to
//! turn them into a PDF first; their text is read by the app itself.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use pdfium_render::prelude::*;

use crate::convert::command;
use crate::{Error, Result};

pub const PDF: &str = "application/pdf";
pub const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const DOC: &str = "application/msword";
pub const ODT: &str = "application/vnd.oasis.opendocument.text";
pub const RTF: &str = "application/rtf";

/// Long documents get pictures of their first pages only; the original holds the rest.
pub const MAX_PAGES: u16 = 100;
const PAGE_WIDTH: i32 = 1400;

pub fn is_document(format: &str) -> bool {
    [PDF, DOCX, DOC, ODT, RTF].contains(&format)
}

/// One readable copy in the holding folder: its file and the name it ends with.
#[derive(Debug, Clone)]
pub struct Made {
    pub path: PathBuf,
    pub ext: &'static str,
    pub format: &'static str,
    pub suffix: String,
}

/// Make the page pictures and the text of `src` in `dir`. Fails only when
/// nothing readable could be made, so the document waits to try again.
pub fn copies(src: &Path, format: &str, dir: &Path, id: &str) -> Result<Vec<Made>> {
    let mut made = Vec::new();
    let mut text = match format {
        DOCX => zip_text(src, "word/document.xml").map(|x| docx_text(&x)),
        ODT => zip_text(src, "content.xml").map(|x| odt_text(&x)),
        RTF => std::fs::read(src).ok().map(|b| rtf_text(&b)),
        _ => None,
    };
    let work = dir.join(format!("{id}-work"));
    std::fs::create_dir_all(&work)?;
    let pdf = if format == PDF { Some(src.to_path_buf()) } else { office_to_pdf(src, &work) };
    let mut why = None;
    if let Some(pdf) = pdf {
        match pdfium() {
            Some(lib) => match pages(lib, &pdf, dir, id) {
                Ok((pics, pdf_text)) => {
                    made.extend(pics);
                    if text.as_deref().is_none_or(|t| t.trim().is_empty()) {
                        text = Some(pdf_text);
                    }
                }
                Err(e) => why = Some(e),
            },
            None => why = Some(Error::Invalid("The PDF tool is missing, so its pages could not be shown.".into())),
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    if let Some(t) = text.filter(|t| !t.trim().is_empty()) {
        let path = dir.join(format!("{id}-text.txt"));
        std::fs::write(&path, t.trim())?;
        made.push(Made { path, ext: "txt", format: "text/plain", suffix: "text".into() });
    }
    if made.is_empty() {
        return Err(why.unwrap_or_else(|| {
            Error::Invalid("No readable copy could be made (Word or LibreOffice is needed for this kind of file).".into())
        }));
    }
    Ok(made)
}

/// The words of a document for a preview before it is brought in (sprint
/// 18): read straight from the file, never changed. A PDF gives its first
/// pages' text; a Word 97 file (.doc) needs Word or LibreOffice, so it has none.
pub fn first_text(src: &Path, format: &str) -> Option<String> {
    let text = match format {
        DOCX => zip_text(src, "word/document.xml").map(|x| docx_text(&x)),
        ODT => zip_text(src, "content.xml").map(|x| odt_text(&x)),
        RTF => std::fs::read(src).ok().map(|b| rtf_text(&b)),
        PDF => {
            let lib = pdfium()?;
            let doc = lib.load_pdf_from_file(src, None).ok()?;
            let mut t = String::new();
            for page in doc.pages().iter().take(2) {
                if let Ok(p) = page.text() {
                    t.push_str(p.all().trim());
                    t.push('\n');
                }
            }
            Some(t)
        }
        _ => None,
    }?;
    Some(text).filter(|t| !t.trim().is_empty())
}

// ------------------------------------------------------------------ PDFium

/// PDFium: `MDP_PDFIUM`, then beside the app, then the system's.
pub fn pdfium() -> Option<&'static Pdfium> {
    static LIB: OnceLock<Option<Pdfium>> = OnceLock::new();
    LIB.get_or_init(|| {
        let mut tries: Vec<PathBuf> = Vec::new();
        if let Ok(p) = std::env::var("MDP_PDFIUM") {
            tries.push(p.into());
        }
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            tries.push(Pdfium::pdfium_platform_library_name_at_path(&dir));
            tries.push(Pdfium::pdfium_platform_library_name_at_path(&dir.join("resources")));
        }
        for p in tries.into_iter().filter(|p| p.is_file()) {
            if let Ok(b) = Pdfium::bind_to_library(&p) {
                return Some(Pdfium::new(b));
            }
        }
        Pdfium::bind_to_system_library().ok().map(Pdfium::new)
    })
    .as_ref()
}

fn pages(lib: &Pdfium, pdf: &Path, dir: &Path, id: &str) -> Result<(Vec<Made>, String)> {
    let bad = |e: PdfiumError| Error::Invalid(format!("The PDF could not be read ({e:?})."));
    let doc = lib.load_pdf_from_file(pdf, None).map_err(bad)?;
    let config = PdfRenderConfig::new().set_target_width(PAGE_WIDTH).set_maximum_height(PAGE_WIDTH * 2);
    let mut made = Vec::new();
    let mut text = String::new();
    for (n, page) in doc.pages().iter().enumerate().take(MAX_PAGES as usize) {
        if let Ok(t) = page.text() {
            text.push_str(t.all().trim());
            text.push_str("\n\n");
        }
        let bitmap = page.render_with_config(&config).map_err(bad)?;
        let (w, h) = (bitmap.width() as u16, bitmap.height() as u16);
        let path = dir.join(format!("{id}-page-{:03}.jpg", n + 1));
        jpeg_encoder::Encoder::new_file(&path, 85)
            .and_then(|e| e.encode(&bitmap.as_rgba_bytes(), w, h, jpeg_encoder::ColorType::Rgba))
            .map_err(|e| Error::Invalid(format!("A page picture could not be written ({e}).")))?;
        made.push(Made { path, ext: "jpg", format: "image/jpeg", suffix: format!("page-{:03}", n + 1) });
    }
    Ok((made, text))
}

// ------------------------------------------------------------------ Word and LibreOffice

/// A PDF of a Word, OpenDocument or RTF file made by LibreOffice or Word, when
/// either is on this PC. They work on a copy in `work`, never the kept file.
fn office_to_pdf(src: &Path, work: &Path) -> Option<PathBuf> {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        return None;
    }
    let ext = src.extension()?.to_string_lossy().to_ascii_lowercase();
    let input = work.join(format!("document.{ext}"));
    std::fs::copy(src, &input).ok()?;
    crate::import::set_readonly(&input, false).ok()?;
    let out = work.join("document.pdf");
    if let Some(soffice) = libreoffice() {
        let profile = work.join("lo-profile");
        let mut c = command(&soffice);
        c.arg(format!("-env:UserInstallation=file:///{}", profile.display().to_string().replace('\\', "/").trim_start_matches('/')))
            .args(["--headless", "--norestore", "--convert-to", "pdf", "--outdir"])
            .arg(work)
            .arg(&input);
        if run_for(c, Duration::from_secs(180)) && out.is_file() {
            return Some(out);
        }
    }
    if cfg!(windows) {
        let script = work.join("to-pdf.ps1");
        std::fs::write(&script, WORD_SCRIPT).ok()?;
        let mut c = command(Path::new("powershell.exe"));
        c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(&script).arg(&input).arg(&out);
        if run_for(c, Duration::from_secs(180)) && out.is_file() {
            return Some(out);
        }
    }
    None
}

/// Word, opened read-only and unseen, saves the copy as a PDF (17 = PDF).
const WORD_SCRIPT: &str = r#"param($in, $out)
$ErrorActionPreference = 'Stop'
$w = New-Object -ComObject Word.Application
try {
  $w.Visible = $false
  $w.DisplayAlerts = 0
  $d = $w.Documents.Open($in, $false, $true, $false)
  $d.ExportAsFixedFormat($out, 17)
  $d.Close(0)
} finally { $w.Quit() }
"#;

fn libreoffice() -> Option<PathBuf> {
    let mut tries: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for v in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(p) = std::env::var(v) {
                tries.push(Path::new(&p).join("LibreOffice").join("program").join("soffice.exe"));
            }
        }
    } else {
        for name in ["soffice", "libreoffice"] {
            if let Some(p) = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(name)).find(|p| p.is_file())) {
                tries.push(p);
            }
        }
        tries.push("/Applications/LibreOffice.app/Contents/MacOS/soffice".into());
    }
    tries.into_iter().find(|p| p.is_file())
}

/// Run a command, stopping it if it takes longer than `limit`.
fn run_for(mut c: std::process::Command, limit: Duration) -> bool {
    c.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let Ok(mut child) = c.spawn() else { return false };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(200)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

// ------------------------------------------------------------------ text

fn zip_text(src: &Path, member: &str) -> Option<String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(src).ok()?).ok()?;
    let mut s = String::new();
    z.by_name(member).ok()?.read_to_string(&mut s).ok()?;
    Some(s)
}

/// Walk the tags of an XML text, calling `tag` for each tag and `text` for
/// the text between tags (entities decoded).
fn walk_xml(xml: &str, mut tag: impl FnMut(&str, &mut String), mut text: impl FnMut(&str, &mut String)) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        if lt > 0 {
            text(&unescape(&rest[..lt]), &mut out);
        }
        let Some(gt) = rest[lt..].find('>') else { break };
        tag(&rest[lt + 1..lt + gt], &mut out);
        rest = &rest[lt + gt + 1..];
    }
    out
}

fn tag_name(t: &str) -> &str {
    t.split(|c: char| c.is_whitespace() || c == '/').find(|s| !s.is_empty()).unwrap_or("")
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find(';') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let ent = &rest[i + 1..i + j];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&rest[i..i + j + 1]),
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}

/// The text of a Word document's `word/document.xml`, a line per paragraph.
pub fn docx_text(xml: &str) -> String {
    let in_t = std::cell::Cell::new(false);
    walk_xml(
        xml,
        |t, out| match (t.starts_with('/'), tag_name(t.trim_start_matches('/'))) {
            (false, "w:t") => in_t.set(!t.ends_with('/')),
            (true, "w:t") => in_t.set(false),
            (false, "w:tab") => out.push('\t'),
            (false, "w:br" | "w:cr") => out.push('\n'),
            (true, "w:p") => out.push('\n'),
            _ => {}
        },
        |s, out| {
            if in_t.get() {
                out.push_str(s)
            }
        },
    )
}

/// The text of an OpenDocument `content.xml`, a line per paragraph or heading.
pub fn odt_text(xml: &str) -> String {
    let in_body = std::cell::Cell::new(false);
    walk_xml(
        xml,
        |t, out| match (t.starts_with('/'), tag_name(t.trim_start_matches('/'))) {
            (false, "office:body") => in_body.set(true),
            (true, "office:body") => in_body.set(false),
            (false, "text:tab") => out.push('\t'),
            (false, "text:line-break") => out.push('\n'),
            (false, "text:s") => {
                let n = t.split("text:c=\"").nth(1).and_then(|r| r.split('"').next()).and_then(|n| n.parse().ok()).unwrap_or(1);
                out.push_str(&" ".repeat(n));
            }
            (true, "text:p" | "text:h") => out.push('\n'),
            _ => {}
        },
        |s, out| {
            if in_body.get() {
                out.push_str(s)
            }
        },
    )
}

/// The text of an RTF file: control words and hidden groups (fonts, colors,
/// styles, pictures) are left out.
pub fn rtf_text(b: &[u8]) -> String {
    const SKIP: &[&str] = &["fonttbl", "colortbl", "stylesheet", "info", "pict", "header", "footer", "footnote", "listtable", "listoverridetable", "rsidtbl", "generator", "xmlnstbl", "themedata", "colorschememapping", "datastore", "latentstyles"];
    let mut out = String::new();
    // For each open group: is it hidden?
    let mut groups: Vec<bool> = vec![false];
    let mut skip_chars = 0usize; // after \uN, the stand-in character
    let mut i = 0;
    let hidden = |g: &Vec<bool>| *g.last().unwrap_or(&false);
    while i < b.len() {
        let c = b[i];
        match c {
            b'{' => {
                let h = hidden(&groups) || b[i + 1..].starts_with(b"\\*");
                groups.push(h);
                i += 1;
            }
            b'}' => {
                groups.pop();
                i += 1;
            }
            b'\\' => {
                i += 1;
                let Some(&n) = b.get(i) else { break };
                if n == b'\'' {
                    if let Some(v) = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                        if !hidden(&groups) {
                            if skip_chars > 0 {
                                skip_chars -= 1;
                            } else {
                                out.push(cp1252(v));
                            }
                        }
                    }
                    i += 3;
                } else if n.is_ascii_alphabetic() {
                    let start = i;
                    while i < b.len() && b[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word = std::str::from_utf8(&b[start..i]).unwrap_or("");
                    let nstart = i;
                    if i < b.len() && (b[i] == b'-' || b[i].is_ascii_digit()) {
                        i += 1;
                        while i < b.len() && b[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    let num: Option<i32> = std::str::from_utf8(&b[nstart..i]).ok().and_then(|s| s.parse().ok());
                    if i < b.len() && b[i] == b' ' {
                        i += 1;
                    }
                    if SKIP.contains(&word) {
                        if let Some(g) = groups.last_mut() {
                            *g = true;
                        }
                    }
                    if hidden(&groups) {
                        continue;
                    }
                    match word {
                        "par" | "line" | "sect" | "page" => out.push('\n'),
                        "tab" => out.push('\t'),
                        "u" => {
                            if let Some(v) = num {
                                let v = if v < 0 { v + 65536 } else { v };
                                if let Some(ch) = char::from_u32(v as u32) {
                                    out.push(ch);
                                }
                                skip_chars = 1;
                            }
                        }
                        _ => {}
                    }
                } else {
                    if !hidden(&groups) && matches!(n, b'\\' | b'{' | b'}') {
                        out.push(n as char);
                    }
                    i += 1;
                }
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                if !hidden(&groups) {
                    if skip_chars > 0 {
                        skip_chars -= 1;
                    } else {
                        out.push(c as char);
                    }
                }
                i += 1;
            }
        }
    }
    out
}

fn cp1252(v: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    if (0x80..0xA0).contains(&v) { HIGH[(v - 0x80) as usize] } else { v as char }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_text() {
        let x = r#"<w:document><w:body><w:p><w:r><w:t>Sunday at</w:t></w:r><w:r><w:t xml:space="preserve"> the farm &amp; barn</w:t></w:r></w:p><w:p><w:r><w:t>Line</w:t><w:tab/><w:t>two</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(docx_text(x), "Sunday at the farm & barn\nLine\ttwo\n");
    }

    #[test]
    fn opendocument_text() {
        let x = r#"<office:document-content><office:automatic-styles><style:style/></office:automatic-styles><office:body><office:text><text:h>Title</text:h><text:p>A<text:s text:c="2"/>B <text:span>C</text:span></text:p></office:text></office:body></office:document-content>"#;
        assert_eq!(odt_text(x), "Title\nA  B C\n");
    }

    #[test]
    fn rtf_plain_text() {
        let r = b"{\\rtf1\\ansi{\\fonttbl{\\f0 Times;}}{\\*\\generator Word;}\\f0 Hello \\b Rowans\\b0\\par Caf\\'e9 \\u8212? done\\}}";
        assert_eq!(rtf_text(r), "Hello Rowans\nCafé — done}");
    }
}
