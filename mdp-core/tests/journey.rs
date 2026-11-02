use mdp_core::{Journey, OpenMode};

#[test]
fn create_makes_folder_catalog_and_version() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("The Rowans");
    let j = Journey::create(&folder, "  The Rowans ").unwrap();
    assert!(folder.join("catalog.sqlite").is_file());
    assert_eq!(j.name().unwrap(), "The Rowans");
    assert!(mdp_core::ids::is_valid(j.id()));
    let v: i64 = j.conn().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(v, 105);
    // Every field-list table exists.
    for t in [
        "journey", "featured", "individual", "person", "pet", "individual_relationship", "family",
        "family_member", "family_relationship", "place", "event", "event_individual", "story",
        "story_memory", "memory", "memory_content", "memory_individual", "content", "picture",
        "voice_recording", "video", "text_content", "content_file", "content_relationship", "history",
    ] {
        let n: i64 = j
            .conn()
            .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [t], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "missing table {t}");
    }
}

#[test]
fn empty_name_and_non_empty_folder_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Journey::create(&dir.path().join("a"), "   ").is_err());
    std::fs::write(dir.path().join("x.txt"), "x").unwrap();
    assert!(Journey::create(dir.path(), "B").is_err());
}

#[test]
fn reopen_after_close_and_after_moving_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("drive-c").join("J");
    let id = {
        let j = Journey::create(&a, "June's Journey").unwrap();
        j.id().to_string()
    };
    let j = Journey::open(&a).unwrap();
    assert_eq!(j.id(), id);
    drop(j);
    let b = dir.path().join("drive-d").join("Moved");
    std::fs::create_dir_all(b.parent().unwrap()).unwrap();
    std::fs::rename(&a, &b).unwrap();
    let j = Journey::open(&b).unwrap();
    assert_eq!(j.id(), id);
    assert_eq!(j.name().unwrap(), "June's Journey");
    assert_eq!(j.mode(), OpenMode::ReadWrite);
}

#[test]
fn newer_major_version_opens_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("J");
    {
        let j = Journey::create(&f, "J").unwrap();
        j.conn().pragma_update(None, "user_version", 200).unwrap();
    }
    let j = Journey::open(&f).unwrap();
    assert_eq!(j.mode(), OpenMode::ReadOnly);
    assert!(j.conn().execute("UPDATE journey SET name = 'x'", []).is_err());
}

#[test]
fn ids_are_unique_and_well_formed() {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..10_000 {
        let id = mdp_core::ids::new_id();
        assert!(mdp_core::ids::is_valid(&id));
        seen.insert(id);
    }
    assert!(seen.len() > 9_990);
}
