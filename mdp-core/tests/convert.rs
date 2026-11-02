//! These need FFmpeg 7.1 or later (for HEIC): the GitHub jobs set
//! `MDP_FFMPEG` to the same LGPL build that ships with the app.

use std::path::{Path, PathBuf};

use mdp_core::convert::{find_tool, iso_duration, probe};
use mdp_core::folder::absolute;
use mdp_core::import::checksum;
use mdp_core::jobs::JobContext;
use mdp_core::Journey;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn walkdir(d: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() { out.extend(walkdir(&p)) } else { out.push(p) }
    }
    out
}

fn files_of(j: &Journey, id: &str) -> Vec<(i64, String, String)> {
    let mut st = j.conn().prepare("SELECT is_copy, path, format FROM content_file WHERE content_id = ?1 ORDER BY is_copy, path").unwrap();
    st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(|r| r.unwrap()).collect()
}

#[test]
fn durations_read_as_iso() {
    assert_eq!(iso_duration(200.4), "PT3M20S");
    assert_eq!(iso_duration(3600.0), "PT1H");
    assert_eq!(iso_duration(0.2), "PT0S");
}

#[test]
fn recordings_and_videos_get_length_size_and_date() {
    let tool = find_tool().expect("FFmpeg is needed for this test (set MDP_FFMPEG)");
    let info = probe(&tool, &fixtures().join("phone-video.mp4"));
    assert_eq!(info.video_codec.as_deref(), Some("hevc"));
    assert_eq!((info.width, info.height), (Some(64), Some(48)));

    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (r, _) = j.import(&[fixtures().join("phone-video.mp4"), fixtures().join("hello.wav")], &JobContext::new()).unwrap();
    let (date, dur, w): (String, String, i64) = j
        .conn()
        .query_row("SELECT c.date, v.duration, v.width FROM content c JOIN video v ON v.id = c.id WHERE c.id = ?1", [&r.imported[0]], |x| {
            Ok((x.get(0)?, x.get(1)?, x.get(2)?))
        })
        .unwrap();
    assert_eq!((date.as_str(), dur.as_str(), w), ("2021-12-25", "PT1S", 64));
    assert!(files_of(&j, &r.imported[0])[0].1.starts_with("managed-content/video/2021/"));
    assert_eq!(files_of(&j, &r.imported[0])[0].2, "video/mp4; codecs=\"hevc\"");
    let dur: String = j.conn().query_row("SELECT duration FROM voice_recording WHERE id = ?1", [&r.imported[1]], |x| x.get(0)).unwrap();
    assert_eq!(dur, "PT1S");
}

#[test]
fn heic_hevc_opus_and_mov_get_a_copy_beside_the_original() {
    let tool = find_tool().expect("FFmpeg is needed for this test (set MDP_FFMPEG)");
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let names = ["red-square.heic", "phone-video.mp4", "voice-note.opus", "clip.mov", "walter-camera.jpg", "hello.wav"];
    let paths: Vec<PathBuf> = names.iter().map(|n| fixtures().join(n)).collect();
    let (r, _) = j.import(&paths, &JobContext::new()).unwrap();
    assert_eq!(r.imported.len(), 6);
    assert_eq!(j.conversions_needed().unwrap().len(), 4, "JPEG and WAV need no copy");

    let originals: Vec<(String, String)> = r.imported.iter().map(|id| {
        let p = absolute(j.root(), &files_of(&j, id)[0].1);
        (id.clone(), checksum(&p).unwrap().0)
    }).collect();
    let ctx = JobContext::new();
    assert_eq!(j.convert_all(&ctx).unwrap(), Some(4), "skipped: {:?}", ctx.skipped());
    assert!(j.conversions_needed().unwrap().is_empty());

    let expect = [("image/jpeg", "jpg"), ("video/mp4", "mp4"), ("audio/mpeg", "mp3"), ("video/mp4", "mp4")];
    for (i, (fmt, ext)) in expect.iter().enumerate() {
        let f = files_of(&j, &r.imported[i]);
        assert_eq!(f.len(), 2, "{} should have its copy", names[i]);
        assert_eq!((f[1].0, f[1].2.as_str()), (1, *fmt));
        assert!(f[1].1.ends_with(&format!("{}-copy.{ext}", r.imported[i])));
        let copy = absolute(j.root(), &f[1].1);
        assert!(std::fs::metadata(&copy).unwrap().permissions().readonly());
        assert!(std::fs::metadata(&copy).unwrap().len() > 0);
    }
    // The copy of the HEVC video is H.264.
    let f = files_of(&j, &r.imported[1]);
    assert_eq!(probe(&tool, &absolute(j.root(), &f[1].1)).video_codec.as_deref(), Some("h264"));
    // Originals are unchanged.
    for (id, sum) in originals {
        assert_eq!(checksum(&absolute(j.root(), &files_of(&j, &id)[0].1)).unwrap().0, sum);
    }
    // Undo removes the copies and puts the files aside.
    let copy = absolute(j.root(), &files_of(&j, &r.imported[0])[1].1);
    j.undo().unwrap();
    assert!(!copy.exists());
    assert_eq!(j.conversions_needed().unwrap().len(), 4);
}

#[test]
fn avif_gets_a_jpeg_copy_beside_the_original() {
    find_tool().expect("FFmpeg is needed for this test (set MDP_FFMPEG)");
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (r, _) = j.import(&[fixtures().join("red-square.avif")], &JobContext::new()).unwrap();
    assert_eq!(r.imported.len(), 1);
    let original = absolute(j.root(), &files_of(&j, &r.imported[0])[0].1);
    let sum = checksum(&original).unwrap().0;
    let ctx = JobContext::new();
    assert_eq!(j.convert_all(&ctx).unwrap(), Some(1), "skipped: {:?}", ctx.skipped());
    let f = files_of(&j, &r.imported[0]);
    assert_eq!(f.len(), 2);
    assert_eq!((f[0].2.as_str(), f[1].0, f[1].2.as_str()), ("image/avif", 1, "image/jpeg"));
    assert!(std::fs::metadata(absolute(j.root(), &f[1].1)).unwrap().len() > 0);
    assert_eq!(checksum(&original).unwrap().0, sum);
}

#[test]
fn bmp_gets_a_png_copy_beside_the_original() {
    find_tool().expect("FFmpeg is needed for this test (set MDP_FFMPEG)");
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (r, _) = j.import(&[fixtures().join("blue-square.bmp")], &JobContext::new()).unwrap();
    let ctx = JobContext::new();
    assert_eq!(j.convert_all(&ctx).unwrap(), Some(1), "skipped: {:?}", ctx.skipped());
    let f = files_of(&j, &r.imported[0]);
    assert_eq!((f[0].2.as_str(), f[1].0, f[1].2.as_str()), ("image/bmp", 1, "image/png"));
    assert!(f[1].1.ends_with(".png"));
}

#[test]
fn dng_gets_its_stored_preview_as_the_copy() {
    find_tool().expect("FFmpeg is needed for this test (set MDP_FFMPEG)");
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (r, _) = j.import(&[fixtures().join("green-raw.dng")], &JobContext::new()).unwrap();
    let ctx = JobContext::new();
    assert_eq!(j.convert_all(&ctx).unwrap(), Some(1), "skipped: {:?}", ctx.skipped());
    let f = files_of(&j, &r.imported[0]);
    assert_eq!((f[0].2.as_str(), f[1].0, f[1].2.as_str()), ("image/x-adobe-dng", 1, "image/jpeg"));
    let copy = std::fs::read(absolute(j.root(), &f[1].1)).unwrap();
    assert_eq!(&copy[..2], &[0xFF, 0xD8]);
}

#[test]
fn documents_get_page_pictures_and_text() {
    find_tool().expect("FFmpeg is needed for this test (set MDP_FFMPEG)");
    mdp_core::documents::pdfium().expect("PDFium is needed for this test (set MDP_PDFIUM)");
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (r, _) = j.import(&[fixtures().join("hello-rowans.pdf"), fixtures().join("family-letter.docx")], &JobContext::new()).unwrap();
    let original = absolute(j.root(), &files_of(&j, &r.imported[0])[0].1);
    let sum = checksum(&original).unwrap().0;
    let ctx = JobContext::new();
    assert_eq!(j.convert_all(&ctx).unwrap(), Some(2), "skipped: {:?}", ctx.skipped());
    // The PDF: one page picture and its text; the original is unchanged.
    let f = files_of(&j, &r.imported[0]);
    let formats: Vec<&str> = f.iter().map(|x| x.2.as_str()).collect();
    assert_eq!(formats, ["application/pdf", "image/jpeg", "text/plain"]);
    assert!(f[1].1.ends_with("-page-001.jpg"));
    assert!(std::fs::read_to_string(absolute(j.root(), &f[2].1)).unwrap().contains("Hello Rowans"));
    assert_eq!(checksum(&original).unwrap().0, sum);
    // The Word file: its text always; page pictures only where Word or LibreOffice is installed.
    let f = files_of(&j, &r.imported[1]);
    let text = f.iter().find(|x| x.2 == "text/plain").unwrap();
    assert_eq!(std::fs::read_to_string(absolute(j.root(), &text.1)).unwrap(), "Dear June,\nThe farm is well.");
    // The export shows the PDF's page pictures and links the original.
    let mem = j.add_memory("Letters home", None).unwrap();
    j.add_to_memory(&mem, &[r.imported[0].clone()]).unwrap();
    let out = dir.path().join("export");
    j.export(&out, mdp_core::export::Privacy::default(), &JobContext::new()).unwrap();
    let html: String = walkdir(&out).iter().filter(|p| p.extension().is_some_and(|e| e == "html")).map(|p| std::fs::read_to_string(p).unwrap()).collect();
    assert!(html.contains("class=\"c-doc\"") && html.contains("alt=\"Page 1 of hello-rowans.pdf\""), "pages shown");
    assert!(html.contains(".pdf\">Original file</a>"));
}

#[test]
fn a_long_named_document_with_many_pages_exports() {
    find_tool().expect("FFmpeg is needed for this test (set MDP_FFMPEG)");
    mdp_core::documents::pdfium().expect("PDFium is needed for this test (set MDP_PDFIUM)");
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (r, _) = j.import(&[fixtures().join("Primary program song lyrics for the ward 2024.pdf")], &JobContext::new()).unwrap();
    j.convert_all(&JobContext::new()).unwrap();
    let mem = j.add_memory("Songs", None).unwrap();
    j.add_to_memory(&mem, &[r.imported[0].clone()]).unwrap();
    // The page pictures share a name cut short; this used to loop for ever (Jared, 2026-10-29).
    j.preview(mdp_core::export::Privacy::default()).unwrap();
    let out = dir.path().join("export");
    j.export(&out, mdp_core::export::Privacy::default(), &JobContext::new()).unwrap();
    let pics = walkdir(&out).iter().filter(|p| p.extension().is_some_and(|e| e == "jpg")).count();
    assert_eq!(pics, 2);
}

#[test]
fn thumbnails_are_made_in_the_cache() {
    find_tool().expect("FFmpeg is needed for this test (set MDP_FFMPEG)");
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (r, _) = j.import(&[fixtures().join("red-square.heic"), fixtures().join("hello.wav")], &JobContext::new()).unwrap();
    let t = mdp_core::convert::thumbnail(&j, &r.imported[0]).unwrap().expect("a preview");
    assert!(t.starts_with(j.root().join("cache")) && std::fs::metadata(&t).unwrap().len() > 0);
    assert!(mdp_core::convert::thumbnail(&j, &r.imported[1]).unwrap().is_none(), "no preview for a recording");
    assert_eq!(j.raw_content("waiting").unwrap().len(), 2);
}
