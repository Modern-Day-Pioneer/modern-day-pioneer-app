//! Import rules: an ordered, editable rules file that fills in a bulk import
//! plan from folder and file names (story, family, memory, date, place,
//! people, series and who took the pictures), with a coverage report.
//!
//! Every name here is made up. The folders follow the same patterns as a
//! real family history collection: one top folder per person or couple,
//! era folders, one-day folders, event folders, dated file names, series
//! numbers, scans and edited copies.

use std::path::{Path, PathBuf};

use mdp_core::bulk::{self, ScanOptions};
use mdp_core::jobs::JobContext;
use mdp_core::rules::{read_date, split_names, leading_names, text_date, RuleSet, BUILT_IN};
use mdp_core::Journey;

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

/// The built-in rules plus a made-up family's names, initials, fixes and settings.
fn rowan_rules() -> String {
    let tail = "\n# Names\n- Rosa\n- Ellen\n- Mark\n- Walter\n- Lily\n\n# Initials\nWJR = Walter J. Rowan\n\n# Spelling fixes\nVaccation = Vacation\n\n# Skip folders\n- Edited\n\n# Settings\nday 01 means month only: yes\nlater photo years: 10\n";
    let head = BUILT_IN.split("\n# Names").next().unwrap();
    format!("{head}{tail}")
}

fn apply(set: &RuleSet, path: &str, photo: Option<&str>) -> mdp_core::rules::Outcome {
    let segs: Vec<String> = path.split('/').map(str::to_string).collect();
    let mut o = set.apply(&segs, &|n: &str| n == "June");
    set.finish(&mut o, photo);
    o
}

fn field<'a>(o: &'a mdp_core::rules::Outcome, k: &str) -> &'a str {
    o.fields.get(k).map(String::as_str).unwrap_or("")
}

#[test]
fn the_rules_file_is_read() {
    let set = RuleSet::built_in();
    assert!(set.problems.is_empty(), "{:?}", set.problems);
    assert_eq!(set.name, "Family history folders");
    assert!(set.rules.len() >= 20, "{}", set.rules.len());
    assert!(!set.day01_unknown);
    assert_eq!(set.later_photo_years, 10);

    let set = RuleSet::parse(&rowan_rules());
    assert!(set.problems.is_empty(), "{:?}", set.problems);
    assert_eq!(set.names.len(), 5);
    assert_eq!(set.initials, vec![("WJR".to_string(), "Walter J. Rowan".to_string())]);
    assert_eq!(set.fixes.len(), 1);
    assert_eq!(set.skip, vec!["Edited".to_string()]);
    assert!(set.day01_unknown);

    // A rule that can't be read is left out and explained; the rest still work.
    let bad = "# Import rules: Test\n\n## 1. Broken\napplies to: folder\npattern: ^(unclosed\n\n## 2. Unknown field\napplies to: file name\npattern: .*\nsets: colour = red\n\n## 3. Fine\napplies to: file name\npattern: ^(?<t>.+)$\nsets: memory = {t}\n\n## 4. Switched off\napplies to: file name\npattern: .*\nsets: memory = Never\noff\n";
    let set = RuleSet::parse(bad);
    assert_eq!(set.problems.len(), 2, "{:?}", set.problems);
    assert!(set.problems[0].contains("rule 1"));
    assert!(set.problems[1].contains("colour"));
    assert_eq!(set.rules.len(), 2);
    let o = apply(&set, "Trips/Lake day.jpg", None);
    assert_eq!(field(&o, "memory"), "Lake day");
}

#[test]
fn dates_names_and_text() {
    assert_eq!(read_date("19650312", false).as_deref(), Some("1965-03-12"));
    assert_eq!(read_date("19650301", true).as_deref(), Some("1965-03"));
    assert_eq!(read_date("19650101", true).as_deref(), Some("1965"));
    assert_eq!(read_date("19650301", false).as_deref(), Some("1965-03-01"));
    assert_eq!(read_date("196503", false).as_deref(), Some("1965-03"));
    assert_eq!(read_date("1961 to 1970", false).as_deref(), Some("1961 to 1970"));
    assert_eq!(read_date("19__", false), None);
    assert_eq!(text_date("Party [30JAN1957]", false).as_deref(), Some("1957-01-30"));
    assert_eq!(text_date("Picnic 8JUL1991", false).as_deref(), Some("1991-07-08"));
    assert_eq!(text_date("Christmas Oct1976", false).as_deref(), Some("1976-10"));
    assert_eq!(text_date("Farm 1960's", false).as_deref(), Some("1960s"));
    assert_eq!(text_date("Old ones 19__", false), None);
    assert_eq!(text_date("Album [1949]", false).as_deref(), Some("1949"));
    assert_eq!(split_names("Ellen, Mark and Walter Rowan"), vec!["Ellen Rowan", "Mark Rowan", "Walter Rowan"]);
    assert_eq!(split_names("Walter and June"), vec!["Walter", "June"]);
    assert_eq!(split_names("Owen & Ellen Hale"), vec!["Owen Hale", "Ellen Hale"]);
    assert_eq!(leading_names("Ellen Rowan 11th birthday"), "Ellen Rowan");
    assert_eq!(leading_names("Christmas Oct1976"), "Christmas");
}

#[test]
fn family_history_folders() {
    let set = RuleSet::parse(&rowan_rules());

    // The top folder is the Story and the Family; its names are People.
    let o = apply(&set, "Walter and June/1961-1970 (High School, College)/19650301-Ellen, Mark and Walter Rowan-Rowan home in Provo, UT.jpg", None);
    assert_eq!(field(&o, "story"), "Walter and June");
    assert_eq!(field(&o, "family"), "Walter and June family");
    // The creator's own scheme: day 01 means only the month is known.
    assert_eq!(field(&o, "date"), "1965-03");
    assert_eq!(field(&o, "memory"), "Ellen, Mark and Walter Rowan");
    assert_eq!(field(&o, "place"), "Rowan home in Provo, UT");
    assert_eq!(o.people, vec!["Walter", "June", "Ellen Rowan", "Mark Rowan", "Walter Rowan"]);
    assert!(o.rules.iter().any(|r| r.starts_with("11.")), "{:?}", o.rules);
    let single = apply(&set, "Ellen/Montana Trip/IMG_6511.jpg", None);
    assert_eq!(field(&single, "family"), "Ellen's family");

    // A series under an event folder: the folder names the memory.
    let o = apply(&set, "Walter and June/2015 Family/Tolver (14).jpg", None);
    assert_eq!((field(&o, "memory"), field(&o, "series"), field(&o, "date")), ("2015 Family", "14", "2015"));
    assert_eq!(o.to_confirm, vec!["Tolver"]);
    // Under a one-day folder the series' name names the memory.
    let o = apply(&set, "Walter and June/2014 03 05/Lily Blessing_0021.jpg", None);
    assert_eq!((field(&o, "memory"), field(&o, "series"), field(&o, "date")), ("Lily Blessing", "21", "2014-03-05"));

    // Camera files take their folder's memory, and one memory per day when nothing else names it.
    assert_eq!(field(&apply(&set, "Walter and June/Montana Trip/IMG_6511.jpg", None), "memory"), "Montana Trip");
    let o = apply(&set, "Walter and June/2010-2019 Kimball/20190628_103744.jpg", None);
    assert_eq!((field(&o, "memory"), field(&o, "date")), ("June 28, 2019", "2019-06-28"));
    let o = apply(&set, "Walter and June/2010-2019 Kimball/IMG_0040.jpg", Some("2015-04-03"));
    assert_eq!(field(&o, "memory"), "April 3, 2015");
    let o = apply(&set, "Walter and June/2010-2019 Kimball/IMG_0041.jpg", None);
    assert_eq!(o.memory_rule, None, "no day, no memory");

    // Who took or gave the pictures is credited, not added to People.
    let o = apply(&set, "Walter and June/2018 Alaska/Rosa/IMG_0001.jpg", None);
    assert_eq!((field(&o, "memory"), field(&o, "photo_by")), ("2018 Alaska", "Rosa"));
    assert!(!o.people.iter().any(|p| p.starts_with("Rosa")));
    let o = apply(&set, "Ellen/2014 Reunion - Walter/IMG_0002.jpg", None);
    assert_eq!((field(&o, "memory"), field(&o, "photo_by")), ("2014 Reunion", "Walter"));
    assert_eq!(o.people, vec!["Ellen"]);
    assert_eq!(field(&apply(&set, "Walter and June/From Nora/Lake day.jpg", None), "photo_by"), "Nora");

    // Edited copies join their original; technical and listed folders are skipped.
    let o = apply(&set, "Walter and June/Vivid/19760701-Ellen Rowan_Vivid_ful.jpg", None);
    assert_eq!((field(&o, "memory"), field(&o, "date")), ("Ellen Rowan", "1976-07"));
    assert!(o.edition);
    let o = apply(&set, "Walter and June/Montana Trip/Edited/IMG_6512.jpg", None);
    assert_eq!(field(&o, "memory"), "Montana Trip");

    // A scan batch's year is when it was scanned: the era folder's years stand.
    let o = apply(&set, "Walter and June/2010-2019 Kimball/2024_0012_a.jpg", Some("2024-05-01"));
    assert_eq!((field(&o, "date"), field(&o, "series")), ("2010 to 2019", "12"));
    assert!(!o.photo_date);

    // A later photo of an earlier time keeps the era's years, with a note.
    let o = apply(&set, "Walter and June/1970-1972 (Army)/20201126-Barracks room, Dover, DE.jpg", None);
    assert_eq!(field(&o, "date"), "1970 to 1972");
    assert!(field(&o, "note").contains("November 26, 2020"), "{:?}", o.fields);
    // A few years out is only flagged.
    let o = apply(&set, "Walter and June/1970-1972 (Army)/IMG_3.jpg", Some("1975-02-02"));
    assert!(field(&o, "note").contains("outside its folder's years"), "{:?}", o.fields);

    // Ultrasound pictures, album pages and initials.
    let o = apply(&set, "Walter and June/Baby/US IMAGES_7.jpg", None);
    assert_eq!((field(&o, "memory"), field(&o, "series")), ("Ultrasound pictures", "7"));
    let o = apply(&set, "Walter and June/WJR Small Photo Album/WJR SPA P27c1 [1949].jpg", None);
    assert_eq!((field(&o, "memory"), field(&o, "series"), field(&o, "date")), ("WJR Small Photo Album", "27c1", "1949"));
    assert!(o.people.contains(&"Walter J. Rowan".to_string()));

    // Spelling fixes, a top folder that isn't a family, couple folders and undated names.
    assert_eq!(field(&apply(&set, "Ellen/Vaccation 2002/IMG_1.jpg", None), "memory"), "Vacation 2002");
    let o = apply(&set, "Pine Street Photos/beach.jpg", None);
    assert_eq!((field(&o, "story"), field(&o, "family")), ("Pine Street Photos", ""));
    assert!(field(&o, "note").contains("doesn't look like a family"));
    let o = apply(&set, "Ellen/Owen and Ellen/Rowans/Christmas Oct1976.jpg", None);
    assert_eq!((field(&o, "memory"), field(&o, "date")), ("Christmas", "1976-10"));
    assert_eq!(o.people, vec!["Ellen", "Owen"]);
    let o = apply(&set, "Ellen/Old ones 19__.jpg", None);
    assert_eq!((field(&o, "memory"), field(&o, "date")), ("Old ones", ""));
}

#[test]
fn a_folder_with_its_own_rules_file() {
    let t = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&t.path().join("journey"), "Test").unwrap();
    j.add_individual("person", "June Rowan").unwrap();
    let src = t.path().join("History");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join(mdp_core::rules::FOLDER_FILE), rowan_rules()).unwrap();
    gif(&src, "Walter and June/2015 Family/Tolver (2).gif", 1);
    gif(&src, "Walter and June/2015 Family/Tolver (10).gif", 2);
    gif(&src, "Walter and June/2015 Family/Tolver (1).gif", 3);
    gif(&src, "Walter and June/2018 Alaska/Rosa/IMG_0001.gif", 4);
    gif(&src, "Walter and June/1961-1970 (School)/19650301-Ellen and Walter Rowan-Rowan home in Provo, UT.gif", 5);
    gif(&src, "Walter and June/2010-2019 Kimball/IMG_0041.gif", 6);

    let ctx = JobContext::new();
    let plan = bulk::scan(&j, &src, ScanOptions::default(), &ctx).unwrap();
    // The rules file itself is not brought in.
    assert_eq!(plan.rows.len(), 6, "{:?}", plan.rows.iter().map(|r| &r.file).collect::<Vec<_>>());
    let row = |f: &str| plan.rows.iter().find(|r| r.file.ends_with(f)).unwrap();
    let r = row("Tolver (10).gif");
    assert_eq!((r.memory.as_str(), r.series.as_str(), r.story.as_str(), r.family.as_str()), ("2015 Family", "10", "Walter and June", "Walter and June family"));
    assert!(r.rule.contains("17. Series under an event folder"), "{}", r.rule);
    assert_eq!(r.date_source, "import rule 7");
    let r = row("IMG_0001.gif");
    assert_eq!((r.memory.as_str(), r.photo_by.as_str()), ("2018 Alaska", "Rosa"));
    // June is in the Journey, so the top folder's "June" is her.
    assert!(r.people.contains("June Rowan"), "{}", r.people);
    let r = row("Provo, UT.gif");
    assert_eq!((r.date.as_str(), r.place.as_str()), ("1965-03", "Rowan home in Provo, UT"));

    // Coverage: what each rule caught and the files no rule caught.
    let c = plan.coverage.as_ref().unwrap();
    assert_eq!(c.files, 6);
    assert_eq!(c.name, "Family history folders");
    assert!(c.source.ends_with(mdp_core::rules::FOLDER_FILE));
    let named = |label: &str| c.rules.iter().find(|r| r.0.starts_with(label)).map(|r| r.1).unwrap_or(0);
    assert_eq!(named("17."), 3);
    assert_eq!(named("15."), 1);
    assert_eq!(named("11."), 1);
    assert_eq!(c.uncaught, vec!["Walter and June/2010-2019 Kimball/IMG_0041.gif".to_string()]);
    let report = c.text();
    assert!(report.contains("FILES NO RULE CAUGHT (1)") && report.contains("NAMES TO CONFIRM (1)") && report.contains("Tolver (3)"), "{report}");

    // The new columns are saved, and read back.
    let csv_path = t.path().join("plan.csv");
    let csv = bulk::to_csv(&plan, &csv_path);
    assert!(csv.lines().next().unwrap().ends_with("description,family,series,photo_by,rule"));
    std::fs::write(&csv_path, &csv).unwrap();
    let back = bulk::from_csv(&csv, &csv_path).unwrap();
    let b = back.rows.iter().find(|r| r.file.ends_with("Tolver (10).gif")).unwrap();
    assert_eq!((b.family.as_str(), b.series.as_str()), ("Walter and June family", "10"));
    assert!(b.rule.contains("17."));

    // An older plan without the new columns still opens.
    let old = "file,include,title,date,people,place,memory,story,description\r\nHistory/Walter and June/2015 Family/Tolver (1).gif,yes,,2015,,,2015 Family,,\r\n";
    let old_plan = bulk::from_csv(old, &csv_path).unwrap();
    assert_eq!(old_plan.rows.len(), 1);
    assert_eq!((old_plan.rows[0].family.as_str(), old_plan.rows[0].series.as_str()), ("", ""));
    assert!(old_plan.rows[0].note.is_empty(), "{}", old_plan.rows[0].note);

    // Without rules the folder imports as before.
    let plain = bulk::scan(&j, &src, ScanOptions { rules: "none".into(), ..ScanOptions::default() }, &JobContext::new()).unwrap();
    assert!(plain.coverage.is_none());
    assert!(plain.rows.iter().all(|r| r.family.is_empty() && r.rule.is_empty() && r.memory.is_empty()));

    // Bring it in: the family is made with its members, the series sets the
    // order, and who took the pictures goes in the description.
    let batch = mdp_core::import::prepare(j.root(), &bulk::plan_files(&plan), &ctx).unwrap();
    let report = j.finish_plan(batch, &plan, true).unwrap();
    assert_eq!(report.imported, 6);
    let c = j.conn();
    let family: String = c.query_row("SELECT id FROM family WHERE name = 'Walter and June family'", [], |r| r.get(0)).unwrap();
    let members: Vec<String> = c
        .prepare("SELECT i.name FROM family_member f JOIN individual i ON i.id = f.individual_id WHERE f.family_id = ?1 ORDER BY i.name")
        .unwrap()
        .query_map([&family], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(members, vec!["June Rowan", "Walter", "Walter Rowan"]);
    let order: Vec<String> = c
        .prepare("SELECT f.original_name FROM memory m JOIN memory_content mc ON mc.memory_id = m.id JOIN content_file f ON f.content_id = mc.content_id AND f.is_copy = 0 WHERE m.name = '2015 Family' ORDER BY mc.position")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(order, vec!["Tolver (1).gif", "Tolver (2).gif", "Tolver (10).gif"]);
    let caption: String = c.query_row("SELECT caption FROM memory WHERE name = '2018 Alaska'", [], |r| r.get(0)).unwrap();
    assert_eq!(caption, "Pictures from Rosa.");
    let stories: i64 = c.query_row("SELECT COUNT(*) FROM story WHERE name = 'Walter and June'", [], |r| r.get(0)).unwrap();
    assert_eq!(stories, 1);
}

#[test]
fn built_in_and_chosen_rules() {
    let t = tempfile::tempdir().unwrap();
    assert!(RuleSet::choose(t.path(), "").unwrap().is_none());
    assert!(RuleSet::choose(t.path(), "none").unwrap().is_none());
    assert_eq!(RuleSet::choose(t.path(), "built in").unwrap().unwrap().source, "built in");
    let f = t.path().join("mine.md");
    std::fs::write(&f, "## 1. Everything\napplies to: file name\npattern: .+\nsets: memory = All\n").unwrap();
    let mine = RuleSet::choose(t.path(), &f.display().to_string()).unwrap().unwrap();
    assert_eq!(mine.name, "mine");
    assert!(RuleSet::choose(t.path(), &t.path().join("missing.md").display().to_string()).is_err());
}
