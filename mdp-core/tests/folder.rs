use std::path::Path;

use mdp_core::folder::{cloud_sync_warning, working_path, year_folder};
use mdp_core::{snapshot, Journey, Row};
use serde_json::json;

fn row(v: serde_json::Value) -> Row {
    v.as_object().unwrap().clone()
}

#[test]
fn new_journey_has_managed_folder_with_read_me() {
    let dir = tempfile::tempdir().unwrap();
    let j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let root = j.root();
    assert!(root.join("READ-ME.txt").is_file());
    let readme = std::fs::read_to_string(root.join("managed-content").join("READ-ME.txt")).unwrap();
    assert!(readme.contains("managed by the Modern Day Pioneer app"));
    assert!(readme.contains("Export"));
    assert!(root.join("snapshots").is_dir() && root.join("cache").is_dir());
}

#[test]
fn working_names_are_id_only_in_type_and_year_folders() {
    assert_eq!(working_path("picture", "k7f3m2qa", Some("1962-06-02"), "JPG"), "managed-content/photos/1962/k7f3m2qa.jpg");
    assert_eq!(working_path("voice recording", "abcd2345", Some("1985~"), ".wav"), "managed-content/audio/1985/abcd2345.wav");
    assert_eq!(working_path("video", "abcd2345", None, "mp4"), "managed-content/video/undated/abcd2345.mp4");
    assert_eq!(working_path("text", "abcd2345", Some(""), "txt"), "managed-content/text/undated/abcd2345.txt");
    assert_eq!(year_folder(Some("196X")), "undated");
    assert_eq!(year_folder(Some("12345")), "undated");
    assert_eq!(year_folder(Some("1913-06")), "1913");
}

#[test]
fn first_change_of_the_day_takes_a_snapshot_and_thirty_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let snaps = j.root().join("snapshots");
    let count = || std::fs::read_dir(&snaps).unwrap().count();
    assert_eq!(count(), 1, "today's snapshot");
    j.act(|a| a.create("family", row(json!({"name": "A"})))).unwrap();
    assert_eq!(count(), 1, "only one per day");
    // A snapshot holds the catalog as it was before that day's first change.
    snapshot::daily(j.root(), j.conn(), "2099-01-01").unwrap();
    let c = rusqlite::Connection::open(snaps.join("catalog-2099-01-01.sqlite")).unwrap();
    let n: i64 = c.query_row("SELECT count(*) FROM family", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    drop(c);
    std::fs::remove_file(snaps.join("catalog-2099-01-01.sqlite")).unwrap();
    for d in 1..=40 {
        snapshot::daily(j.root(), j.conn(), &format!("2025-01-{d:02}")).unwrap();
    }
    assert_eq!(count(), 30);
}

#[test]
fn cache_can_be_deleted_and_rebuilt_with_nothing_lost() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    j.act(|a| {
        let id = a.create("individual", row(json!({"kind": "Person", "name": "June Rowan"})))?;
        a.insert("person", row(json!({"id": id})))?;
        a.create("place", row(json!({"name": "Ogden, Utah"})))?;
        a.create("memory", row(json!({"name": "The kitchen table"})))
    })
    .unwrap();
    assert_eq!(j.search("jun", 10).unwrap().len(), 1);
    let before = std::fs::read_to_string(j.root().join("catalog.jsonl")).unwrap();
    let root = j.root().to_path_buf();
    drop(j);
    std::fs::remove_dir_all(root.join("cache")).unwrap();
    let mut j = Journey::open(&root).unwrap();
    assert_eq!(j.search("jun", 10).unwrap()[0].name, "June Rowan");
    assert_eq!(j.search("ogden", 10).unwrap()[0].kind, "place");
    assert_eq!(j.search("kitchen", 10).unwrap()[0].kind, "memory");
    // The list shown before anything is typed: only the kinds asked for, A to Z.
    let all = j.browse(&["person".into(), "place".into()], 10).unwrap();
    assert_eq!(all.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(), ["June Rowan", "Ogden, Utah"]);
    assert_eq!(std::fs::read_to_string(root.join("catalog.jsonl")).unwrap(), before);
    // Deleting it while the app is open is also survived.
    j.close_cache();
    std::fs::remove_dir_all(root.join("cache")).unwrap();
    assert_eq!(j.search("june rowan", 10).unwrap().len(), 1);
}

#[test]
fn cloud_synced_folders_get_a_warning() {
    for p in [
        r"C:\Users\Jared\OneDrive\Documents\Journeys\Rowans",
        r"C:\Users\Jared\OneDrive - Contoso\Rowans",
        r"C:\Users\Jared\Dropbox\Rowans",
        r"G:\My Drive\Rowans",
        r"C:\Users\Jared\iCloudDrive\Rowans",
    ] {
        assert!(cloud_sync_warning(Path::new(p)).is_some(), "{p}");
    }
    let w = cloud_sync_warning(Path::new(r"C:\Users\Jared\Dropbox\Rowans")).unwrap();
    assert!(w.contains("Dropbox"));
    for p in [r"C:\Users\Jared\Documents\Modern Day Pioneer\Rowans", r"D:\Family\Boxes of photos"] {
        assert!(cloud_sync_warning(Path::new(p)).is_none(), "{p}");
    }
}
