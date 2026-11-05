//! Sprint 18: previews in Bring in a folder, read straight from
//! the originals into a throwaway preview folder. Needs FFmpeg
//! (`MDP_FFMPEG`) like the other picture tests.

use std::path::{Path, PathBuf};

use mdp_core::bulk::{self, ScanOptions};
use mdp_core::jobs::JobContext;
use mdp_core::{preview, Journey};

/// A real JPEG made by FFmpeg (a test pattern), `w` pixels wide.
fn jpeg(dir: &Path, name: &str, w: u32) -> PathBuf {
    let tool = mdp_core::convert::find_tool().expect("FFmpeg is needed for this test (set MDP_FFMPEG)");
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    let ok = std::process::Command::new(&tool.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc=size={w}x{}:rate=1", w * 3 / 4))
        .args(["-frames:v", "1"])
        .arg(&p)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    p
}

fn zip_of(path: &Path, files: &[(&str, Vec<u8>)]) {
    let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in files {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut w, bytes).unwrap();
    }
    w.finish().unwrap();
}

/// Bytes and time changed, to show a file was left exactly as it was.
fn fingerprint(p: &Path) -> (Vec<u8>, std::time::SystemTime) {
    (std::fs::read(p).unwrap(), std::fs::metadata(p).unwrap().modified().unwrap())
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    v.sort();
    v
}

#[test]
fn previews_read_the_originals_and_go_into_a_throwaway_folder() {
    let t = tempfile::tempdir().unwrap();
    let j = Journey::create(&t.path().join("journey"), "Test").unwrap();
    let src = t.path().join("box");
    let photo = jpeg(&src, "1972 lake.jpg", 1200);
    std::fs::write(src.join("letter.txt"), "\u{feff}\r\n  Dear June,\r\nWe arrived   safely.\r\n\r\nThe lake is cold.\r\nFourth line.\r\n").unwrap();
    // A Word file: only its words are read.
    let docx = src.join("notes.docx");
    zip_of(&docx, &[("word/document.xml", br#"<w:document><w:body><w:p><w:r><w:t>Grandpa&apos;s notes</w:t></w:r></w:p><w:p><w:r><w:t>Born 1921</w:t></w:r></w:p></w:body></w:document>"#.to_vec())]);
    let before = [fingerprint(&photo), fingerprint(&src.join("letter.txt")), fingerprint(&docx)];

    // A picture: a small JPEG in cache/preview, not beside the original.
    let thumb = preview::thumbnail(j.root(), &photo).unwrap().expect("a preview");
    assert!(thumb.starts_with(preview::dir(j.root())));
    let small = std::fs::read(&thumb).unwrap();
    assert!(small.starts_with(&[0xFF, 0xD8]) && small.len() < std::fs::metadata(&photo).unwrap().len() as usize);
    assert_eq!(preview::thumbnail(j.root(), &photo).unwrap(), Some(thumb.clone()), "made once");

    // Text and documents: their first lines.
    assert_eq!(preview::first_lines(j.root(), &src.join("letter.txt")).unwrap().as_deref(), Some("Dear June,\nWe arrived safely.\nThe lake is cold."));
    assert_eq!(preview::first_lines(j.root(), &docx).unwrap().as_deref(), Some("Grandpa's notes\nBorn 1921"));
    // A recording has neither; it plays from the original itself.
    std::fs::write(src.join("talk.wav"), b"RIFF").unwrap();
    assert_eq!(preview::thumbnail(j.root(), &src.join("talk.wav")).unwrap(), None);
    assert_eq!(preview::readable(j.root(), &src.join("talk.wav")).unwrap(), src.join("talk.wav"));
    assert!(preview::readable(j.root(), &src.join("nothing.wav")).is_err());
    assert!(preview::readable(j.root(), &src.join("letter.exe")).is_err(), "only files the app brings in");

    // The originals are exactly as they were, and nothing was added beside them.
    let after = [fingerprint(&photo), fingerprint(&src.join("letter.txt")), fingerprint(&docx)];
    assert_eq!(before, after);
    assert_eq!(files_in(&src), ["1972 lake.jpg", "letter.txt", "notes.docx", "talk.wav"]);

    preview::clear(j.root());
    assert!(!preview::dir(j.root()).exists());
}

#[test]
fn only_the_previewed_file_comes_out_of_a_zip() {
    let t = tempfile::tempdir().unwrap();
    let j = Journey::create(&t.path().join("journey"), "Test").unwrap();
    let scratch = t.path().join("scratch");
    let a = std::fs::read(jpeg(&scratch, "a.jpg", 640)).unwrap();
    let b = std::fs::read(jpeg(&scratch, "b.jpg", 320)).unwrap();
    let zip = t.path().join("takeout-20261101T000000Z-1-001.zip");
    zip_of(&zip, &[
        ("Takeout/Google Photos/Trip/a.jpg", a.clone()),
        ("Takeout/Google Photos/Trip/b.jpg", b),
        ("Takeout/Google Photos/Trip/note.txt", b"Packed the car".to_vec()),
        ("Takeout/Google Photos/Trip/song.mp3", vec![0u8; 64]),
    ]);
    let zip_before = fingerprint(&zip);
    let inside = |n: &str| zip.join("Takeout").join("Google Photos").join("Trip").join(n);

    assert!(preview::thumbnail(j.root(), &inside("a.jpg")).unwrap().is_some());
    // To play, the one file is pulled out on its own.
    let song = preview::readable(j.root(), &inside("song.mp3")).unwrap();
    assert!(song.starts_with(preview::dir(j.root())) && std::fs::read(&song).unwrap().len() == 64);
    // Text is read in place.
    assert_eq!(preview::first_lines(j.root(), &inside("note.txt")).unwrap().as_deref(), Some("Packed the car"));
    let made = files_in(&preview::dir(j.root()));
    assert_eq!(made.iter().filter(|n| n.ends_with(".jpg") && !n.ends_with("partial.jpg")).count(), 2, "a.jpg pulled out, and its preview: {made:?}");
    assert_eq!(made.iter().filter(|n| n.ends_with(".mp3")).count(), 1, "{made:?}");
    assert_eq!(made.len(), 3, "b.jpg and the text were not unpacked: {made:?}");
    assert_eq!(fingerprint(&zip), zip_before);
}

#[test]
fn preview_pictures_become_thumbnails_after_the_import() {
    let t = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&t.path().join("journey"), "Test").unwrap();
    let src = t.path().join("box");
    let one = jpeg(&src, "one.jpg", 800);
    let two = jpeg(&src.join("more"), "two.jpg", 400);
    let ctx = JobContext::new();
    let plan = bulk::scan(&j, &src, ScanOptions::default(), &ctx).unwrap();
    assert_eq!(plan.rows.len(), 2);
    // Only the first row came into view before the import.
    let row = &plan.rows.iter().find(|r| r.file == "one.jpg").unwrap();
    let thumb = preview::thumbnail(j.root(), &bulk::resolve(&plan.base, &row.file)).unwrap().unwrap();
    let preview_bytes = std::fs::read(&thumb).unwrap();

    let batch = mdp_core::import::prepare(j.root(), &bulk::plan_files(&plan), &ctx).unwrap();
    let report = j.finish_plan(batch, &plan, false).unwrap();
    assert_eq!(report.made.len(), 2);
    assert!(report.made.iter().any(|(p, _)| p == &one) && report.made.iter().any(|(p, _)| p == &two));
    assert_eq!(preview::reuse(j.root(), &report.made), 1);
    preview::clear(j.root());

    let id_one = &report.made.iter().find(|(p, _)| p == &one).unwrap().1;
    let id_two = &report.made.iter().find(|(p, _)| p == &two).unwrap().1;
    // The reused one is the preview itself; the other is made as before.
    let t1 = mdp_core::convert::thumbnail(&j, id_one).unwrap().unwrap();
    assert_eq!(std::fs::read(&t1).unwrap(), preview_bytes);
    assert!(mdp_core::convert::thumbnail(&j, id_two).unwrap().is_some());
    assert!(!preview::dir(j.root()).exists());
}
