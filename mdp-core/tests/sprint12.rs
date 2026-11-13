//! Sprint 12 (release 2, desktop): Google Takeout sidecars in the folder
//! import fill the date, description, place and tagged people; Google's
//! "-edited" copies are skipped; each sidecar is kept beside its photo.

use std::path::{Path, PathBuf};

use mdp_core::bulk::{self, ScanOptions};
use mdp_core::jobs::JobContext;
use mdp_core::Journey;
use serde_json::json;

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

fn sidecar(dir: &Path, name: &str, v: serde_json::Value) {
    std::fs::write(dir.join(name), serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

fn count(j: &Journey, sql: &str) -> i64 {
    j.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn takeout_sidecars_fill_the_preview() {
    let t = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&t.path().join("journey"), "Test").unwrap();
    j.add_individual("person", "June Rowan").unwrap();
    let house = j.add_place("The Rowan house").unwrap();
    j.update_place(&house, &json!({"coordinates": "43.6150, -116.2023"}).as_object().cloned().unwrap()).unwrap();

    let album = t.path().join("Takeout/Google Photos/Photos from 2019");
    // Noon UTC, so the date is the same in any time zone in the Americas or Europe.
    let noon = "1562846400"; // 2019-07-11 12:00 UTC
    gif(&album, "IMG_0001.gif", 1);
    sidecar(&album, "IMG_0001.gif.json", json!({"title": "IMG_0001.gif", "description": "June on the porch",
        "photoTakenTime": {"timestamp": noon}, "geoData": {"latitude": 43.6152, "longitude": -116.2020},
        "people": [{"name": "June Rowan"}, {"name": "Cousin Pat"}]}));
    gif(&album, "IMG_0001-edited.gif", 2);
    gif(&album, "IMG_0001(1).gif", 3);
    sidecar(&album, "IMG_0001.gif.supplemental-metadata(1).json", json!({"title": "IMG_0001.gif",
        "photoTakenTime": {"timestamp": "0"}, "geoData": {"latitude": 10.0, "longitude": 10.0}}));
    let long = "Grandma and Grandpa at the lake in summer.gif";
    gif(&album, long, 4);
    sidecar(&album, "Grandma and Grandpa at the lake in sum.json", json!({"title": long, "photoTakenTime": {"timestamp": noon}}));
    sidecar(&album, "metadata.json", json!({"title": "Photos from 2019"}));

    let ctx = JobContext::new();
    let plan = bulk::scan(&j, &t.path().join("Takeout"), ScanOptions::default(), &ctx).unwrap();
    let row = |f: &str| plan.rows.iter().find(|r| r.file.ends_with(f)).unwrap_or_else(|| panic!("{f}: {:#?}", plan.rows));
    assert_eq!(plan.rows.len(), 3, "{:#?}", plan.rows);
    let a = row("/IMG_0001.gif");
    assert_eq!((a.date.as_str(), a.date_source.as_str()), ("2019-07-11", "Google Photos"));
    assert_eq!(a.description, "June on the porch");
    assert_eq!(a.place, "The Rowan house", "within a kilometer of a place with a location");
    assert_eq!(a.people, "June Rowan; Cousin Pat");
    assert!(a.people_note.contains("Cousin Pat"), "{a:?}");
    let b = row("IMG_0001(1).gif");
    assert_eq!(b.date, "2019", "no Google date, so the folder's");
    assert!(b.place.is_empty() && b.place_note.contains("near Yamrat, Bauchi, Nigeria"), "{b:?}");
    assert_eq!(row(long).date, "2019-07-11", "a sidecar name cut short");
    let skipped = ctx.skipped();
    assert!(skipped.iter().any(|s| s.item.ends_with("IMG_0001-edited.gif")));
    assert!(!skipped.iter().any(|s| s.item.ends_with(".json")), "{skipped:?}");

    // Import without adding new people: Cousin Pat is left out, not added.
    let batch = mdp_core::import::prepare(j.root(), &bulk::plan_files(&plan), &JobContext::new()).unwrap();
    let report = j.finish_plan(batch, &plan, false).unwrap();
    assert_eq!(report.imported, 3);
    assert!(report.notes.iter().any(|n| n.contains("Cousin Pat")), "{:?}", report.notes);
    assert_eq!(count(&j, "SELECT count(*) FROM individual WHERE name = 'Cousin Pat'"), 0);
    assert_eq!(count(&j, "SELECT count(*) FROM memory WHERE caption = 'June on the porch' AND date = '2019-07-11' AND place_id IS NOT NULL"), 1);
    // Each sidecar sits beside its photo's copy, read-only.
    let mut st = j.conn().prepare("SELECT path FROM content_file").unwrap();
    let paths: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    let beside: Vec<PathBuf> = paths.iter().map(|p| j.root().join(format!("{p}.json"))).filter(|p| p.exists()).collect();
    assert_eq!(beside.len(), 3, "{paths:?}");
    assert!(beside.iter().all(|p| std::fs::metadata(p).unwrap().permissions().readonly()));
    let text = std::fs::read_to_string(beside.iter().find(|p| std::fs::read_to_string(p).unwrap().contains("porch")).unwrap()).unwrap();
    assert!(text.contains("Cousin Pat"), "the whole sidecar is kept");
    // The originals are untouched.
    assert!(album.join("IMG_0001.gif.json").exists());
}

#[test]
fn plain_json_files_are_still_reported() {
    let t = tempfile::tempdir().unwrap();
    let j = Journey::create(&t.path().join("journey"), "Test").unwrap();
    let src = t.path().join("box");
    gif(&src, "scan.gif", 1);
    std::fs::write(src.join("settings.json"), "{}").unwrap();
    let ctx = JobContext::new();
    let plan = bulk::scan(&j, &src, ScanOptions::default(), &ctx).unwrap();
    assert_eq!(plan.rows.len(), 1);
    assert!(ctx.skipped().iter().any(|s| s.item.ends_with("settings.json")));
}

#[test]
fn assign_to_several_people_by_name() {
    let t = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&t.path().join("journey"), "Test").unwrap();
    j.add_individual("person", "June Rowan").unwrap();
    let walter = j.add_individual("person", "Walter Rowan").unwrap();
    j.update_individual(&walter, &json!({"given_name": "Walter"}).as_object().cloned().unwrap()).unwrap();
    let old_ada = j.add_individual("person", "Ada Hale").unwrap();
    j.update_individual(&old_ada, &json!({"born": "1912"}).as_object().cloned().unwrap()).unwrap();
    let young_ada = j.add_individual("person", "Ada Hale").unwrap();
    j.update_individual(&young_ada, &json!({"born": "2015-03-02"}).as_object().cloned().unwrap()).unwrap();
    let hints = j.name_hints().unwrap();
    assert!(hints.iter().any(|h| h.id == old_ada && h.hint == "born 1912"), "{hints:?}");
    let src = t.path().join("in");
    let files = vec![gif(&src, "a.gif", 1), gif(&src, "b.gif", 2)];
    let (report, _) = j.import(&files, &JobContext::new()).unwrap();
    let before = count(&j, "SELECT count(DISTINCT action_id) FROM history");
    let r = j.assign_to_names(" june rowan, Walter ,Cousin  Pat, Ada Hale, cousin pat", &report.imported).unwrap();
    assert_eq!(r.added, vec!["Cousin Pat".to_string()], "added at once, no confirmation");
    assert_eq!(r.unsure, vec!["Ada Hale".to_string()]);
    assert_eq!(count(&j, &format!("SELECT count(*) FROM memory_individual WHERE memory_id = '{}'", r.memory)), 3);
    assert_eq!(count(&j, &format!("SELECT count(*) FROM memory_content WHERE memory_id = '{}'", r.memory)), 2);
    assert_eq!(count(&j, "SELECT count(*) FROM memory WHERE name = 'June Rowan and 2 others: 2 items'"), 1);
    assert_eq!(count(&j, "SELECT count(DISTINCT action_id) FROM history"), before + 1, "one undo");
    j.undo().unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM individual WHERE name = 'Cousin Pat'"), 0);
    assert!(j.assign_to_names(" , ", &report.imported).is_err());
    // Told apart by the year: "Ada Hale (born 2015)" or just "(2015)".
    for typed in ["Ada Hale (born 2015)", "Ada Hale (2015)"] {
        let r = j.assign_to_names(typed, &report.imported[..1]).unwrap();
        assert!(r.unsure.is_empty() && r.added.is_empty(), "{r:?}");
        assert_eq!(count(&j, &format!("SELECT count(*) FROM memory_individual WHERE memory_id = '{}' AND individual_id = '{young_ada}'", r.memory)), 1);
    }
}

#[test]
fn memory_who_box_links_several_by_name() {
    let t = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&t.path().join("journey"), "Test").unwrap();
    j.add_individual("person", "June Rowan").unwrap();
    j.add_family("The Rowans").unwrap();
    let src = t.path().join("in");
    let (report, _) = j.import(&[gif(&src, "a.gif", 1)], &JobContext::new()).unwrap();
    let m = j.assign_to_names("June Rowan", &report.imported).unwrap().memory;
    let r = j.link_names_to_memory(&m, "June Rowan, The Rowans, Cousin Pat", "taken by", None, false).unwrap();
    assert_eq!(r.added, vec!["Cousin Pat".to_string()]);
    assert_eq!(count(&j, &format!("SELECT count(*) FROM memory_individual WHERE memory_id = '{m}' AND role = 'taken by'")), 3);
    // Again: already linked with that role, so nothing doubles.
    j.link_names_to_memory(&m, "june rowan", "taken by", None, false).unwrap();
    assert_eq!(count(&j, &format!("SELECT count(*) FROM memory_individual WHERE memory_id = '{m}'")), 4);
    assert!(j.name_hints().unwrap().iter().any(|h| h.kind == "family" && h.name == "The Rowans"));
}

#[test]
fn tiff_gets_a_viewable_copy() {
    // Issue #27: the app's window can't draw TIFF, so it gets a JPEG copy.
    assert_eq!(mdp_core::convert::target("image/tiff", None), Some(("jpg", "image/jpeg")));
}

/// A zip with these files (path inside, bytes).
fn zip_of(path: &Path, files: &[(&str, Vec<u8>)]) {
    let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in files {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut w, bytes).unwrap();
    }
    w.finish().unwrap();
}

#[test]
fn takeout_zips_are_read_where_they_are() {
    let t = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&t.path().join("journey"), "Test").unwrap();
    let scratch = t.path().join("scratch");
    let pic = std::fs::read(gif(&scratch, "a.gif", 7)).unwrap();
    let pic2 = std::fs::read(gif(&scratch, "b.gif", 8)).unwrap();
    let side = serde_json::to_vec(&json!({"title": "IMG_0001.gif", "description": "At the lake",
        "photoTakenTime": {"timestamp": "1562846400"}})).unwrap();
    // Google put the photo in part 1 and its sidecar in part 2.
    let downloads = t.path().join("Downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let part1 = downloads.join("takeout-20261101-001.zip");
    let part2 = downloads.join("takeout-20261101-002.zip");
    zip_of(&part1, &[("Takeout/Google Photos/Lake trip/IMG_0001.gif", pic), ("Takeout/archive_browser.html", b"<html>".to_vec())]);
    zip_of(&part2, &[("Takeout/Google Photos/Lake trip/IMG_0001.gif.json", side), ("Takeout/Google Photos/Lake trip/IMG_0002.gif", pic2)]);
    let before = (std::fs::read(&part1).unwrap(), std::fs::read(&part2).unwrap());

    let ctx = JobContext::new();
    let plan = bulk::scan(&j, &downloads, ScanOptions { story_per_folder: true, ..ScanOptions::default() }, &ctx).unwrap();
    assert_eq!(plan.rows.len(), 2, "{:#?}", plan.rows);
    let a = plan.rows.iter().find(|r| r.file.ends_with("IMG_0001.gif")).unwrap();
    assert_eq!(a.file, "takeout-20261101-001.zip/Takeout/Google Photos/Lake trip/IMG_0001.gif");
    assert_eq!((a.date.as_str(), a.description.as_str(), a.story.as_str()), ("2019-07-11", "At the lake", "Lake trip"));
    assert!(ctx.skipped().iter().any(|s| s.item.ends_with("archive_browser.html")));

    // The same zips chosen one by one give the same plan.
    let known = bulk::Known::read(&j).unwrap();
    let picked = bulk::scan_zips(&known, j.root(), &[part1.clone(), part2.clone()], ScanOptions { story_per_folder: true, ..ScanOptions::default() }, &JobContext::new()).unwrap();
    assert_eq!(picked.rows, plan.rows);
    assert!(bulk::scan_zips(&known, j.root(), &[downloads.clone()], ScanOptions::default(), &JobContext::new()).is_err());
    let files = bulk::plan_files(&plan);
    assert!(bulk::total_size(&files) > 80, "sizes come from inside the zips");
    let batch = mdp_core::import::prepare(j.root(), &files, &JobContext::new()).unwrap();
    let report = j.finish_plan(batch, &plan, false).unwrap();
    assert_eq!(report.imported, 2);
    assert_eq!(count(&j, "SELECT count(*) FROM memory WHERE caption = 'At the lake' AND date = '2019-07-11'"), 1);
    let mut st = j.conn().prepare("SELECT path FROM content_file").unwrap();
    let paths: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    let beside: Vec<PathBuf> = paths.iter().map(|p| j.root().join(format!("{p}.json"))).filter(|p| p.exists()).collect();
    assert_eq!(beside.len(), 1, "the sidecar from part 2 sits beside the photo from part 1");
    assert!(std::fs::read_to_string(&beside[0]).unwrap().contains("At the lake"));
    assert_eq!((std::fs::read(&part1).unwrap(), std::fs::read(&part2).unwrap()), before, "the zips are untouched");
    // Nothing was unpacked beside the zips.
    assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 2);
}

#[test]
fn unzipped_takeout_parts_pair_across_folders() {
    // Names from a real Takeout (2026-11-01): each part unzipped into its own folder.
    let t = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&t.path().join("journey"), "Test").unwrap();
    let base = t.path().join("New folder");
    let album = |part: &str| base.join(format!("takeout-20261101T214823Z-1-{part}/Takeout/Google Photos/Lucy Special Album"));
    let (a1, a2) = (album("001"), album("002"));
    std::fs::create_dir_all(&a1).unwrap();
    let noon = "1562846400";
    sidecar(&a1, "20230801_113242.jpg.supplemental-metadata.json", json!({"title": "20230801_113242.jpg", "description": "Lucy at the park", "photoTakenTime": {"timestamp": noon}}));
    sidecar(&a1, "PXL_20210130_195150249.MP.jpg.supplemental-metadata.json", json!({"title": "PXL_20210130_195150249.MP.jpg", "photoTakenTime": {"timestamp": noon}}));
    sidecar(&a1, "metadata.json", json!({"title": "Lucy Special Album"}));
    std::fs::write(a1.join("PXL_20210130_195150249.MP"), b"\0\0\0\x18ftypmp42").unwrap();
    std::fs::write(a1.join("PXL_20241026_025223701.RAW-01.MP.COVER"), b"cover").unwrap();
    gif(&a2, "20230801_113242.jpg", 1);
    gif(&a2, "PXL_20210130_195150249.MP.jpg", 2);

    let ctx = JobContext::new();
    let plan = bulk::scan(&j, &base, ScanOptions::default(), &ctx).unwrap();
    assert_eq!(plan.rows.len(), 2, "{:#?}", plan.rows);
    assert!(plan.rows.iter().all(|r| r.sidecar.contains("-001")), "both found their sidecar in the other part: {:#?}", plan.rows);

    assert!(plan.rows.iter().any(|r| r.description == "Lucy at the park"));
    assert!(ctx.skipped().is_empty(), "sidecars, the album's metadata.json and motion-photo parts are passed over quietly: {:?}", ctx.skipped());

    let batch = mdp_core::import::prepare(j.root(), &bulk::plan_files(&plan), &JobContext::new()).unwrap();
    assert_eq!(j.finish_plan(batch, &plan, false).unwrap().imported, 2);
    let mut st = j.conn().prepare("SELECT path FROM content_file").unwrap();
    let paths: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(paths.iter().filter(|p| j.root().join(format!("{p}.json")).exists()).count(), 2, "each sidecar from part 001 sits beside its photo from part 002");
}

/// Names found: each name once; a choice counts for every file with that
/// name, the notes go once a name is matched, and the import follows the
/// choices (an existing person, a new one, or left out).
#[test]
fn names_found_are_matched_once_for_every_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("journey"), "Test").unwrap();
    let june = j.add_individual("person", "June Rowan").unwrap();
    let sam1 = j.add_individual("person", "Sam Lee").unwrap();
    j.add_individual("person", "Sam Lee").unwrap();
    j.add_individual("person", "Diana Dickson").unwrap();
    let src = dir.path().join("photos");
    let files: Vec<PathBuf> = (1..=3).map(|n| gif(&src, &format!("p{n}.gif"), n)).collect();
    let row = |f: &PathBuf, people: &str| bulk::PlanRow { file: f.display().to_string(), include: true, title: "t".into(), people: people.into(), kind: "picture".into(), ..Default::default() };
    let mut plan = bulk::Plan {
        base: String::new(),
        rows: vec![
            row(&files[0], "Grandma Jo; Sam Lee; Bob Newman"),
            row(&files[1], "Grandma Jo (in the picture); June Rowan"),
            row(&files[2], "Stranger; Diana"),
        ],
        names: vec![],
        places: vec![],
        coverage: None,
    };
    let known = bulk::Known::read(&j).unwrap();
    let found = bulk::check_names(&mut plan, &known);
    let by = |n: &str| found.iter().find(|f| f.name == n).unwrap().clone();
    assert_eq!(found.len(), 6, "{found:#?}");
    assert_eq!(by("Diana").status, "known", "Google tags by first name; one person has it");
    assert_eq!((by("Grandma Jo").files, by("Grandma Jo").status.as_str()), (2, "new"));
    assert_eq!(by("Sam Lee").status, "several");
    assert_eq!(by("June Rowan").status, "known");
    assert_eq!(found.last().unwrap().name, "June Rowan", "names needing a choice come first");
    assert!(plan.rows[0].people_note.contains("More than one person is called Sam Lee"));
    assert!(plan.rows[1].people_note.contains("Not in this Journey yet: Grandma Jo."));

    let choose = |name: &str, action: &str, id: &str| bulk::NameChoice { name: name.into(), action: action.into(), id: id.into(), remember: true };
    plan.names = vec![choose("grandma jo", "person", &june), choose("Sam Lee", "person", &sam1), choose("Bob Newman", "new", ""), choose("Stranger", "leave", "")];
    let found = bulk::check_names(&mut plan, &known);
    assert!(plan.rows.iter().all(|r| r.people_note.is_empty()), "{:#?}", plan.rows);
    assert!(found.iter().filter(|f| f.status != "known").all(|f| f.chosen));

    let batch = mdp_core::import::prepare(j.root(), &bulk::plan_files(&plan), &JobContext::new()).unwrap();
    let report = j.finish_plan(batch, &plan, false).unwrap();
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    let who = |m: &str| -> Vec<String> {
        let mut st = j.conn().prepare("SELECT i.name FROM memory_individual mi JOIN individual i ON i.id = mi.individual_id WHERE mi.memory_id = ?1 ORDER BY i.name").unwrap();
        st.query_map([m], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(who(&report.memories[0]), ["Bob Newman", "June Rowan", "Sam Lee"]);
    assert_eq!(who(&report.memories[1]), ["June Rowan"], "Grandma Jo is June, linked once");
    assert_eq!(who(&report.memories[2]), ["Diana Dickson"], "Stranger was left out; Diana is Diana Dickson");
    let linked_sam: String = j.conn().query_row("SELECT individual_id FROM memory_individual WHERE memory_id = ?1 AND individual_id IN (SELECT id FROM individual WHERE name = 'Sam Lee')", [&report.memories[0]], |r| r.get(0)).unwrap();
    assert_eq!(linked_sam, sam1);
    let other: String = j.conn().query_row("SELECT other_names FROM person WHERE id = ?1", [&june], |r| r.get(0)).unwrap();
    assert_eq!(other, "grandma jo", "kept as another name, so the next import matches it");
    assert_eq!(bulk::Known::read(&j).unwrap().matches("Grandma Jo").len(), 1);
}

/// Places found: files with Google locations within a kilometer are one
/// group, named after a place in the Journey nearby or the nearest town
/// from the list built in; a choice fills every file in the group, and a
/// new place keeps the location.
#[test]
fn places_found_group_nearby_locations() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("journey"), "Test").unwrap();
    let house = j.add_place("The Rowan house").unwrap();
    j.update_place(&house, &json!({"coordinates": "43.6150, -116.2023"}).as_object().cloned().unwrap()).unwrap();
    let src = dir.path().join("photos");
    let files: Vec<PathBuf> = (1..=5).map(|n| gif(&src, &format!("p{n}.gif"), n)).collect();
    let at = |f: &PathBuf, la: f64, lo: f64| bulk::PlanRow { file: f.display().to_string(), include: true, title: "t".into(), kind: "picture".into(), latitude: Some(la), longitude: Some(lo), ..Default::default() };
    let mut plan = bulk::Plan {
        base: String::new(),
        rows: vec![
            at(&files[0], 43.6151, -116.2024), // the Rowan house
            at(&files[1], 43.4927, -112.0401), // Idaho Falls
            at(&files[2], 43.4930, -112.0410), // Idaho Falls, 80 m away
            at(&files[3], 0.0, -150.0),        // the middle of the Pacific
            bulk::PlanRow { place: "Grandma's".into(), ..at(&files[4], 43.4927, -112.0401) }, // typed: kept
        ],
        names: vec![],
        places: vec![],
        coverage: None,
    };
    let known = bulk::Known::read(&j).unwrap();
    let found = bulk::check_places(&mut plan, &known, true);
    assert_eq!(found.len(), 3, "{found:#?}");
    let falls = found.iter().find(|f| f.status == "town").unwrap();
    assert_eq!((falls.files, falls.suggestion.as_str()), (2, "Idaho Falls, Idaho, United States"));
    assert_eq!(found.iter().find(|f| f.status == "known").unwrap().suggestion, "The Rowan house");
    assert!(found.iter().any(|f| f.status == "none"));
    assert_eq!(found.last().unwrap().status, "known", "groups needing a choice come first");
    assert_eq!(plan.rows[0].place, "The Rowan house");
    assert!(plan.rows[1].place.is_empty() && plan.rows[1].place_note.contains("near Idaho Falls"));
    assert_eq!(plan.rows[4].place, "Grandma's");
    // Without the town list, nothing is suggested.
    let mut off = plan.clone();
    assert!(bulk::check_places(&mut off, &known, false).iter().all(|f| f.status != "town"));

    let key = falls.key.clone();
    let pacific = found.iter().find(|f| f.status == "none").unwrap().key.clone();
    plan.places = vec![
        bulk::PlaceChoice { key, action: "new".into(), name: "Idaho Falls, Idaho".into() },
        bulk::PlaceChoice { key: pacific, action: "leave".into(), name: String::new() },
    ];
    bulk::check_places(&mut plan, &known, true);
    assert!(plan.rows.iter().all(|r| r.place_note.is_empty()), "{:#?}", plan.rows);
    assert_eq!((plan.rows[1].place.as_str(), plan.rows[2].place.as_str()), ("Idaho Falls, Idaho", "Idaho Falls, Idaho"));

    let batch = mdp_core::import::prepare(j.root(), &bulk::plan_files(&plan), &JobContext::new()).unwrap();
    let report = j.finish_plan(batch, &plan, false).unwrap();
    assert_eq!(report.notes, ["The place \u{201c}Grandma's\u{201d} is not in this Journey, so it was left out."], "only the typed one");
    let (n, la): (i64, f64) = j.conn().query_row("SELECT count(*), max(latitude) FROM place WHERE name = 'Idaho Falls, Idaho'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(n, 1, "one new place for the group");
    assert!((la - 43.4927).abs() < 0.001, "it keeps the location");
    let placed: i64 = j.conn().query_row("SELECT count(*) FROM memory WHERE place_id IS NOT NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(placed, 3, "the Rowan house and the two Idaho Falls photos");
}
