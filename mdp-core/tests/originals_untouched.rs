//! Proof that the app never changes, moves, adds to or removes the family's
//! original files. One folder of originals goes through every way the app
//! reads files: a plain import, a folder import with Google Takeout
//! sidecars, Takeout zip files read where they are, previews before an
//! import, the file check, an export and a backup. Before and after, every
//! file and folder under the originals is fingerprinted: SHA-256, size,
//! modified time and read-only mark. The originals start read-only, the way
//! a copied CD or a protected folder would be, and must stay that way.

use std::path::{Path, PathBuf};

use mdp_core::backup::backup;
use mdp_core::bulk::{self, ScanOptions};
use mdp_core::export::Privacy;
use mdp_core::import::checksum;
use mdp_core::jobs::JobContext;
use mdp_core::{preview, Journey};
use serde_json::json;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn zip_of(path: &Path, files: &[(&str, Vec<u8>)]) {
    let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in files {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut w, bytes).unwrap();
    }
    w.finish().unwrap();
}

/// The originals: loose files, a subfolder, a Takeout folder with sidecars
/// and two Takeout zip parts.
fn originals(root: &Path) -> PathBuf {
    let src = root.join("Originals");
    let trip = src.join("1962 trip");
    std::fs::create_dir_all(&trip).unwrap();
    for (f, to) in [
        ("walter-camera.jpg", &trip),
        ("home-gps.jpg", &src),
        ("no-exif.png", &src),
        ("hello.wav", &src),
        ("scrapbook-note.txt", &src),
        ("family-letter.docx", &src),
    ] {
        std::fs::copy(fixtures().join(f), to.join(f)).unwrap();
    }
    let album = src.join("Takeout/Google Photos/Photos from 2019");
    std::fs::create_dir_all(&album).unwrap();
    std::fs::copy(fixtures().join("walter-camera.jpg"), album.join("IMG_0001.jpg")).unwrap();
    std::fs::write(
        album.join("IMG_0001.jpg.json"),
        serde_json::to_vec(&json!({"title": "IMG_0001.jpg", "description": "On the porch",
            "photoTakenTime": {"timestamp": "1562846400"}, "people": [{"name": "June Rowan"}]}))
        .unwrap(),
    )
    .unwrap();
    let zips = src.join("Downloads");
    std::fs::create_dir_all(&zips).unwrap();
    let pic = std::fs::read(fixtures().join("no-exif.png")).unwrap();
    let side = serde_json::to_vec(&json!({"title": "IMG_0002.png", "description": "At the lake",
        "photoTakenTime": {"timestamp": "1562846400"}})).unwrap();
    zip_of(&zips.join("takeout-001.zip"), &[("Takeout/Google Photos/Lake/IMG_0002.png", pic)]);
    zip_of(&zips.join("takeout-002.zip"), &[("Takeout/Google Photos/Lake/IMG_0002.png.json", side)]);
    src
}

fn all_paths(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![dir.to_path_buf()];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p.clone());
            }
            out.push(p);
        }
    }
    out.sort();
    out
}

fn set_readonly(dir: &Path, on: bool) {
    for p in all_paths(dir).into_iter().filter(|p| p.is_file()) {
        let mut perm = std::fs::metadata(&p).unwrap().permissions();
        perm.set_readonly(on);
        std::fs::set_permissions(&p, perm).unwrap();
    }
}

/// Every file and folder: path, SHA-256 (files), size, modified time, read-only.
fn fingerprint(dir: &Path) -> Vec<(PathBuf, String, u64, std::time::SystemTime, bool)> {
    all_paths(dir)
        .into_iter()
        .map(|p| {
            let m = std::fs::metadata(&p).unwrap();
            let sha = if m.is_file() { checksum(&p).unwrap().0 } else { "folder".into() };
            let modified = if m.is_file() { m.modified().unwrap() } else { std::time::UNIX_EPOCH };
            (p.clone(), sha, m.len() * m.is_file() as u64, modified, m.permissions().readonly())
        })
        .collect()
}

#[test]
fn originals_are_never_changed_by_any_way_the_app_reads_them() {
    let t = tempfile::tempdir().unwrap();
    let src = originals(t.path());
    set_readonly(&src, true);
    let before = fingerprint(&src);
    assert!(before.iter().filter(|f| f.1 != "folder").count() >= 10, "{before:#?}");

    let mut j = Journey::create(&t.path().join("journey"), "Proof").unwrap();
    j.add_individual("person", "June Rowan").unwrap();
    let ctx = JobContext::new();

    // 1. Plain import of loose files and a subfolder.
    let loose: Vec<PathBuf> = ["home-gps.jpg", "no-exif.png", "hello.wav", "scrapbook-note.txt", "family-letter.docx", "1962 trip"]
        .iter()
        .map(|f| src.join(f))
        .collect();
    let (report, _) = j.import(&loose, &ctx).unwrap();
    assert!(report.imported.len() >= 5, "{report:#?}");

    // 2. Previews read the originals before anything is brought in.
    preview::thumbnail(j.root(), &src.join("home-gps.jpg")).unwrap();
    preview::first_lines(j.root(), &src.join("scrapbook-note.txt")).unwrap();
    preview::first_lines(j.root(), &src.join("family-letter.docx")).unwrap();

    // 3. Folder import with Google Takeout sidecars.
    let plan = bulk::scan(&j, &src.join("Takeout"), ScanOptions { story_per_folder: true, ..ScanOptions::default() }, &ctx).unwrap();
    assert_eq!(plan.rows.len(), 1, "{:#?}", plan.rows);
    let batch = mdp_core::import::prepare(j.root(), &bulk::plan_files(&plan), &ctx).unwrap();
    j.finish_plan(batch, &plan, true).unwrap();

    // 4. Takeout zips read where they are (previews come out of the zip too).
    let zips = [src.join("Downloads/takeout-001.zip"), src.join("Downloads/takeout-002.zip")];
    let known = bulk::Known::read(&j).unwrap();
    let plan = bulk::scan_zips(&known, j.root(), &zips, ScanOptions::default(), &ctx).unwrap();
    assert_eq!(plan.rows.len(), 1, "{:#?}", plan.rows);
    let files = bulk::plan_files(&plan);
    for f in &files {
        preview::thumbnail(j.root(), f).unwrap();
    }
    let batch = mdp_core::import::prepare(j.root(), &files, &ctx).unwrap();
    j.finish_plan(batch, &plan, true).unwrap();

    // 5. File check, export and backup.
    j.check_files(&ctx).unwrap();
    j.export(&t.path().join("export"), Privacy::default(), &ctx).unwrap();
    backup(j.root(), &t.path().join("proof.mdp-backup.zip"), &ctx).unwrap();

    // Nothing under the originals changed, moved, appeared or went away.
    let after = fingerprint(&src);
    let changed: Vec<_> = before.iter().zip(after.iter()).filter(|(a, b)| a != b).collect();
    assert_eq!(before.len(), after.len(), "files appeared or went away:\nbefore {before:#?}\nafter {after:#?}");
    assert!(changed.is_empty(), "originals changed: {changed:#?}");

    set_readonly(&src, false); // so the temporary folder can be removed on Windows
}
