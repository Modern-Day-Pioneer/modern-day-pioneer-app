use mdp_core::{sample, Journey};

#[test]
fn sample_family_fills_the_home_and_search() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = sample::create(&dir.path().join("Sample")).unwrap();
    let h = j.home().unwrap();
    assert_eq!(h.name, sample::NAME);
    assert_eq!((h.counts.stories, h.counts.memories, h.counts.people, h.counts.pets, h.counts.families), (2, 4, 3, 1, 1));
    assert_eq!(h.featured.len(), 2);
    assert!(h.to_finish.iter().any(|e| e.name == "Walter and June's wedding day" && e.note.contains("The Kitchen Table")));
    assert_eq!(j.undo_depth(), 0, "the sample is not undoable");
    // Pictures and a recording come in through the normal import.
    assert_eq!(h.counts.waiting, 1);
    assert_eq!(j.raw_content("set aside").unwrap().len(), 1);
    let files: i64 = j.conn().query_row("SELECT count(*) FROM content_file", [], |r| r.get(0)).unwrap();
    assert_eq!(files, 4);
    // Search finds people, families, places, stories and memories by name or title.
    for (q, kind) in [("walt", "person"), ("pepper", "pet"), ("rowans", "family"), ("ogden", "place"), ("kitchen table", "story"), ("sunday", "memory")] {
        let hits = j.search(q, 10).unwrap();
        assert!(hits.iter().any(|h| h.kind == kind), "{q} -> {hits:?}");
    }
    let groups = j.people_by_family().unwrap();
    assert_eq!(groups[0].members.len(), 4);
    assert_eq!(j.stories().unwrap().len(), 2);
    let walter = &j.search("walter rowan", 1).unwrap()[0];
    let item = j.item(&walter.id).unwrap().unwrap();
    assert_eq!(item["givenName"], "Walter");
}

#[test]
fn an_empty_journey_has_an_empty_home() {
    let dir = tempfile::tempdir().unwrap();
    let j = Journey::create(&dir.path().join("J"), "Mine").unwrap();
    let h = j.home().unwrap();
    assert_eq!(h.counts, Default::default());
    assert!(h.to_finish.is_empty() && h.featured.is_empty());
    assert!(j.people_by_family().unwrap().is_empty());
}

#[test]
fn the_waiting_list_holds_every_waiting_item() {
    // Jared, 2026-11-12: after a big import the Waiting tab said 2000 while the
    // home page said 5790. The Waiting screen now gets every item and pages them.
    let dir = tempfile::tempdir().unwrap();
    let j = Journey::create(&dir.path().join("J"), "Mine").unwrap();
    let n = 2345;
    j.conn()
        .execute_batch(&format!(
            "WITH RECURSIVE k(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM k WHERE i < {n})
             INSERT INTO item (id, table_name) SELECT 'w' || i, 'content' FROM k;
             INSERT INTO content (id, kind, name, updated) SELECT id, 'picture', 'IMG_' || id || '.jpg', '2026-11-12' FROM item WHERE table_name = 'content';"
        ))
        .unwrap();
    assert_eq!(j.home().unwrap().counts.waiting, n as i64);
    assert_eq!(j.raw_content("waiting").unwrap().len(), n);
}
