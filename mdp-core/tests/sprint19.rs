//! Sprint 19 (combined with sprint 18): kinds of event the family adds
//! (catalog format 1.6), an optional checksum list in the
//! export (BagIt manifest), and pages that print cleanly.

use std::collections::{HashMap, HashSet};

use mdp_core::export::{check_manifest, Privacy, MANIFEST_FILE};
use mdp_core::jobs::JobContext;
use mdp_core::{rebuild, Journey};

fn first_person(j: &Journey) -> String {
    j.conn().query_row("SELECT id FROM individual WHERE kind <> 'Pet' ORDER BY id LIMIT 1", [], |r| r.get(0)).unwrap()
}

fn kind_of(j: &Journey, id: &str) -> String {
    j.conn().query_row("SELECT kind FROM event WHERE id = ?1", [id], |r| r.get(0)).unwrap()
}

#[test]
fn a_family_adds_its_own_kinds_of_event() {
    let t = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&t.path().join("j"), "Test").unwrap();
    let ada = j.add_individual("Person", "Ada Rowan").unwrap();

    // A new kind is kept as typed, and offered again.
    let mission = j.add_event("  Mission ", None, std::slice::from_ref(&ada), "1972", None).unwrap();
    assert_eq!(kind_of(&j, &mission), "Mission");
    assert_eq!(j.conn().query_row::<String, _, _>("SELECT name FROM event WHERE id = ?1", [&mission], |r| r.get(0)).unwrap(), "Mission of Ada Rowan");
    let kinds = j.event_kinds().unwrap();
    assert_eq!(kinds.len(), mdp_core::places::EVENT_KINDS.len() + 1);
    let added: Vec<_> = kinds.iter().filter(|k| k.added).collect();
    assert_eq!((added[0].key.as_str(), added[0].events), ("Mission", 1));

    // The same kind typed another way stays one kind; a fixed kind by name is the fixed one.
    let again = j.add_event("mission", Some("Second mission"), &[], "", None).unwrap();
    assert_eq!(kind_of(&j, &again), "Mission");
    let wed = j.add_event("Wedding", None, &[], "", None).unwrap();
    assert_eq!(kind_of(&j, &wed), "wedding");
    assert!(j.add_event("   ", None, &[], "", None).is_err());
    assert!(j.add_event(&"x".repeat(41), None, &[], "", None).is_err());

    // Changing an event to another new kind.
    let mut ch = mdp_core::Row::new();
    ch.insert("kind".into(), "Baptism".into());
    j.update_event(&again, &ch).unwrap();
    assert_eq!(kind_of(&j, &again), "Baptism");
    let labels: Vec<String> = j.event_kinds().unwrap().into_iter().filter(|k| k.added).map(|k| k.label).collect();
    assert_eq!(labels, ["Baptism", "Mission"]);
    // Undo takes the change back.
    j.undo().unwrap();
    assert_eq!(kind_of(&j, &again), "Mission");
}

#[test]
fn added_kinds_go_out_in_the_export_and_come_back_in() {
    let t = tempfile::tempdir().unwrap();
    let mut j = mdp_core::sample::create(&t.path().join("sample")).unwrap();
    let who = first_person(&j);
    let ev = j.add_event("Homecoming", Some("Homecoming 1974"), std::slice::from_ref(&who), "1974", None).unwrap();
    let out = t.path().join("export");
    j.export(&out, Privacy::default(), &JobContext::new()).unwrap();
    let cat = std::fs::read_to_string(out.join("catalog.json")).unwrap();
    assert!(cat.contains("\"Homecoming\""));

    let mut fresh = Journey::create(&t.path().join("fresh"), "Fresh").unwrap();
    let eb = rebuild::prepare(fresh.root(), &out, &HashSet::new(), &JobContext::new()).unwrap();
    let report = fresh.finish_export(eb, &HashMap::new()).unwrap();
    assert!(!report.notes.iter().any(|n| n.contains("Homecoming")), "{:?}", report.notes);
    assert_eq!(kind_of(&fresh, &ev), "Homecoming", "the export keeps IDs");
}

#[test]
fn a_journey_from_format_1_5_keeps_its_events_and_takes_new_kinds() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("old");
    let mut j = mdp_core::sample::create(&root).unwrap();
    let before: Vec<(String, String, String)> = {
        let mut st = j.conn().prepare("SELECT id, kind, name FROM event ORDER BY id").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    assert!(!before.is_empty());
    drop(j);
    // Put the catalog back as format 1.5 wrote it: the old fixed list in the event table.
    {
        let c = rusqlite::Connection::open(root.join(mdp_core::journey::CATALOG_FILE)).unwrap();
        c.execute_batch(
            "PRAGMA foreign_keys = OFF;
             CREATE TABLE event_old (id TEXT PRIMARY KEY REFERENCES item(id),
               kind TEXT NOT NULL CHECK (kind IN ('birth', 'death', 'wedding', 'divorce', 'move', 'graduation', 'reunion', 'funeral', 'holiday', 'trip')),
               name TEXT NOT NULL CHECK (name <> ''), start_date TEXT, start_date_edtf TEXT, end_date TEXT, end_date_edtf TEXT,
               place_id TEXT REFERENCES place(id), description TEXT, updated TEXT NOT NULL);
             INSERT INTO event_old SELECT id, kind, name, start_date, start_date_edtf, end_date, end_date_edtf, place_id, description, updated FROM event;
             DROP TABLE event; ALTER TABLE event_old RENAME TO event;
             PRAGMA user_version = 105;",
        )
        .unwrap();
    }
    j = Journey::open(&root).unwrap();
    let v: i64 = j.conn().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(v, 106);
    let after: Vec<(String, String, String)> = {
        let mut st = j.conn().prepare("SELECT id, kind, name FROM event ORDER BY id").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    assert_eq!(before, after);
    let fk: bool = j.conn().pragma_query_value(None, "foreign_keys", |r| r.get(0)).unwrap();
    assert!(fk, "links are checked again");
    let who = first_person(&j);
    let id = j.add_event("Mission", None, &[who], "", None).unwrap();
    assert_eq!(kind_of(&j, &id), "Mission");
}

#[test]
fn the_checksum_list_finds_a_damaged_file() {
    let t = tempfile::tempdir().unwrap();
    let j = mdp_core::sample::create(&t.path().join("sample")).unwrap();
    let plain = t.path().join("plain");
    j.export(&plain, Privacy::default(), &JobContext::new()).unwrap();
    assert!(!plain.join(MANIFEST_FILE).exists(), "off unless chosen");
    assert!(!std::fs::read_to_string(plain.join("ABOUT-THIS-FOLDER.txt")).unwrap().contains("manifest-sha256.txt"));

    let out = t.path().join("checked");
    j.export(&out, Privacy::default().with_checksums(true), &JobContext::new()).unwrap();
    let list = std::fs::read_to_string(out.join(MANIFEST_FILE)).unwrap();
    let lines: Vec<&str> = list.lines().collect();
    assert!(lines.len() > 10);
    // BagIt manifest lines: 64 hex digits, two spaces, a path with forward slashes.
    for l in &lines {
        let (sha, path) = l.split_once("  ").unwrap();
        assert!(sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit()), "{l}");
        assert!(!path.contains('\\') && !path.starts_with('/'), "{l}");
    }
    assert!(lines.iter().any(|l| l.ends_with("  catalog.json")));
    assert!(lines.iter().any(|l| l.ends_with("  ABOUT-THIS-FOLDER.txt")));
    assert!(!lines.iter().any(|l| l.ends_with(MANIFEST_FILE)));
    assert!(std::fs::read_to_string(out.join("ABOUT-THIS-FOLDER.txt")).unwrap().contains("CHECKING THAT NO FILE WAS DAMAGED"));
    assert_eq!(check_manifest(&out).unwrap(), Vec::<String>::new());

    // One changed byte in a photo is found.
    let photo = lines.iter().map(|l| l.split_once("  ").unwrap().1).find(|p| p.starts_with("photos/")).unwrap().to_string();
    let p = out.join(&photo);
    let mut b = std::fs::read(&p).unwrap();
    let mid = b.len() / 2;
    b[mid] ^= 0xFF;
    std::fs::write(&p, b).unwrap();
    assert_eq!(check_manifest(&out).unwrap(), vec![photo]);
}

#[test]
fn pages_print_cleanly() {
    let t = tempfile::tempdir().unwrap();
    let j = mdp_core::sample::create(&t.path().join("sample")).unwrap();
    let out = t.path().join("export");
    j.export(&out, Privacy::default().with_export_style("scrapbook"), &JobContext::new()).unwrap();
    let css = std::fs::read_to_string(out.join("pages").join("style.css")).unwrap();
    let print = &css[css.find("@media print{").expect("a print stylesheet")..];
    for rule in [".top,.foot", "break-inside:avoid", "--ink:#000", "audio,video{display:none"] {
        assert!(print.contains(rule), "{rule}");
    }
    // The Scrapbook look stays on screen only.
    assert!(std::fs::read_to_string(out.join("pages").join("scrapbook.css")).unwrap().contains("@media screen"));
    assert!(std::fs::read_to_string(out.join("ABOUT-THIS-FOLDER.txt")).unwrap().contains("PRINTING"));
}
