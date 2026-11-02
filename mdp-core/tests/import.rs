use std::path::{Path, PathBuf};

use mdp_core::import::{self, checksum};
use mdp_core::jobs::JobContext;
use mdp_core::Journey;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// A folder of originals the way a family might have them, with a subfolder
/// and a file the app does not import.
fn originals(dir: &Path) -> PathBuf {
    let src = dir.join("Pictures from Mom");
    std::fs::create_dir_all(src.join("1962 trip")).unwrap();
    for f in ["walter-camera.jpg", "no-exif.png", "hello.wav", "scrapbook-note.txt"] {
        let to = if f == "walter-camera.jpg" { src.join("1962 trip").join(f) } else { src.join(f) };
        std::fs::copy(fixtures().join(f), to).unwrap();
    }
    std::fs::write(src.join("budget.xlsx"), b"not media").unwrap();
    std::fs::write(src.join("Thumbs.db"), b"system").unwrap();
    src
}

fn fingerprint(dir: &Path) -> Vec<(PathBuf, String, std::time::SystemTime, bool)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let m = std::fs::metadata(&p).unwrap();
                out.push((p.clone(), checksum(&p).unwrap().0, m.modified().unwrap(), m.permissions().readonly()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn import_copies_and_never_touches_the_originals() {
    let dir = tempfile::tempdir().unwrap();
    let src = originals(dir.path());
    let before = fingerprint(&src);
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (report, skipped) = j.import(&[src.clone()], &JobContext::new()).unwrap();

    // same files, same checksums, same dates, nothing added or removed.
    assert_eq!(fingerprint(&src), before);
    assert_eq!(report.imported.len(), 4);
    assert_eq!(skipped.len(), 1, "only the spreadsheet is listed: {skipped:?}");
    assert!(skipped[0].item.ends_with("budget.xlsx"));

    // Each copy is in its working place, read-only, and matches the original.
    let rows: Vec<(String, String, String, i64)> = {
        let mut st = j.conn().prepare("SELECT c.kind, f.path, f.sha256, f.size FROM content c JOIN content_file f ON f.content_id = c.id ORDER BY f.path").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    assert_eq!(rows.len(), 4);
    for (_, path, sha, size) in &rows {
        let abs = mdp_core::folder::absolute(j.root(), path);
        assert!(std::fs::metadata(&abs).unwrap().permissions().readonly(), "{path} should be read-only");
        assert_eq!(checksum(&abs).unwrap(), (sha.clone(), *size as u64));
    }
    assert!(rows.iter().any(|r| r.0 == "picture" && r.1.starts_with("managed-content/photos/1962/")));
    assert!(rows.iter().any(|r| r.0 == "picture" && r.1.starts_with("managed-content/photos/undated/")));
    assert!(rows.iter().any(|r| r.0 == "voice recording" && r.1.starts_with("managed-content/audio/undated/")));
    assert!(rows.iter().any(|r| r.0 == "text" && r.1.starts_with("managed-content/text/undated/")));

    // Everything waits, and the holding folder is gone.
    let waiting: i64 = j.conn().query_row("SELECT count(*) FROM content WHERE state = 'waiting'", [], |r| r.get(0)).unwrap();
    assert_eq!(waiting, 4);
    let holding = j.root().join("managed-content").join(import::HOLDING_DIR);
    assert!(!holding.exists() || std::fs::read_dir(&holding).unwrap().next().is_none());

    // One action in history, and the plain-text copy names the file.
    let actions: i64 = j.conn().query_row("SELECT count(DISTINCT action_id) FROM history WHERE table_name = 'content_file'", [], |r| r.get(0)).unwrap();
    assert_eq!(actions, 1);
    let jsonl = std::fs::read_to_string(j.root().join("catalog.jsonl")).unwrap();
    assert!(jsonl.contains("walter-camera.jpg"));
}

#[test]
fn picture_details_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (report, _) = j.import(&[fixtures().join("walter-camera.jpg")], &JobContext::new()).unwrap();
    let id = &report.imported[0];
    let (date, w, h, lat, lon, camera): (String, i64, i64, f64, f64, String) = j
        .conn()
        .query_row(
            "SELECT c.date, p.width, p.height, p.latitude, p.longitude, p.camera FROM content c JOIN picture p ON p.id = c.id WHERE c.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap();
    assert_eq!(date, "1962-06-02");
    assert_eq!((w, h), (64, 48));
    assert!((lat - 41.223333).abs() < 1e-5, "{lat}");
    assert!((lon + 111.97).abs() < 1e-5, "{lon}");
    assert_eq!(camera, "Google Pixel 11 Pro XL");
    let name: String = j.conn().query_row("SELECT name FROM content WHERE id = ?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(name, "walter-camera.jpg");
}

#[test]
fn duplicates_are_listed_and_not_copied_twice() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::copy(fixtures().join("no-exif.png"), a.join("one.png")).unwrap();
    std::fs::copy(fixtures().join("no-exif.png"), a.join("same-again.png")).unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (first, _) = j.import(&[a.clone()], &JobContext::new()).unwrap();
    assert_eq!(first.imported.len(), 1);
    assert_eq!(first.duplicates.len(), 1, "the second copy in the same import");
    let (second, _) = j.import(&[a.join("one.png")], &JobContext::new()).unwrap();
    assert!(second.imported.is_empty());
    assert_eq!(second.duplicates[0].existing_id.as_deref(), Some(first.imported[0].as_str()));
    let files: i64 = j.conn().query_row("SELECT count(*) FROM content_file", [], |r| r.get(0)).unwrap();
    assert_eq!(files, 1);
}

#[test]
fn canceled_import_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let src = originals(dir.path());
    let j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let ctx = JobContext::new();
    ctx.cancel();
    assert!(matches!(import::prepare(j.root(), &[src], &ctx), Err(mdp_core::Error::Canceled)));
    let n: i64 = j.conn().query_row("SELECT count(*) FROM content", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    let holding = j.root().join("managed-content").join(import::HOLDING_DIR);
    assert!(!holding.exists() || std::fs::read_dir(&holding).unwrap().next().is_none());
}

#[test]
fn a_dropped_batch_removes_its_holding_folder() {
    let dir = tempfile::tempdir().unwrap();
    let src = originals(dir.path());
    let j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let batch = import::prepare(j.root(), &[src], &JobContext::new()).unwrap();
    assert_eq!(batch.items.len(), 4);
    assert!(batch.items.iter().all(|p| p.holding.exists()));
    drop(batch);
    let holding = j.root().join("managed-content").join(import::HOLDING_DIR);
    assert!(std::fs::read_dir(&holding).unwrap().next().is_none());
}

#[test]
fn a_missing_file_is_skipped_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (report, skipped) = j
        .import(&[dir.path().join("gone.jpg"), fixtures().join("hello.wav")], &JobContext::new())
        .unwrap();
    assert_eq!(report.imported.len(), 1);
    assert_eq!(skipped.len(), 1);
}

#[test]
fn a_format_1_0_catalog_is_upgraded_with_a_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("J");
    {
        let j = Journey::create(&folder, "Old").unwrap();
        j.conn()
            .execute_batch(
                "ALTER TABLE picture DROP COLUMN latitude; ALTER TABLE picture DROP COLUMN longitude; ALTER TABLE picture DROP COLUMN camera;
                 ALTER TABLE video DROP COLUMN latitude; ALTER TABLE video DROP COLUMN longitude; ALTER TABLE video DROP COLUMN camera;
                 UPDATE journey SET format_version = '1.0'; PRAGMA user_version = 100;",
            )
            .unwrap();
    }
    let mut j = Journey::open(&folder).unwrap();
    let v: i64 = j.conn().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(v, 105);
    assert!(folder.join("snapshots").join("catalog-before-format-1.5.sqlite").is_file());
    let fv: String = j.conn().query_row("SELECT format_version FROM journey", [], |r| r.get(0)).unwrap();
    assert_eq!(fv, "1.5");
    assert_eq!(j.undo_depth(), 0);
    let (report, _) = j.import(&[fixtures().join("walter-camera.jpg")], &JobContext::new()).unwrap();
    assert_eq!(report.imported.len(), 1);
}
