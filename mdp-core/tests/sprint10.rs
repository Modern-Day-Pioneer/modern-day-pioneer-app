//! Sprint 10 (release 2, easier sorting and tidying): dates and titles from
//! file names, waiting items in groups, proposed values for a memory,
//! suggested relationships, merging duplicates with aliases, and a
//! recording's quiet start.

use std::path::{Path, PathBuf};

use mdp_core::jobs::JobContext;
use mdp_core::sorting::{best_date, file_name_date, file_name_title, DateSource};
use mdp_core::voice::quiet_start;
use mdp_core::{Journey, Row};
use serde_json::json;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

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

fn changes(v: serde_json::Value) -> Row {
    v.as_object().cloned().unwrap()
}

#[test]
fn dates_from_file_names() {
    for (name, want) in [
        ("IMG_20240612_153012.jpg", Some("2024-06-12")),
        ("PXL_20240612_153012345.jpg", Some("2024-06-12")),
        ("20240612_153012.jpg", Some("2024-06-12")),
        ("VID_20240612_101010.mp4", Some("2024-06-12")),
        ("Screenshot 2024-06-12 at 15.30.12.png", Some("2024-06-12")),
        ("Screenshot_20240612-153012.png", Some("2024-06-12")),
        ("WhatsApp Image 2024-06-12 at 15.30.12.jpeg", Some("2024-06-12")),
        ("scan_1952_06_02.tif", Some("1952-06-02")),
        ("1952-06 Grandpa camera.jpg", Some("1952-06")),
        ("1952 Christmas.jpg", Some("1952")),
        ("Grandpa 1950s.jpg", Some("1950s")),
        ("06-12-52 picnic.jpg", None),
        ("IMG_4512.jpg", None),
        ("DSC01987.jpg", None),
        ("DSC_1987.jpg", None),
        ("cup-of-coffee.jpg", None),
        ("20241399_000000.jpg", None),
        ("2999 future.jpg", None),
    ] {
        assert_eq!(file_name_date(name).as_deref(), want, "{name}");
    }
}

#[test]
fn titles_from_file_names() {
    for (name, want) in [
        ("1952-06 Grandpa camera.jpg", Some("Grandpa camera")),
        ("walters_saturday-camera.jpg", Some("walters saturday camera")),
        ("IMG_4512.jpg", None),
        ("IMG_20240612_153012.jpg", None),
        ("Screenshot 2024-06-12 at 15.30.12.png", None),
        ("Grandpa 1950s.jpg", Some("Grandpa")),
        ("PXL_20240612_153012345 Lake day.jpg", Some("Lake day")),
        ("1952 Christmas.jpg", Some("Christmas")),
    ] {
        assert_eq!(file_name_title(name).as_deref(), want, "{name}");
    }
}

#[test]
fn camera_date_wins_unless_the_name_is_decades_older() {
    assert_eq!(best_date(Some("2024-06-12"), "IMG_20240612_1.jpg"), Some(("2024-06-12".into(), DateSource::PhotoFacts)));
    // A scan made in 2019 of a photo named by hand as 1952.
    assert_eq!(best_date(Some("2019-03-01"), "1952-06 Grandpa.jpg"), Some(("1952-06".into(), DateSource::FileName)));
    assert_eq!(best_date(Some("2019-03-01"), "2010 lake.jpg"), Some(("2019-03-01".into(), DateSource::PhotoFacts)));
    assert_eq!(best_date(None, "1952 Christmas.jpg"), Some(("1952".into(), DateSource::FileName)));
    assert_eq!(best_date(None, "cup.jpg"), None);
}

#[test]
fn waiting_items_come_in_groups_and_become_memories_in_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let files = vec![
        gif(&src, "phone/IMG_20240612_100000.gif", 1),
        gif(&src, "phone/IMG_20240612_110000.gif", 2),
        gif(&src, "phone/IMG_20240613_090000.gif", 3),
        gif(&src, "Grandpa's box/1952 Christmas.gif", 4),
        gif(&src, "Grandpa's box/1952 Christmas tree.gif", 5),
        gif(&src, "loose/cup.gif", 6),
    ];
    let (r, _) = j.import(&files, &JobContext::new()).unwrap();
    assert_eq!(r.imported.len(), 6);
    let raw = j.raw_content("waiting").unwrap();
    let xmas = raw.iter().find(|i| i.name == "1952 Christmas.gif").unwrap();
    assert_eq!(xmas.name_date.as_deref(), Some("1952"));
    assert_eq!(xmas.title.as_deref(), Some("Christmas"));
    assert_eq!(xmas.date_sort.as_deref(), Some("1952"));

    let groups = j.waiting_groups().unwrap();
    assert_eq!(groups.len(), 2, "{groups:#?}");
    let day = groups.iter().find(|g| g.key == "day:2024-06-12").unwrap();
    assert_eq!(day.items.len(), 2);
    assert_eq!(day.label, "June 12, 2024");
    assert_eq!(day.date.as_deref(), Some("2024-06-12"));
    assert_eq!(day.date_source.as_deref(), Some("from the file name"));
    let folder = groups.iter().find(|g| g.reason == "From the same folder").unwrap();
    assert_eq!(folder.label, "Grandpa's box");
    assert_eq!(folder.title, "Grandpa's box");
    assert_eq!(folder.date.as_deref(), Some("1952"));
    assert!(groups.iter().all(|g| !g.items.contains(&r.imported[5])), "a lone item is in no group");

    let m = j.memory_from_group(&day.title, &day.items, day.date.as_deref(), None).unwrap();
    let v = j.memory_view(&m).unwrap();
    assert_eq!(v.items.len(), 2);
    assert_eq!(v.date.as_ref().unwrap().edtf, "2024-06-12");
    assert_eq!(j.waiting_groups().unwrap().len(), 1);
    j.undo().unwrap();
    assert_eq!(j.waiting_groups().unwrap().len(), 2, "one undo takes the whole step back");

    let each = j.memory_each(&folder.items).unwrap();
    assert_eq!(each.len(), 2);
    let names: Vec<String> = each.iter().map(|m| j.memory_view(m).unwrap().name).collect();
    assert!(names.contains(&"Christmas".to_string()) && names.contains(&"Christmas tree".to_string()), "{names:?}");
    assert_eq!(j.memory_view(&each[0]).unwrap().date.unwrap().edtf, "1952");
}

#[test]
fn pictures_near_a_known_place_group_and_propose_it() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let ogden = j.add_place("Ogden, Utah").unwrap();
    j.update_place(&ogden, &changes(json!({"coordinates": "41.2230, -111.9738"}))).unwrap();
    std::fs::create_dir_all(&src).unwrap();
    std::fs::copy(fixtures().join("walter-camera.jpg"), src.join("walter-camera.jpg")).unwrap();
    let (r, _) = j.import(&[src.join("walter-camera.jpg")], &JobContext::new()).unwrap();
    let m = j.new_memory_with("walter-camera.jpg", &r.imported, None).unwrap();
    let p = j.memory_proposals(&m).unwrap();
    let date = p.iter().find(|p| p.field == "date").unwrap();
    assert_eq!((date.value.as_str(), date.source.as_str()), ("1962-06-02", "from photo facts"));
    let place = p.iter().find(|p| p.field == "place_id").unwrap();
    assert_eq!((place.value.as_str(), place.words.as_str()), (ogden.as_str(), "Ogden, Utah"));
    let title = p.iter().find(|p| p.field == "name").unwrap();
    assert_eq!(title.value, "walter camera");

    // People from another memory of the same day.
    let walter = j.add_individual("Person", "Walter Rowan").unwrap();
    let other = j.add_memory("Saturday", None).unwrap();
    j.update_memory(&other, &changes(json!({"date": "1962-06-02"}))).unwrap();
    j.link_to_memory(&other, &walter, "taken by", None, false).unwrap();
    j.update_memory(&m, &changes(json!({"date": "1962-06-02"}))).unwrap();
    let p = j.memory_proposals(&m).unwrap();
    let who = p.iter().find(|p| p.field == "person").unwrap();
    assert_eq!((who.value.as_str(), who.role.as_deref()), (walter.as_str(), Some("in the picture")));
    assert!(!p.iter().any(|p| p.field == "date"), "the memory has a date now");

    j.update_memory(&m, &changes(json!({"finished": true}))).unwrap();
    assert!(j.memory_proposals(&m).unwrap().is_empty(), "a finished memory gets no proposals");
}

#[test]
fn relationships_are_suggested_from_the_rule_table() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let p = |j: &mut Journey, n: &str, g: &str| {
        let id = j.add_individual("Person", n).unwrap();
        j.update_individual(&id, &changes(json!({"gender": g}))).unwrap();
        id
    };
    let ada = p(&mut j, "Ada Hale", "female");
    let walter = p(&mut j, "Walter Rowan", "male");
    let june = p(&mut j, "June Rowan", "female");
    let ellen = p(&mut j, "Ellen Rowan", "female");
    let tom = p(&mut j, "Tom Rowan", "male");
    let rose = p(&mut j, "Rose", "female");
    j.add_relationship(&ada, "parent", &walter, None, None, None).unwrap();
    j.add_relationship(&ada, "parent", &june, None, None, None).unwrap();
    j.add_relationship(&walter, "parent", &ellen, None, None, None).unwrap();
    j.add_relationship(&walter, "parent", &tom, None, None, None).unwrap();
    j.add_relationship(&rose, "partner", &walter, None, None, None).unwrap();

    let s = j.suggested_relationships(&ellen).unwrap();
    let find = |from: &str, kind: &str, to: &str| s.iter().find(|x| x.from_id == from && x.kind == kind && x.to_id == to).cloned();
    let gran = find(&ada, "grandparent", &ellen).expect("grandmother");
    assert_eq!(gran.words, "Ada Hale is probably Ellen Rowan's grandmother");
    assert_eq!(gran.because, "Ada Hale is Walter Rowan's mother, and Walter Rowan is Ellen Rowan's father.");
    assert!(find(&tom, "sibling", &ellen).is_some());
    assert!(find(&june, "uncle or aunt", &ellen).is_some());
    let step = find(&rose, "parent", &ellen).expect("step-parent");
    assert_eq!(step.qualifier.as_deref(), Some("step"));

    j.answer_suggestion(&gran, true).unwrap();
    let rels = j.relationships_of(&ellen).unwrap();
    assert!(rels.iter().any(|r| r.words == "Granddaughter of Ada Hale"), "{rels:?}");
    j.answer_suggestion(&step, false).unwrap();
    let s = j.suggested_relationships(&ellen).unwrap();
    assert!(!s.iter().any(|x| x.from_id == ada || x.from_id == rose), "answered suggestions are not offered again");
    assert!(!j.relationships_of(&ellen).unwrap().iter().any(|r| r.other_id == rose), "a turned-down suggestion is not a relationship");

    // The other way round, from the grandmother's page; and an in-law.
    let s = j.suggested_relationships(&ada).unwrap();
    assert!(s.iter().any(|x| x.from_id == ada && x.kind == "grandparent" && x.to_id == tom));
    assert!(s.iter().any(|x| x.from_id == rose && x.kind == "in-law" && x.to_id == ada) || s.iter().any(|x| x.kind == "in-law"));
}

#[test]
fn duplicate_people_merge_and_keep_the_old_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let w1 = j.add_individual("Person", "Walter Rowan").unwrap();
    let w2 = j.add_individual("Person", "Walt Rowan").unwrap();
    let ellen = j.add_individual("Person", "Ellen Rowan").unwrap();
    let cat = j.add_individual("Pet", "Pepper").unwrap();
    j.update_individual(&w2, &changes(json!({"born": "1931", "description": "Loved cameras."}))).unwrap();
    j.add_relationship(&w2, "parent", &ellen, None, None, None).unwrap();
    j.add_relationship(&w1, "parent", &ellen, None, None, None).unwrap();
    j.add_relationship(&w1, "sibling", &w2, None, None, None).unwrap();
    let fam = j.add_family("The Rowans").unwrap();
    j.add_family_member(&fam, &w2).unwrap();
    let m = j.add_memory("Saturday", None).unwrap();
    j.link_to_memory(&m, &w2, "taken by", None, false).unwrap();
    j.link_to_memory(&m, &w1, "taken by", None, false).unwrap();
    assert!(j.merge_individuals(&w1, &cat).is_err(), "a person and a pet can't be merged");

    j.merge_individuals(&w1, &w2).unwrap();
    let v = j.individual_view(&w1).unwrap();
    assert_eq!(v.born.as_ref().unwrap().edtf, "1931");
    assert_eq!(v.description.as_deref(), Some("Loved cameras."));
    assert_eq!(v.other_names.as_deref(), Some("Walt Rowan"));
    let rels = j.relationships_of(&w1).unwrap();
    assert_eq!(rels.len(), 1, "the doubled parent link and the self link are gone: {rels:?}");
    assert_eq!(j.family_view(&fam).unwrap().members.len(), 1);
    assert_eq!(j.memory_view(&m).unwrap().people.len(), 1);
    assert_eq!(j.resolve_id(&w2).unwrap(), w1);
    assert!(j.individual_view(&w2).is_err());
    let jsonl = std::fs::read_to_string(j.root().join("catalog.jsonl")).unwrap();
    let line = jsonl.lines().find(|l| l.contains(&format!("\"@id\":\"mdpid:{w1}\""))).unwrap();
    assert!(line.contains(&format!("\"mdp:formerId\":[\"mdpid:{w2}\"]")), "{line}");

    // An alias follows a second merge.
    let w3 = j.add_individual("Person", "W. Rowan").unwrap();
    j.merge_individuals(&w3, &w1).unwrap();
    assert_eq!(j.resolve_id(&w2).unwrap(), w3);
    j.undo().unwrap();
    assert_eq!(j.resolve_id(&w2).unwrap(), w1);
    j.undo().unwrap(); // adding W. Rowan
    j.undo().unwrap();
    assert!(j.individual_view(&w2).is_ok(), "undo brings the merged entry back");
    assert_eq!(j.resolve_id(&w2).unwrap(), w2);
}

#[test]
fn merged_places_keep_the_old_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let a = j.add_place("The Rowan house").unwrap();
    let b = j.add_place("Rowan home").unwrap();
    j.merge_places(&a, &b).unwrap();
    assert_eq!(j.resolve_id(&b).unwrap(), a);
}

#[test]
fn a_quiet_start_is_found_and_skipped_when_played() {
    let rate = 16_000;
    let tone = |secs: f64| (0..(secs * rate as f64) as usize).map(|i| (i as f32 * 0.05).sin() * 0.3).collect::<Vec<f32>>();
    let quiet = |secs: f64| vec![0.001_f32; (secs * rate as f64) as usize];
    let mut s = quiet(3.0);
    s.extend(tone(2.0));
    assert_eq!(quiet_start(&s, rate), 2.5);
    // A click in the quiet part is not the start.
    let mut c = quiet(1.0);
    c.extend(tone(0.05));
    c.extend(quiet(2.0));
    c.extend(tone(1.0));
    assert_eq!(quiet_start(&c, rate), 2.5);
    let mut short = quiet(1.0);
    short.extend(tone(1.0));
    assert_eq!(quiet_start(&short, rate), 0.0, "a short quiet start is left alone");
    assert_eq!(quiet_start(&quiet(4.0), rate), 0.0, "all quiet");

    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let (r, _) = j.import(&[fixtures().join("hello.wav")], &JobContext::new()).unwrap();
    let id = &r.imported[0];
    let todo = j.recordings_to_check().unwrap();
    assert_eq!(todo.len(), 1);
    assert!(todo[0].1.is_file());
    let undo = j.undo_depth();
    j.set_quiet_start(id, 2.5).unwrap();
    assert_eq!(j.undo_depth(), undo, "finding the quiet start is not an undo step");
    assert!(j.recordings_to_check().unwrap().is_empty());
    assert_eq!(j.play_from(id).unwrap(), (2.5, 2.5));
    j.set_play_full(id, true).unwrap();
    assert_eq!(j.play_from(id).unwrap(), (0.0, 2.5));
    j.undo().unwrap();
    assert_eq!(j.play_from(id).unwrap(), (2.5, 2.5));
    let m = j.new_memory_with("Hello", &r.imported, None).unwrap();
    let out = dir.path().join("out");
    j.export(&out, mdp_core::export::Privacy::default(), &JobContext::new()).unwrap();
    let mut html = String::new();
    fn all(p: &Path, out: &mut String) {
        if p.is_dir() {
            for e in std::fs::read_dir(p).unwrap().flatten() {
                all(&e.path(), out);
            }
        } else if p.extension().is_some_and(|x| x == "html") {
            out.push_str(&std::fs::read_to_string(p).unwrap());
        }
    }
    all(&out, &mut html);
    assert!(html.contains("#t=2.5\""), "the export skips the quiet start");
    let _ = m;
}
