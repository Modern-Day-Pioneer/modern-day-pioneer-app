//! Sprint 3: people, pets, families, places and events.

use mdp_core::dates;
use mdp_core::people::is_living;
use mdp_core::{Journey, Row};
use serde_json::json;

fn journey(dir: &std::path::Path) -> Journey {
    Journey::create(&dir.join("J"), "Rowans").unwrap()
}

fn changes(v: serde_json::Value) -> Row {
    v.as_object().cloned().unwrap()
}

fn count(j: &Journey, sql: &str) -> i64 {
    j.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn dates_typed_as_known() {
    let cases = [
        ("1956", "1956", "1956", "1956"),
        ("June 1956", "1956-06", "1956-06", "June 1956"),
        ("June 2, 1956", "1956-06-02", "1956-06-02", "June 2, 1956"),
        ("2 June 1956", "1956-06-02", "1956-06-02", "June 2, 1956"),
        ("6/2/1956", "1956-06-02", "1956-06-02", "June 2, 1956"),
        ("1956-06-02", "1956-06-02", "1956-06-02", "June 2, 1956"),
        ("about 1930", "1930~", "1930", "about 1930"),
        ("c. 1930", "1930~", "1930", "about 1930"),
        ("maybe 1945", "1945?", "1945", "maybe 1945"),
        ("1945?", "1945?", "1945", "maybe 1945"),
        ("1970s", "197X", "1970", "the 1970s"),
        ("1960 to 1965", "1960/1965", "1960", "1960 to 1965"),
        ("1960-1965", "1960/1965", "1960", "1960 to 1965"),
        ("between 1960 and 1965", "1960/1965", "1960", "1960 to 1965"),
        ("before 1900", "../1900", "1900", "before 1900"),
        ("after 1950", "1950/..", "1950", "after 1950"),
        ("196X", "196X", "1960", "the 1960s"),
    ];
    for (typed, edtf, nearest, words) in cases {
        let d = dates::parse(typed).unwrap().unwrap_or_else(|| panic!("{typed}"));
        assert_eq!(d.edtf, edtf, "{typed}");
        assert_eq!(d.nearest.as_deref(), Some(nearest), "{typed}");
        assert_eq!(d.words, words, "{typed}");
    }
    assert!(dates::parse("  ").unwrap().is_none());
    for bad in ["last spring", "June 31, 1956", "13/1/1956", "1965 to 1960", "grandma's birthday"] {
        assert!(dates::parse(bad).is_err(), "{bad} should be refused");
    }
}

#[test]
fn living_rule_and_override() {
    assert!(is_living(None, None, None, 2026));
    assert!(is_living(None, Some("1958"), None, 2026));
    assert!(!is_living(None, Some("1900"), None, 2026));
    assert!(!is_living(Some("1999"), Some("1958"), None, 2026));
    assert!(is_living(Some("1999"), None, Some("living"), 2026));
    assert!(!is_living(None, Some("1990"), Some("not living"), 2026));
}

#[test]
fn a_person_is_always_two_rows_and_undo_removes_both() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = journey(dir.path());
    let june = j.add_individual("person", "June Rowan").unwrap();
    let pepper = j.add_individual("pet", "Pepper").unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM individual"), 2);
    assert_eq!(count(&j, "SELECT count(*) FROM person"), 1);
    assert_eq!(count(&j, "SELECT count(*) FROM pet"), 1);
    let v = j.individual_view(&june).unwrap();
    assert_eq!((v.given_name.as_deref(), v.family_name.as_deref()), (Some("June"), Some("Rowan")));
    assert_eq!(j.individual_view(&pepper).unwrap().kind, "Pet");
    // Undo the pet: both of its rows go together.
    j.undo().unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM individual"), 1);
    assert_eq!(count(&j, "SELECT count(*) FROM pet"), 0);
    assert!(j.add_individual("person", "  ").is_err());
    assert!(j.add_individual("robot", "R2").is_err());
    // Quick-add shows up in search straight away.
    let hits = j.search("jun", 10).unwrap();
    assert!(hits.iter().any(|h| h.id == june && h.kind == "person"));
}

#[test]
fn editing_a_person_is_one_action_and_born_makes_the_birth_event() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = journey(dir.path());
    let june = j.add_individual("person", "June").unwrap();
    j.update_individual(&june, &changes(json!({"family_name": "Rowan", "gender": "female", "born": "about 1931", "description": "Kept a scrapbook."})))
        .unwrap();
    let v = j.individual_view(&june).unwrap();
    assert_eq!(v.born.as_ref().unwrap().edtf, "1931~");
    assert_eq!(v.gender.as_deref(), Some("female"));
    let birth = v.birth_event.clone().expect("a birth event is made");
    let e = j.event_view(&birth).unwrap();
    assert_eq!(e.name, "Birth of June");
    assert_eq!(e.start.unwrap().edtf, "1931~");
    // Changing the event's date changes the person's, in step.
    j.update_event(&birth, &changes(json!({"start": "April 12, 1931"}))).unwrap();
    assert_eq!(j.individual_view(&june).unwrap().born.unwrap().edtf, "1931-04-12");
    let stored: String = j.conn().query_row("SELECT birth_date FROM individual WHERE id = ?1", [&june], |r| r.get(0)).unwrap();
    assert_eq!(stored, "1931-04-12");
    // Renaming keeps the event's name in step.
    j.update_individual(&june, &changes(json!({"name": "June Rowan"}))).unwrap();
    assert_eq!(j.event_view(&birth).unwrap().name, "Birth of June Rowan");
    // A bad date changes nothing, and the error gives examples.
    let err = j.update_individual(&june, &changes(json!({"name": "X", "died": "someday"}))).unwrap_err().to_string();
    assert!(err.contains("Try a year"), "{err}");
    assert_eq!(j.individual_view(&june).unwrap().name, "June Rowan");
    // Undo the rename, then the date change on the event.
    j.undo().unwrap();
    assert_eq!(j.individual_view(&june).unwrap().name, "June");
    assert_eq!(j.event_view(&birth).unwrap().name, "Birth of June");
    j.undo().unwrap();
    assert_eq!(j.individual_view(&june).unwrap().born.unwrap().edtf, "1931~");
    // Pets have no person fields.
    let pepper = j.add_individual("pet", "Pepper").unwrap();
    assert!(j.update_individual(&pepper, &changes(json!({"gender": "female"}))).is_err());
    j.update_individual(&pepper, &changes(json!({"species": "cat", "joined": "1964"}))).unwrap();
    assert_eq!(j.individual_view(&pepper).unwrap().joined.unwrap().edtf, "1964");
}

#[test]
fn relationships_are_stored_once_and_read_from_both_sides() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = journey(dir.path());
    let walter = j.add_individual("person", "Walter Rowan").unwrap();
    let ellen = j.add_individual("person", "Ellen Rowan").unwrap();
    let pepper = j.add_individual("pet", "Pepper").unwrap();
    j.update_individual(&walter, &changes(json!({"gender": "male"}))).unwrap();
    j.update_individual(&ellen, &changes(json!({"gender": "female"}))).unwrap();
    // Entered from Ellen's side: "Ellen is the child of Walter".
    j.add_relationship(&ellen, "child", &walter, None, None, Some("Birth certificate")).unwrap();
    let (from, to, kind): (String, String, String) =
        j.conn().query_row("SELECT from_id, to_id, kind FROM individual_relationship", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!((from.as_str(), to.as_str(), kind.as_str()), (walter.as_str(), ellen.as_str(), "parent"));
    assert_eq!(j.relationships_of(&walter).unwrap()[0].words, "Father of Ellen Rowan");
    assert_eq!(j.relationships_of(&ellen).unwrap()[0].words, "Daughter of Walter Rowan");
    // The same link again, from either side, is refused.
    assert!(j.add_relationship(&walter, "parent", &ellen, None, None, None).is_err());
    assert!(j.add_relationship(&ellen, "child", &walter, None, None, None).is_err());
    // Qualifier and "probably".
    let r = j.relationships_of(&walter).unwrap()[0].id.clone();
    j.update_relationship(&r, &changes(json!({"qualifier": "adoptive", "certainty": "probably"}))).unwrap();
    assert_eq!(j.relationships_of(&ellen).unwrap()[0].words, "Daughter of Walter Rowan (adoptive), probably");
    assert!(j.add_relationship(&walter, "sibling", &pepper, Some("step"), None, None).is_err());
    // A pet can be given any kind, and a family's own term reads from both sides.
    j.add_relationship(&ellen, "best friend", &pepper, None, None, None).unwrap();
    let ps = j.relationships_of(&pepper).unwrap();
    assert_eq!(ps[0].words, "Ellen Rowan is their best friend");
    // No suggestions are ever made in release 1.
    assert_eq!(count(&j, "SELECT count(*) FROM individual_relationship WHERE status <> 'entered'"), 0);
    j.remove_relationship(&r).unwrap();
    assert_eq!(j.relationships_of(&walter).unwrap().len(), 0);
    j.undo().unwrap();
    assert_eq!(j.relationships_of(&walter).unwrap().len(), 1);
    assert!(j.add_relationship(&walter, "parent", &walter, None, None, None).is_err());
}

#[test]
fn families_hold_people_in_several_or_none_and_relate() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = journey(dir.path());
    let june = j.add_individual("person", "June Rowan").unwrap();
    let ada = j.add_individual("person", "Ada Hale").unwrap();
    let frank = j.add_individual("person", "Frank Lenord").unwrap();
    let rowans = j.add_family("The Rowans").unwrap();
    let hales = j.add_family("The Hale family").unwrap();
    j.add_family_member(&rowans, &june).unwrap();
    j.add_family_member(&hales, &june).unwrap();
    j.add_family_member(&hales, &ada).unwrap();
    j.add_family_member(&hales, &ada).unwrap(); // twice changes nothing
    assert_eq!(j.individual_view(&june).unwrap().families.len(), 2);
    assert_eq!(j.family_view(&hales).unwrap().members.len(), 2);
    let groups = j.people_by_family().unwrap();
    let loose = groups.iter().find(|g| g.name.starts_with("Not in a family")).unwrap();
    assert_eq!(loose.members.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec![frank.as_str()]);
    j.relate_families(&rowans, &hales).unwrap();
    j.relate_families(&hales, &rowans).unwrap(); // stored once
    assert_eq!(count(&j, "SELECT count(*) FROM family_relationship"), 1);
    assert_eq!(j.family_view(&hales).unwrap().related[0].name, "The Rowans");
    j.remove_family_member(&hales, &june).unwrap();
    assert_eq!(j.individual_view(&june).unwrap().families.len(), 1);
    j.update_family(&hales, &changes(json!({"name": "Ada's side"}))).unwrap();
    assert!(j.update_family(&hales, &changes(json!({"name": " "}))).is_err());
    j.unrelate_families(&hales, &rowans).unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM family_relationship"), 0);
}

#[test]
fn places_and_events() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = journey(dir.path());
    let walter = j.add_individual("person", "Walter Rowan").unwrap();
    let june = j.add_individual("person", "June Rowan").unwrap();
    let ogden = j.add_place("Ogden, Utah").unwrap();
    j.update_place(&ogden, &changes(json!({"coordinates": "41.223, -111.974", "exact": false, "address": "Ogden, UT"}))).unwrap();
    let p = j.place_view(&ogden).unwrap();
    assert_eq!((p.latitude, p.longitude), (Some(41.223), Some(-111.974)));
    assert!(j.update_place(&ogden, &changes(json!({"coordinates": "200, 5"}))).is_err());
    assert!(j.update_place(&ogden, &changes(json!({"coordinates": "north-ish"}))).is_err());

    let wedding = j.add_event("wedding", None, &[walter.clone(), june.clone()], "June 2, 1956", Some(&ogden)).unwrap();
    let e = j.event_view(&wedding).unwrap();
    assert_eq!(e.name, "Wedding of Walter and June Rowan");
    assert_eq!(e.place.unwrap().name, "Ogden, Utah");
    assert!(j.add_event("party", None, &[], "", None).is_err());
    assert_eq!(j.individual_view(&walter).unwrap().events[0].note, "June 2, 1956 · Ogden, Utah");

    // A birth event added on its own sets the person's birth date and birthplace.
    let birth = j.add_event("birth", None, &[june.clone()], "1931", Some(&ogden)).unwrap();
    let v = j.individual_view(&june).unwrap();
    assert_eq!(v.born.unwrap().edtf, "1931");
    assert_eq!(v.birthplace.unwrap().name, "Ogden, Utah");
    assert_eq!(v.birth_event.as_deref(), Some(birth.as_str()));
    assert!(j.add_event("birth", None, &[june.clone()], "1932", None).is_err());

    // Who took part, in order.
    j.set_event_people(&wedding, &[june.clone(), walter.clone()]).unwrap();
    assert_eq!(j.event_view(&wedding).unwrap().people[0].id, june);
    j.set_event_people(&wedding, &[june.clone()]).unwrap();
    assert_eq!(j.event_view(&wedding).unwrap().people.len(), 1);
    assert!(j.set_event_people(&wedding, &[june.clone(), june.clone()]).is_err());
    let list = j.places_and_events().unwrap();
    assert_eq!(list.places.len(), 1);
    assert_eq!(list.events.len(), 2);
}

#[test]
fn merging_places_moves_every_link_and_undo_puts_them_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = journey(dir.path());
    let a = j.add_place("Ogden, Utah").unwrap();
    let b = j.add_place("Ogden UT").unwrap();
    j.update_place(&b, &changes(json!({"coordinates": "41.2, -111.9"}))).unwrap();
    let e1 = j.add_event("move", Some("The move to Ogden"), &[], "1957", Some(&b)).unwrap();
    // A memory at the duplicate place (memories get screens in sprint 4).
    let m = j.act(|act| act.create("memory", changes(json!({"name": "Summer in Ogden", "place_id": b})))).unwrap();
    j.merge_places(&a, &b).unwrap();
    assert_eq!(j.event_view(&e1).unwrap().place.unwrap().id, a);
    let mp: String = j.conn().query_row("SELECT place_id FROM memory WHERE id = ?1", [&m], |r| r.get(0)).unwrap();
    assert_eq!(mp, a);
    assert_eq!(count(&j, "SELECT count(*) FROM place"), 1);
    // The kept place takes the details it was missing.
    assert_eq!(j.place_view(&a).unwrap().latitude, Some(41.2));
    j.undo().unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM place"), 2);
    assert_eq!(j.event_view(&e1).unwrap().place.unwrap().id, b);
    assert!(j.place_view(&a).unwrap().latitude.is_none());
    assert!(j.merge_places(&a, &a).is_err());
}

#[test]
fn removing_unlinks_only_warns_first_and_undo_restores_everything() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = journey(dir.path());
    let walter = j.add_individual("person", "Walter Rowan").unwrap();
    let june = j.add_individual("person", "June Rowan").unwrap();
    let fam = j.add_family("The Rowans").unwrap();
    j.add_family_member(&fam, &walter).unwrap();
    j.add_family_member(&fam, &june).unwrap();
    j.add_relationship(&walter, "partner", &june, None, None, None).unwrap();
    j.update_individual(&walter, &changes(json!({"born": "1928"}))).unwrap();
    let house = j.add_place("The Rowan house").unwrap();
    let wedding = j.add_event("wedding", None, &[walter.clone(), june.clone()], "1956", Some(&house)).unwrap();
    let (m, story) = j
        .act(|a| {
            let m = a.create("memory", changes(json!({"name": "Walter's camera", "place_id": house, "event_id": wedding})))?;
            a.insert("memory_individual", changes(json!({"memory_id": m, "individual_id": walter, "role": "taken by"})))?;
            let s = a.create("story", changes(json!({"name": "Walter's camera", "focus_id": walter})))?;
            Ok((m, s))
        })
        .unwrap();
    let before = count(&j, "SELECT count(*) FROM history");

    let warning = j.removal_warning(&walter).unwrap();
    let text = warning.join("\n");
    assert!(text.contains("1 relationship with others"), "{text}");
    assert!(text.contains("Membership in The Rowans"), "{text}");
    assert!(text.contains("Birth of Walter Rowan (removed too"), "{text}");
    assert!(text.contains("Taking part in Wedding of Walter and June Rowan (the event stays)"), "{text}");
    assert!(text.contains("Their role in 1 memory (the memories stay)"), "{text}");
    assert!(text.contains("focus of the story"), "{text}");

    j.remove_item(&walter).unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM individual"), 1);
    assert_eq!(count(&j, "SELECT count(*) FROM individual_relationship"), 0);
    assert_eq!(count(&j, "SELECT count(*) FROM event WHERE kind = 'birth'"), 0);
    assert_eq!(j.event_view(&wedding).unwrap().people.len(), 1);
    assert_eq!(count(&j, "SELECT count(*) FROM memory"), 1);
    let focus: Option<String> = j.conn().query_row("SELECT focus_id FROM story WHERE id = ?1", [&story], |r| r.get(0)).unwrap();
    assert!(focus.is_none());

    j.undo().unwrap();
    assert_eq!(j.individual_view(&walter).unwrap().relationships.len(), 1);
    assert_eq!(j.individual_view(&walter).unwrap().born.unwrap().edtf, "1928");
    assert_eq!(j.individual_view(&walter).unwrap().memories.len(), 1);
    assert!(count(&j, "SELECT count(*) FROM history") > before);

    // A place: memories and events stay, without a place.
    assert!(j.removal_warning(&house).unwrap().join(" ").contains("The place of 1 memory"));
    j.remove_item(&house).unwrap();
    assert_eq!(j.event_view(&wedding).unwrap().place, None);
    let mp: Option<String> = j.conn().query_row("SELECT place_id FROM memory WHERE id = ?1", [&m], |r| r.get(0)).unwrap();
    assert!(mp.is_none());

    // An event: memories stay, without an event; a birth date stays on the person.
    j.remove_item(&wedding).unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM event_individual"), 1);
    let birth = j.individual_view(&walter).unwrap().birth_event.unwrap();
    assert!(j.removal_warning(&birth).unwrap().join(" ").contains("birth date stays"));
    j.remove_item(&birth).unwrap();
    assert_eq!(j.individual_view(&walter).unwrap().born.unwrap().edtf, "1928");

    // A family: members stay.
    j.remove_item(&fam).unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM individual"), 2);
    assert_eq!(count(&j, "SELECT count(*) FROM family_member"), 0);
    // Stories and memories have their own rules from sprint 4 (tests/memories.rs).
    j.remove_item(&m).unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM memory"), 0);
}
