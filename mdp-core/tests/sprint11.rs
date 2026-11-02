//! Sprint 11 (release 2, bulk folder import): dates, titles, people and
//! places from folder and file names, the CSV import plan, a bulk import
//! made into memories in one action, and an exported history brought back
//! in as a full rebuild (which also merges two Journeys).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use mdp_core::bulk::{self, guess_title, name_date, ScanOptions};
use mdp_core::export::Privacy;
use mdp_core::jobs::JobContext;
use mdp_core::{rebuild, Journey};
use serde_json::json;

/// A one-pixel GIF, made unique by `n` so files are not duplicates.
fn gif(dir: &Path, name: &str, n: u8) -> PathBuf {
    let mut b = vec![
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0,
        1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B,
    ];
    b.push(n);
    let p = dir.join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, b).unwrap();
    p
}

fn count(j: &Journey, sql: &str) -> i64 {
    j.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn dates_and_titles_from_names() {
    for (name, want) in [
        ("1970", Some("1970")),
        ("1980-1985", Some("1980 to 1985")),
        ("1980 to 1985 Lake trips", Some("1980 to 1985")),
        ("June 1970", Some("1970-06")),
        ("Christmas Dec 1952", Some("1952-12")),
        ("1970s", Some("1970s")),
        ("1970-06 Picnic", Some("1970-06")),
        ("Grandpa's box", None),
        ("IMG_1234", None),
        ("1985-1980", Some("1985")),
    ] {
        assert_eq!(name_date(name).as_deref(), want, "{name}");
    }
    assert!(mdp_core::dates::parse("1970s").unwrap().is_some());
    assert_eq!(guess_title("Grandpa's box/scan001.jpg"), "Grandpa's box");
    assert_eq!(guess_title("1952 Christmas copy (2).jpg"), "Christmas");
    assert_eq!(guess_title("Copy of Lake Powell.jpg"), "Lake Powell");
    assert_eq!(guess_title("IMG_4512.jpg"), "IMG_4512");
}

fn journey_with_people(dir: &Path) -> Journey {
    let mut j = Journey::create(&dir.join("journey"), "Test").unwrap();
    let walter = j.add_individual("person", "Walter Rowan").unwrap();
    j.update_individual(&walter, &json!({"other_names": "Walt"}).as_object().cloned().unwrap()).unwrap();
    j.add_individual("person", "June Rowan").unwrap();
    j.add_individual("person", "Ada Hale").unwrap();
    j.add_individual("person", "Ada Smith").unwrap();
    j.add_place("The Rowan house").unwrap();
    j
}

#[test]
fn scan_guesses_and_imports_in_one_action() {
    let t = tempfile::tempdir().unwrap();
    let mut j = journey_with_people(t.path());
    let src = t.path().join("Family photos");
    gif(&src, "1970s/June Rowan at the Rowan house.gif", 1);
    gif(&src, "1970s/walt-and-june.gif", 2);
    gif(&src, "1980-1985 Lake trips/Boat ride 1983.gif", 3);
    gif(&src, "1980-1985 Lake trips/IMG_0001.gif", 4);
    gif(&src, "1980-1985 Lake trips/IMG_0002.gif", 5);
    std::fs::write(src.join("1980-1985 Lake trips/IMG_0001.txt"), "Ada on the dock.").unwrap();
    std::fs::write(src.join("Thumbs.db"), "x").unwrap();
    std::fs::write(src.join("notes.xyz"), "x").unwrap();
    gif(&src, "Ada.gif", 6);

    let ctx = JobContext::new();
    let plan = bulk::scan(&j, &src, ScanOptions { story_per_folder: true, ..ScanOptions::default() }, &ctx).unwrap();
    let row = |f: &str| plan.rows.iter().find(|r| r.file == f).unwrap_or_else(|| panic!("{f}: {:#?}", plan.rows));
    assert_eq!(plan.rows.len(), 6, "{:#?}", plan.rows);
    let a = row("1970s/June Rowan at the Rowan house.gif");
    assert_eq!((a.date.as_str(), a.date_source.as_str()), ("1970s", "folder \u{201c}1970s\u{201d}"));
    assert_eq!(a.people, "June Rowan");
    assert_eq!(a.place, "The Rowan house");
    assert_eq!(a.title, "June Rowan at the Rowan house");
    assert_eq!(a.story, "1970s");
    assert_eq!(row("1970s/walt-and-june.gif").people, "June Rowan; Walter Rowan");
    let b = row("1980-1985 Lake trips/Boat ride 1983.gif");
    assert_eq!((b.date.as_str(), b.date_source.as_str()), ("1983", "file name"));
    let c = row("1980-1985 Lake trips/IMG_0001.gif");
    assert_eq!(c.date, "1980 to 1985");
    assert_eq!(c.title, "Lake trips");
    assert_eq!(c.description, "Ada on the dock.");
    assert_eq!(c.group, "Lake trips", "same folder, date and people");
    // "Ada" is two people's first name, so it is asked about under Names
    // found (sprint 12), not guessed.
    let d = row("Ada.gif");
    assert!(d.people == "Ada" && d.people_note.contains("More than one person is called Ada"), "{d:?}");
    assert!(ctx.skipped().iter().any(|s| s.item.ends_with("notes.xyz")));
    assert!(!ctx.skipped().iter().any(|s| s.item.ends_with("Thumbs.db")));

    // CSV round trip, as a spreadsheet would save it.
    let csv_path = t.path().join("plans").join("plan.csv");
    let text = bulk::to_csv(&plan, &csv_path);
    assert!(text.starts_with('\u{feff}'));
    assert!(text.contains(&src.join("1970s").join("walt-and-june.gif").display().to_string()), "outside the CSV's folder: full paths");
    let back = bulk::from_csv(&text, &csv_path).unwrap();
    assert_eq!(back.rows.len(), plan.rows.len());
    assert_eq!(back.rows[0].people, plan.rows[0].people);
    assert!(back.rows.iter().all(|r| r.note.is_empty()), "{:#?}", back.rows);

    // Group the lake trip, give one row a new person, and import.
    let mut plan = plan;
    for r in plan.rows.iter_mut().filter(|r| r.file.contains("IMG_")) {
        r.memory = "Lake trips".into();
    }
    plan.rows.iter_mut().find(|r| r.file == "Ada.gif").unwrap().people = "Bob Smith (in the picture); June Rowan (taken by)".into();
    let before = count(&j, "SELECT count(*) FROM history");
    let ctx = JobContext::new();
    let batch = mdp_core::import::prepare(j.root(), &bulk::plan_files(&plan), &ctx).unwrap();
    let report = j.finish_plan(batch, &plan, false).unwrap();
    assert_eq!(report.imported, 6);
    assert_eq!(report.memories.len(), 5);
    assert!(report.notes.iter().any(|n| n.contains("Bob Smith")), "{:?}", report.notes);
    let actions = count(&j, &format!("SELECT count(DISTINCT action_id) FROM history WHERE seq > {before}"));
    assert_eq!(actions, 1, "one undo takes the import back");
    assert_eq!(count(&j, "SELECT count(*) FROM content WHERE state = 'in a memory'"), 6);
    assert_eq!(count(&j, "SELECT count(*) FROM memory WHERE name = 'Lake trips' AND date_edtf = '1980/1985' AND caption = 'Ada on the dock.'"), 1);
    assert_eq!(count(&j, "SELECT count(*) FROM memory_individual WHERE role = 'taken by'"), 1);
    assert_eq!(count(&j, "SELECT count(*) FROM individual WHERE name = 'Bob Smith'"), 0, "never added silently");
    assert_eq!(count(&j, "SELECT count(*) FROM story WHERE name = '1970s'"), 1);
    assert_eq!(count(&j, "SELECT count(*) FROM story_memory"), 4);

    // Running the same folder again brings in only new files.
    gif(&src, "1970s/new one.gif", 7);
    let plan2 = bulk::scan(&j, &src, ScanOptions::default(), &JobContext::new()).unwrap();
    let batch = mdp_core::import::prepare(j.root(), &bulk::plan_files(&plan2), &JobContext::new()).unwrap();
    let r2 = j.finish_plan(batch, &plan2, true).unwrap();
    assert_eq!((r2.imported, r2.duplicates, r2.memories.len()), (1, 6, 1));

    // Undo takes the second import back whole.
    j.undo().unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM content"), 6);
}

#[test]
fn plan_written_by_hand_and_sample() {
    let t = tempfile::tempdir().unwrap();
    let mut j = journey_with_people(t.path());
    gif(t.path(), "box/a.gif", 1);
    gif(t.path(), "box/b.gif", 2);
    let csv = "Title;File;Date;People;Memory\n\"Picnic; 1952\";box/a.gif;summer 1952;June Rowan;\n;box/b.gif;not a date;Walt;\n;box/missing.gif;;;\n";
    let plan = bulk::from_csv(csv, &t.path().join("plan.csv")).unwrap();
    assert_eq!(plan.rows[0].title, "Picnic; 1952");
    assert!(plan.rows[1].note.starts_with("The date") && plan.rows[1].include);
    assert!(plan.rows[2].note.contains("not found") && !plan.rows[2].include);
    let batch = mdp_core::import::prepare(j.root(), &bulk::plan_files(&plan), &JobContext::new()).unwrap();
    let r = j.finish_plan(batch, &plan, true).unwrap();
    assert_eq!(r.memories.len(), 2);
    assert!(r.notes.iter().any(|n| n.contains("not a date")));
    // "Walt" is Walter's other name.
    assert_eq!(count(&j, "SELECT count(*) FROM memory_individual m JOIN individual i ON i.id = m.individual_id WHERE i.name = 'Walter Rowan'"), 1);

    let (sample, readme) = bulk::sample_plan();
    let p = bulk::from_csv(&sample, &t.path().join("import-plan.csv")).unwrap();
    assert_eq!(p.rows.len(), 5);
    assert_eq!(p.rows[0].memory, "Christmas 1952");
    assert!(readme.contains("people"));
    assert!(bulk::check_space(t.path(), u64::MAX / 2).is_err());
    assert!(bulk::check_space(t.path(), 1).is_ok());
}

#[test]
fn exported_history_rebuilds_and_merges() {
    let t = tempfile::tempdir().unwrap();
    let src = mdp_core::sample::create(&t.path().join("sample")).unwrap();
    let export = t.path().join("export");
    src.export(&export, Privacy { living_dates: true, exact_places: true, coffee_table: false }, &JobContext::new()).unwrap();
    let count_all = |j: &Journey| {
        ["individual", "family", "family_member", "individual_relationship", "place", "event", "story", "story_memory", "memory", "memory_content", "memory_individual", "content"]
            .iter()
            .map(|t| (t.to_string(), count(j, &format!("SELECT count(*) FROM {t}"))))
            .collect::<HashMap<_, _>>()
    };

    // A full rebuild into an empty Journey.
    let mut j = Journey::create(&t.path().join("rebuilt"), "Rebuilt").unwrap();
    let look = rebuild::look(&j, &export).unwrap();
    assert!(look.people >= 4 && look.memories > 0 && look.matches.is_empty() && look.already == 0, "{look:?}");
    let eb = rebuild::prepare(j.root(), &export, &HashSet::new(), &JobContext::new()).unwrap();
    let r = j.finish_export(eb, &HashMap::new()).unwrap();
    assert!(r.files > 0 && r.notes.is_empty(), "{r:?}");
    let (a, b) = (count_all(&src), count_all(&j));
    for k in ["individual", "family", "family_member", "individual_relationship", "place", "event"] {
        assert_eq!(a[k], b[k], "{k}");
    }
    // Memories with nothing in them are not exported, nor stories holding only those.
    assert_eq!(b["memory"] as usize, look.memories);
    assert_eq!(b["story"] as usize, look.stories);
    assert_eq!(b["memory"], count(&src, "SELECT count(*) FROM memory WHERE status <> 'needs content'"));
    // The same IDs, so bringing it in again adds nothing.
    let walter: String = src.conn().query_row("SELECT id FROM individual WHERE name = 'Walter Rowan'", [], |r| r.get(0)).unwrap();
    assert_eq!(count(&j, &format!("SELECT count(*) FROM individual WHERE id = '{walter}'")), 1);
    let again = rebuild::look(&j, &export).unwrap();
    assert!(again.already > 0);
    let eb = rebuild::prepare(j.root(), &export, &HashSet::new(), &JobContext::new()).unwrap();
    let r = j.finish_export(eb, &HashMap::new()).unwrap();
    assert_eq!((r.added, r.files), (0, 0));
    assert_eq!(count_all(&j), b);

    // Choosing what comes in: leave out one memory and one person.
    let mut picky = Journey::create(&t.path().join("picky"), "Picky").unwrap();
    let look = rebuild::look(&picky, &export).unwrap();
    assert!(look.items.iter().any(|i| i.kind == "memory") && look.items.iter().any(|i| i.kind == "story"), "{:?}", look.items);
    let memory = look.items.iter().find(|i| i.kind == "memory").unwrap();
    let ellen = look.items.iter().find(|i| i.name == "Ellen Rowan").unwrap();
    assert!(!memory.detail.is_empty());
    let leave: HashSet<String> = [memory.export_id.clone(), ellen.export_id.clone()].into();
    let eb = rebuild::prepare(picky.root(), &export, &leave, &JobContext::new()).unwrap();
    picky.finish_export(eb, &HashMap::new()).unwrap();
    assert_eq!(count(&picky, "SELECT count(*) FROM memory") as usize, look.memories - 1);
    assert_eq!(count(&picky, "SELECT count(*) FROM individual WHERE name = 'Ellen Rowan'"), 0);
    assert_eq!(count(&picky, "SELECT count(*) FROM content WHERE state = 'waiting'"), 0, "only what the memories brought in hold");

    // Merging: another Journey that has its own June Rowan and a memory.
    let mut other = Journey::create(&t.path().join("other"), "Other").unwrap();
    let june = other.add_individual("person", "June Rowan").unwrap();
    let look = rebuild::look(&other, &export).unwrap();
    let m = look.matches.iter().find(|m| m.name == "June Rowan").expect("June is offered as a match");
    assert_eq!(m.journey_id, june);
    let merge: HashMap<String, String> = look.matches.iter().map(|m| (m.export_id.clone(), m.journey_id.clone())).collect();
    let eb = rebuild::prepare(other.root(), &export, &HashSet::new(), &JobContext::new()).unwrap();
    let r = other.finish_export(eb, &merge).unwrap();
    assert!(r.matched >= 1);
    assert_eq!(count(&other, "SELECT count(*) FROM individual WHERE name = 'June Rowan'"), 1, "no double June");
    assert!(count(&other, &format!("SELECT count(*) FROM family_member WHERE individual_id = '{june}'")) > 0);
    // One undo takes the whole merge back.
    other.undo().unwrap();
    assert_eq!(count(&other, "SELECT count(*) FROM individual"), 1);
}
