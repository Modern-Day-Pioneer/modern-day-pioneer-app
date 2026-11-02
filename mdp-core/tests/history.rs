use mdp_core::{id_key, jsonl, Journey, Row};
use serde_json::{json, Value};

fn row(v: Value) -> Row {
    v.as_object().unwrap().clone()
}

/// Every table except history, without `updated`, as sorted text: the catalog's state.
fn dump(j: &Journey) -> Vec<String> {
    let conn = j.conn();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT IN ('history','sqlite_sequence') ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    let mut out = vec![];
    for t in tables {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {t}")).unwrap();
        let cols: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
        let rows = stmt
            .query_map([], |r| {
                let mut s = t.clone();
                for (i, c) in cols.iter().enumerate() {
                    if c != "updated" {
                        s.push_str(&format!("|{c}={:?}", r.get_ref(i)?));
                    }
                }
                Ok(s)
            })
            .unwrap();
        out.extend(rows.map(|r| r.unwrap()));
    }
    out.sort();
    out
}

fn new_journey() -> (tempfile::TempDir, Journey) {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "The Rowans").unwrap();
    j.set_author("Ellen");
    (dir, j)
}

fn add_person(j: &mut Journey, name: &str) -> String {
    j.act(|a| {
        let id = a.create("individual", row(json!({"kind": "Person", "name": name})))?;
        a.insert("person", row(json!({"id": id})))?;
        Ok(id)
    })
    .unwrap()
}

#[test]
fn each_change_writes_history_with_action_time_author_and_values() {
    let (_d, mut j) = new_journey();
    let id = add_person(&mut j, "June");
    j.act(|a| a.update("individual", id_key(&id), "name", json!("June Rowan"))).unwrap();
    let h = j.history_all().unwrap();
    let last = h.last().unwrap();
    assert_eq!(last.author, "Ellen");
    assert_eq!(last.table_name, "individual");
    assert_eq!(last.item_id, id);
    assert_eq!(last.field.as_deref(), Some("name"));
    assert_eq!(last.old_value, Some(json!("June")));
    assert_eq!(last.new_value, Some(json!("June Rowan")));
    assert!(!last.at.is_empty() && !last.action_id.is_empty());
    // The person's creation was one action of three rows: item, individual, person.
    let create = h.iter().find(|e| e.table_name == "person").unwrap();
    assert_eq!(j.history_of(&create.action_id).unwrap().len(), 3);
}

#[test]
fn plain_text_copy_is_rewritten_and_matches_the_catalog() {
    let (_d, mut j) = new_journey();
    let june = add_person(&mut j, "June");
    let fam = j
        .act(|a| {
            let f = a.create("family", row(json!({"name": "The Rowans"})))?;
            a.insert("family_member", row(json!({"family_id": f, "individual_id": june})))?;
            Ok(f)
        })
        .unwrap();
    let text = std::fs::read_to_string(j.root().join("catalog.jsonl")).unwrap();
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines, jsonl::lines(j.conn()).unwrap(), "file is the current catalog");
    assert_eq!(lines[0], json!({"@context": "context.jsonld"}));
    assert_eq!(lines[1]["@type"], "Journey");
    assert_eq!(lines[1]["name"], "The Rowans");
    let items: i64 = j.conn().query_row("SELECT count(*) FROM item", [], |r| r.get(0)).unwrap();
    assert_eq!(lines.len() as i64, items + 1, "one line per item plus the context");
    let family = lines.iter().find(|l| l["@id"] == format!("mdpid:{fam}")).unwrap();
    assert_eq!(family["member"], json!([format!("mdpid:{june}")]));
    // Fixed order: items after the Journey are sorted by ID.
    let ids: Vec<&str> = lines[2..].iter().map(|l| l["@id"].as_str().unwrap()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    // An update shows up straight away.
    j.act(|a| a.update("family", id_key(&fam), "description", json!("Mom's side"))).unwrap();
    let text = std::fs::read_to_string(j.root().join("catalog.jsonl")).unwrap();
    assert!(text.contains("Mom's side"));
    assert!(j.root().join("context.jsonld").is_file());
}

#[test]
fn a_deferred_copy_catches_up_when_written_and_on_the_next_open() {
    let (_d, mut j) = new_journey();
    j.defer_copy(true);
    let june = add_person(&mut j, "June");
    let file = j.root().join("catalog.jsonl");
    let behind = j.root().join(mdp_core::journey::COPY_BEHIND_FILE);
    assert!(!std::fs::read_to_string(&file).unwrap().contains("June"), "the copy waits for a pause");
    assert!(behind.exists());
    assert!(j.copy_due(std::time::Duration::ZERO));
    j.write_copy().unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains("June"));
    assert!(!behind.exists() && !j.copy_due(std::time::Duration::ZERO));
    // A crash before the copy is written: the next open catches it up.
    j.act(|a| a.update("individual", id_key(&june), "description", json!("Kept a scrapbook"))).unwrap();
    let root = j.root().to_path_buf();
    std::mem::forget(j);
    assert!(!std::fs::read_to_string(&file).unwrap().contains("scrapbook"));
    let j = Journey::open(&root).unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains("scrapbook"));
    assert!(!behind.exists());
    drop(j);
}

#[test]
fn undo_reverses_a_whole_multi_record_action() {
    let (_d, mut j) = new_journey();
    let june = add_person(&mut j, "June");
    let before = dump(&j);
    let jsonl_before = std::fs::read_to_string(j.root().join("catalog.jsonl")).unwrap();
    j.act(|a| {
        let walter = a.create("individual", row(json!({"kind": "Person", "name": "Walter"})))?;
        a.insert("person", row(json!({"id": walter, "given_name": "Walter"})))?;
        let f = a.create("family", row(json!({"name": "The Rowans"})))?;
        a.insert("family_member", row(json!({"family_id": f, "individual_id": june})))?;
        a.insert("family_member", row(json!({"family_id": f, "individual_id": walter})))?;
        a.update("individual", id_key(&june), "description", json!("Kept a scrapbook"))?;
        Ok(())
    })
    .unwrap();
    assert_ne!(dump(&j), before);
    assert!(j.undo().unwrap());
    assert_eq!(dump(&j), before);
    assert_eq!(std::fs::read_to_string(j.root().join("catalog.jsonl")).unwrap(), jsonl_before);
}

#[test]
fn undo_goes_back_to_when_the_journey_was_opened_and_no_further() {
    let (d, mut j) = new_journey();
    let start = dump(&j);
    let a = add_person(&mut j, "A");
    add_person(&mut j, "B");
    j.act(|x| x.update("individual", id_key(&a), "name", json!("A2"))).unwrap();
    j.act(|x| x.delete_item("individual", &a)).unwrap();
    assert_eq!(j.undo_depth(), 4);
    while j.undo().unwrap() {}
    assert_eq!(dump(&j), start);
    assert!(!j.undo().unwrap(), "creating the Journey is not undoable");

    // Undo is per session: after reopening there is nothing to undo.
    add_person(&mut j, "C");
    let root = j.root().to_path_buf();
    drop(j);
    let mut j = Journey::open(&root).unwrap();
    assert_eq!(j.undo_depth(), 0);
    assert!(!j.undo().unwrap());
    drop(d);
}

#[test]
fn delete_and_undo_restore_the_record() {
    let (_d, mut j) = new_journey();
    let june = add_person(&mut j, "June");
    let before = dump(&j);
    j.act(|a| a.delete_item("individual", &june)).unwrap();
    let gone: i64 = j.conn().query_row("SELECT count(*) FROM individual", [], |r| r.get(0)).unwrap();
    assert_eq!(gone, 0);
    j.undo().unwrap();
    assert_eq!(dump(&j), before);
}

#[test]
fn a_failed_action_changes_nothing() {
    let (_d, mut j) = new_journey();
    let before = dump(&j);
    let seq_before = j.history_all().unwrap().len();
    let r = j.act(|a| {
        a.create("family", row(json!({"name": "Half done"})))?;
        a.create("family", row(json!({"name": ""}))) // refused by the schema
    });
    assert!(r.is_err());
    assert_eq!(dump(&j), before);
    assert_eq!(j.history_all().unwrap().len(), seq_before);
    assert_eq!(j.undo_depth(), 0);
}

#[test]
fn unknown_tables_and_fields_are_refused() {
    let (_d, mut j) = new_journey();
    assert!(j.act(|a| a.insert("history", Row::new())).is_err());
    assert!(j.act(|a| a.create("family", row(json!({"name": "x", "nope": 1})))).is_err());
    assert!(j.act(|a| a.insert("family; DROP TABLE item", Row::new())).is_err());
}
