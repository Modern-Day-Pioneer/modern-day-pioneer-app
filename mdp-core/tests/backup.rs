use std::path::Path;

use mdp_core::backup::{backup, restore, sha256_file, suggested_name};
use mdp_core::jobs::{spawn, JobContext, Outcome};
use mdp_core::{folder, Journey, Row};
use serde_json::json;

fn row(v: serde_json::Value) -> Row {
    v.as_object().unwrap().clone()
}

/// A small sample Journey with catalog records and two working files.
fn sample(root: &Path) -> Journey {
    let mut j = Journey::create(root, "The Rowans").unwrap();
    j.act(|a| {
        let june = a.create("individual", row(json!({"kind": "Person", "name": "June Rowan", "birth_date_edtf": "1931"})))?;
        a.insert("person", row(json!({"id": june})))?;
        let m = a.create("memory", row(json!({"name": "The kitchen table", "caption": "Sunday dinners"})))?;
        for (kind, ext, date, bytes) in [("picture", "jpg", Some("1962-06-02"), vec![0xFFu8; 70_000]), ("voice recording", "wav", None, b"RIFF....WAVE".to_vec())] {
            let c = a.create("content", row(json!({"kind": kind, "name": format!("file.{ext}"), "date": date, "state": "in a memory"})))?;
            let path = folder::working_path(kind, &c, date, ext);
            let abs = folder::absolute(a.conn().path().map(|p| Path::new(p).parent().unwrap().to_path_buf()).unwrap().as_path(), &path);
            std::fs::create_dir_all(abs.parent().unwrap())?;
            std::fs::write(&abs, &bytes)?;
            let size = bytes.len() as i64;
            let sha = sha256_file(&abs)?;
            a.insert("content_file", row(json!({"content_id": c, "is_copy": 0, "path": path, "format": "application/octet-stream", "size": size, "sha256": sha})))?;
            a.insert("memory_content", row(json!({"memory_id": m, "content_id": c, "position": 0})))?;
        }
        Ok(())
    })
    .unwrap();
    j
}

fn dump(conn: &rusqlite::Connection) -> Vec<String> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    let mut out = vec![];
    for t in tables {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {t}")).unwrap();
        let n = stmt.column_count();
        let rows = stmt
            .query_map([], |r| Ok((0..n).map(|i| format!("{:?}", r.get_ref(i).unwrap())).collect::<Vec<_>>().join("|")))
            .unwrap();
        out.extend(rows.map(|r| format!("{t}|{}", r.unwrap())));
    }
    out.sort();
    out
}

fn files_with_sums(root: &Path) -> Vec<(String, String)> {
    let mut out = vec![];
    for e in walk(&root.join("managed-content")) {
        out.push((e.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), sha256_file(&e).unwrap()));
    }
    out.sort();
    out
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut v = vec![];
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            v.extend(walk(&p));
        } else {
            v.push(p);
        }
    }
    v
}

#[test]
fn backup_and_restore_match_by_checksum() {
    let dir = tempfile::tempdir().unwrap();
    let j = sample(&dir.path().join("Rowans"));
    let archive = dir.path().join(suggested_name("The Rowans"));
    assert!(archive.to_string_lossy().ends_with(".mdp-backup.zip"));
    // The name carries the date and time (no colons, so Windows accepts it).
    let stamp = archive.file_name().unwrap().to_string_lossy().trim_start_matches("The Rowans_").trim_end_matches(".mdp-backup.zip").to_string();
    assert!(chrono::NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%d_%H-%M-%S").is_ok(), "{stamp}");
    let root = j.root().to_path_buf();
    let a = archive.clone();
    // Runs as a background job while the Journey is still open.
    let job = spawn(move |ctx| backup(&root, &a, ctx));
    let Outcome::Finished { .. } = job.wait() else { panic!("backup failed") };
    assert!(archive.is_file());

    let restored = dir.path().join("Restored");
    restore(&archive, &restored, &JobContext::new()).unwrap();
    let r = Journey::open(&restored).unwrap();
    assert_eq!(r.id(), j.id());
    assert_eq!(dump(r.conn()), dump(j.conn()), "same catalog");
    assert_eq!(files_with_sums(&restored), files_with_sums(j.root()), "same files");
    // Every working file still matches the checksum recorded in the catalog.
    let mut stmt = r.conn().prepare("SELECT path, sha256 FROM content_file").unwrap();
    for row in stmt.query_map([], |x| Ok((x.get::<_, String>(0)?, x.get::<_, String>(1)?))).unwrap() {
        let (p, sum) = row.unwrap();
        assert_eq!(sha256_file(&folder::absolute(&restored, &p)).unwrap(), sum);
        assert!(std::fs::metadata(folder::absolute(&restored, &p)).unwrap().permissions().readonly());
    }
    assert!(restored.join("catalog.jsonl").is_file() && restored.join("READ-ME.txt").is_file());
}

#[test]
fn restore_refuses_a_folder_in_use_and_a_damaged_archive() {
    let dir = tempfile::tempdir().unwrap();
    let j = sample(&dir.path().join("Rowans"));
    let archive = dir.path().join("b.zip");
    backup(j.root(), &archive, &JobContext::new()).unwrap();
    assert!(restore(&archive, j.root(), &JobContext::new()).is_err(), "never restores over a project");

    // Flip bytes inside the archive's stored picture data.
    let mut bytes = std::fs::read(&archive).unwrap();
    let at = bytes.windows(8).position(|w| w == [0xFF; 8]).unwrap();
    bytes[at + 100] = 0x00;
    let bad = dir.path().join("bad.zip");
    std::fs::write(&bad, bytes).unwrap();
    let target = dir.path().join("Restored");
    assert!(restore(&bad, &target, &JobContext::new()).is_err());
    assert!(!target.exists());
    let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert!(leftovers.iter().all(|n| !n.to_string_lossy().contains(".partial-")), "{leftovers:?}");
}

#[test]
fn a_canceled_backup_leaves_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let j = sample(&dir.path().join("Rowans"));
    let archive = dir.path().join("b.zip");
    let ctx = JobContext::new();
    ctx.cancel();
    assert!(matches!(backup(j.root(), &archive, &ctx), Err(mdp_core::Error::Canceled)));
    let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["Rowans".to_string()]);
}

/// Sprint 7: restore reads the backup's name and date before it starts.
#[test]
fn a_backup_describes_itself() {
    let dir = tempfile::tempdir().unwrap();
    let j = sample(&dir.path().join("Rowans"));
    let zip = dir.path().join(mdp_core::backup::suggested_name("The Rowans"));
    mdp_core::backup::backup(j.root(), &zip, &mdp_core::jobs::JobContext::new()).unwrap();
    let (name, created) = mdp_core::backup::describe(&zip).unwrap();
    assert_eq!(name, j.name().unwrap());
    assert!(created.starts_with("20"));
    let not = dir.path().join("not-a-backup.zip");
    std::fs::write(&not, b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0").unwrap();
    assert!(mdp_core::backup::describe(&not).is_err());
}

#[test]
fn restore_fills_an_empty_folder_the_person_chose_without_removing_it() {
    // On Windows a folder just made in the folder picker or File Explorer is
    // often still held open, so restore must fill it in place (Jared, 0.7.30:
    // "Access is denied" restoring into F:\Test Journeys\test\My Journey).
    let dir = tempfile::tempdir().unwrap();
    let j = sample(&dir.path().join("Rowans"));
    let archive = dir.path().join(suggested_name("The Rowans"));
    backup(j.root(), &archive, &JobContext::new()).unwrap();
    let chosen = dir.path().join("test").join("My Journey");
    std::fs::create_dir_all(&chosen).unwrap();
    let held = std::fs::File::open(&chosen).ok(); // a handle on the folder, as Explorer keeps
    assert_eq!(restore(&archive, &chosen, &JobContext::new()).unwrap(), chosen);
    drop(held);
    assert_eq!(files_with_sums(&chosen), files_with_sums(j.root()), "same files");
    assert_eq!(Journey::open(&chosen).unwrap().id(), j.id());
    let left: Vec<_> = std::fs::read_dir(chosen.parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left.len(), 1, "no partial folder left beside it: {left:?}");
}

/// Copies removed from the Journey wait in content/removed only for Ctrl+Z;
/// a backup leaves them out, so removing items makes the next backup smaller.
#[test]
fn a_backup_leaves_out_removed_copies() {
    let dir = tempfile::tempdir().unwrap();
    let j = sample(&dir.path().join("Rowans"));
    let removed = j.root().join(folder::CONTENT_DIR).join(mdp_core::files::REMOVED_DIR);
    std::fs::create_dir_all(&removed).unwrap();
    std::fs::write(removed.join("gone.jpg"), vec![0u8; 50_000]).unwrap();
    let archive = dir.path().join("b.zip");
    backup(j.root(), &archive, &JobContext::new()).unwrap();
    let zip = zip::ZipArchive::new(std::fs::File::open(&archive).unwrap()).unwrap();
    let names: Vec<_> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    assert!(names.iter().any(|n| n.starts_with(folder::CONTENT_DIR)), "{names:?}");
    assert!(!names.iter().any(|n| n.contains("removed")), "{names:?}");
}
