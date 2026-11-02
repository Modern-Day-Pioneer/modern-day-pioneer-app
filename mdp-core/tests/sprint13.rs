//! Sprint 13 (release 3, the last sprint before the public release): the
//! coffee table front door, "In memory of" pages with a note on each
//! memory, and search in the exported history.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use mdp_core::export::{Privacy, FRONT};
use mdp_core::jobs::JobContext;
use mdp_core::{rebuild, Journey};

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// Every `href` and `src` in a page resolves to a file in the export.
fn links_resolve(dest: &Path) {
    for p in walk(dest).into_iter().filter(|p| p.extension().is_some_and(|e| e == "html")) {
        let html = std::fs::read_to_string(&p).unwrap();
        for attr in ["href=\"", "src=\""] {
            let mut rest = html.as_str();
            while let Some(i) = rest.find(attr) {
                rest = &rest[i + attr.len()..];
                let end = rest.find('"').unwrap();
                let l = rest[..end].split('#').next().unwrap().replace("%20", " ");
                if !l.is_empty() {
                    assert!(p.parent().unwrap().join(&l).exists(), "{} links to missing {l}", p.display());
                }
                rest = &rest[end..];
            }
        }
    }
}

fn one(j: &Journey, sql: &str) -> String {
    j.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

/// The sample Journey with an "In memory of" page about Walter.
fn with_tribute(dir: &Path) -> (Journey, String, String, String) {
    let mut j = mdp_core::sample::create(&dir.join("sample")).unwrap();
    let walter = one(&j, "SELECT id FROM individual WHERE name = 'Walter Rowan'");
    let mems: Vec<String> = {
        let mut st = j
            .conn()
            .prepare("SELECT id FROM memory WHERE status <> 'needs content' AND EXISTS (SELECT 1 FROM memory_content mc WHERE mc.memory_id = memory.id) ORDER BY name LIMIT 2")
            .unwrap();
        st.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
    };
    assert_eq!(mems.len(), 2);
    let t = j.add_tribute(&walter, "In memory of Walter").unwrap();
    j.update_story(&t, &[("introduction".to_string(), "He never put the camera down.".into())].into_iter().collect()).unwrap();
    j.add_memories_to_story(&t, &mems).unwrap();
    j.set_memory_note(&t, &mems[0], "This is how we remember his Saturdays.").unwrap();
    (j, t, walter, mems[0].clone())
}

#[test]
fn in_memory_of_page_is_kept_exported_and_rebuilt() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, t, walter, first) = with_tribute(dir.path());
    // A family can't have one; a person can have several.
    let fam = one(&j, "SELECT id FROM family LIMIT 1");
    assert!(j.add_tribute(&fam, "In memory of us").is_err());
    let v = j.story_view(&t).unwrap();
    assert_eq!(v.genre, "tribute");
    assert_eq!(v.memories[0].note.as_deref(), Some("This is how we remember his Saturdays."));
    assert_eq!(v.memories[1].note, None);
    assert_eq!(j.tributes_about(&walter).unwrap().len(), 1);
    // The plain-text copy keeps the kind and the note.
    let line = mdp_core::jsonl::line(j.conn(), &t).unwrap().unwrap();
    assert_eq!(line["genre"], "tribute");
    assert_eq!(line["memoryNotes"][0]["memory"], format!("mdpid:{first}"));
    // Clearing a note, then undoing it.
    j.set_memory_note(&t, &first, " ").unwrap();
    assert_eq!(j.story_view(&t).unwrap().memories[0].note, None);
    j.undo().unwrap();
    assert!(j.story_view(&t).unwrap().memories[0].note.is_some());

    let dest = dir.path().join("export");
    j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
    let cat: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dest.join("catalog.json")).unwrap()).unwrap();
    let node = cat["@graph"].as_array().unwrap().iter().find(|o| o["@id"] == format!("mdpid:{t}")).unwrap();
    assert_eq!(node["genre"], "tribute");
    assert_eq!(node["focus"], format!("mdpid:{walter}"));
    assert_eq!(node["memoryNotes"][0]["note"], "This is how we remember his Saturdays.");
    let page = walk(&dest).into_iter().find(|p| p.to_string_lossy().contains("tributes") && p.extension().is_some_and(|e| e == "html")).expect("a tribute page");
    let html = std::fs::read_to_string(&page).unwrap();
    assert!(html.contains("In memory of Walter") && html.contains("Why it matters") && html.contains("remember his Saturdays"));
    assert!(html.contains("He never put the camera down."));
    let shelf = std::fs::read_to_string(dest.join(FRONT)).unwrap();
    assert!(shelf.contains("bookcase") && shelf.contains(">In memory of<"), "the bookshelf has an In memory of shelf");
    let walter_page = walk(&dest).into_iter().find(|p| p.to_string_lossy().contains("walter-rowan")).unwrap();
    assert!(std::fs::read_to_string(walter_page).unwrap().contains("In memory of Walter"), "Walter's page links to it");
    links_resolve(&dest);

    // An exported history brought back in keeps the page and its note.
    let mut k = Journey::create(&dir.path().join("rebuilt"), "Rebuilt").unwrap();
    let eb = rebuild::prepare(k.root(), &dest, &HashSet::new(), &JobContext::new()).unwrap();
    k.finish_export(eb, &HashMap::new()).unwrap();
    let v = k.story_view(&t).unwrap();
    assert_eq!(v.genre, "tribute");
    assert_eq!(v.memories.len(), 2);
    assert!(v.memories[0].note.as_deref().is_some_and(|n| n.contains("Saturdays")));
}

#[test]
fn coffee_table_front_door_and_search() {
    let dir = tempfile::tempdir().unwrap();
    let (j, _, _, _) = with_tribute(dir.path());
    let dest = dir.path().join("table");
    j.export(&dest, Privacy::from_settings("leave out", "town only").with_front_door("coffee table"), &JobContext::new()).unwrap();
    let front = std::fs::read_to_string(dest.join(FRONT)).unwrap();
    assert!(front.contains("coffee-table") && !front.contains("bookcase"));
    assert!(front.contains(">Coffee table</a>") && front.contains("In memory of Walter"));
    assert!(front.contains("Also on the table"), "the plain lists stay one link away");
    assert!(!front.contains("<script"));
    links_resolve(&dest);

    // Search: one page with a script, reading an index made with the pages.
    let search = std::fs::read_to_string(dest.join("pages/search.html")).unwrap();
    assert!(search.contains("<script src=\"search.js\"></script>") && search.contains("<noscript>"));
    let idx = std::fs::read_to_string(dest.join("pages/search.js")).unwrap();
    let idx: serde_json::Value = serde_json::from_str(idx.trim().trim_start_matches("window.SEARCH = ").trim_end_matches(';')).unwrap();
    let rows = idx.as_array().unwrap();
    assert!(rows.iter().any(|r| r["k"] == "Person" && r["t"] == "Walter Rowan"));
    assert!(rows.iter().any(|r| r["k"] == "In memory of"));
    let mem = rows.iter().find(|r| r["k"] == "Memory").unwrap();
    let url = mem["u"].as_str().unwrap();
    assert!(dest.join("pages").join(url.split('#').next().unwrap()).exists(), "{url}");
    // Every page's menu has Search.
    assert!(std::fs::read_to_string(dest.join("pages/people.html")).unwrap().contains("search.html\">Search</a>"));
}

#[test]
fn a_format_1_4_journey_gains_tributes_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("old");
    {
        let j = Journey::create(&root, "Old").unwrap();
        j.conn()
            .execute_batch("ALTER TABLE story DROP COLUMN genre; ALTER TABLE story_memory DROP COLUMN note; PRAGMA user_version = 104;")
            .unwrap();
    }
    let mut j = Journey::open(&root).unwrap();
    let p = j.add_individual("Person", "Ada Hale").unwrap();
    let t = j.add_tribute(&p, "Celebrating Ada").unwrap();
    assert_eq!(j.story_view(&t).unwrap().genre, "tribute");
}
