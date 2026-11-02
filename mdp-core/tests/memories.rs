//! Sprint 4: memories, roles, stories, the Journey and the Waiting actions
//!.

use std::path::{Path, PathBuf};

use mdp_core::jobs::JobContext;
use mdp_core::memories::roles_for;
use mdp_core::{folder, Journey, Row};
use serde_json::json;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn changes(v: serde_json::Value) -> Row {
    v.as_object().cloned().unwrap()
}

fn count(j: &Journey, sql: &str) -> i64 {
    j.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

fn state(j: &Journey, id: &str) -> String {
    j.conn().query_row("SELECT state FROM content WHERE id = ?1", [id], |r| r.get(0)).unwrap()
}

/// A Journey with three waiting items: a picture, a recording and a text.
fn journey_with_items(dir: &Path) -> (Journey, Vec<String>) {
    let mut j = Journey::create(&dir.join("J"), "Rowans").unwrap();
    let paths = [fixtures().join("walter-camera.jpg"), fixtures().join("hello.wav"), fixtures().join("scrapbook-note.txt")];
    let (r, _) = j.import(&paths, &JobContext::new()).unwrap();
    assert_eq!(r.imported.len(), 3);
    (j, r.imported)
}

#[test]
fn content_in_a_memory_is_ordered_with_a_cover_and_status() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, items) = journey_with_items(dir.path());
    let m = j.add_memory("Walter's camera", None).unwrap();
    assert_eq!(j.memory_view(&m).unwrap().status, "needs content");

    j.add_to_memory(&m, &items[..2]).unwrap();
    let v = j.memory_view(&m).unwrap();
    assert_eq!(v.status, "complete");
    assert_eq!(v.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>(), items[..2].to_vec());
    assert!(v.items[0].cover && !v.cover_chosen, "the first picture is the cover until one is chosen");
    assert!(items[..2].iter().all(|i| state(&j, i) == "in a memory"));

    // A recording is never the cover (sprint 5 feedback), even when first.
    j.move_in_memory(&m, &items[1], -1).unwrap();
    let v = j.memory_view(&m).unwrap();
    assert_eq!(v.items[0].id, items[1]);
    assert!(!v.items[0].cover && v.items[1].cover);
    assert!(j.update_memory(&m, &changes(json!({"cover_id": items[1]}))).is_err());

    // A chosen cover stays put when the order changes.
    j.update_memory(&m, &changes(json!({"cover_id": items[0]}))).unwrap();
    j.move_in_memory(&m, &items[0], 1).unwrap();
    let v = j.memory_view(&m).unwrap();
    assert!(v.cover_chosen && v.items.iter().find(|i| i.cover).unwrap().id == items[0]);

    // Something outside the memory can't be its cover.
    assert!(j.update_memory(&m, &changes(json!({"cover_id": items[2]}))).is_err());

    // Taking the cover out clears the choice and sends it back to waiting.
    j.remove_from_memory(&m, &items[0]).unwrap();
    let v = j.memory_view(&m).unwrap();
    assert!(!v.cover_chosen && v.items.len() == 1);
    assert_eq!(state(&j, &items[0]), "waiting");

    // Taking the last item out makes it a placeholder again.
    j.remove_from_memory(&m, &items[1]).unwrap();
    assert_eq!(j.memory_view(&m).unwrap().status, "needs content");

    // Undo puts it back.
    j.undo().unwrap();
    assert_eq!(j.memory_view(&m).unwrap().status, "complete");
    assert_eq!(state(&j, &items[1]), "in a memory");
}

#[test]
fn content_in_two_memories_stays_until_the_last_lets_go() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, items) = journey_with_items(dir.path());
    let a = j.new_memory_with("Saturday mornings", &items[..1], None).unwrap();
    let b = j.add_memory("The camera", None).unwrap();
    j.add_to_memory(&b, &items[..1]).unwrap();
    j.remove_from_memory(&a, &items[0]).unwrap();
    assert_eq!(state(&j, &items[0]), "in a memory", "still held by the other memory");
    j.remove_from_memory(&b, &items[0]).unwrap();
    assert_eq!(state(&j, &items[0]), "waiting");
}

#[test]
fn memory_details_take_dates_as_known_and_check_links() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let m = j.add_memory("Summers in Boise", None).unwrap();
    let p = j.add_place("Boise, Idaho").unwrap();
    j.update_memory(&m, &changes(json!({"date": "1960 to 1965", "caption": "Hot days.", "place_id": p}))).unwrap();
    let v = j.memory_view(&m).unwrap();
    assert_eq!(v.date.unwrap().edtf, "1960/1965");
    assert_eq!(v.place.unwrap().name, "Boise, Idaho");
    assert!(!v.long_caption);
    assert!(j.update_memory(&m, &changes(json!({"date": "sometime"}))).is_err());
    assert!(j.update_memory(&m, &changes(json!({"name": " "}))).is_err());
    assert!(j.update_memory(&m, &changes(json!({"event_id": "nope"}))).is_err());
    j.update_memory(&m, &changes(json!({"caption": "word ".repeat(80)}))).unwrap();
    assert!(j.memory_view(&m).unwrap().long_caption);
}

#[test]
fn a_typed_note_is_a_text_file_in_the_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let m = j.add_memory("Sunday dinners", None).unwrap();
    j.update_memory(&m, &changes(json!({"caption": "Everyone talked at once."}))).unwrap();
    let note = j.add_note(&m, "", "Everyone talked at once.\nJune kept score.", true).unwrap();
    let v = j.memory_view(&m).unwrap();
    assert_eq!(v.status, "complete");
    assert!(v.caption.is_none(), "the caption became the note");
    assert_eq!(v.items[0].kind, "text");
    assert!(v.items[0].name.starts_with("Note: Everyone talked"));
    let rel: String = j.conn().query_row("SELECT path FROM content_file WHERE content_id = ?1", [&note], |r| r.get(0)).unwrap();
    let abs = folder::absolute(j.root(), &rel);
    assert_eq!(std::fs::read_to_string(&abs).unwrap(), "Everyone talked at once.\r\nJune kept score.");

    // Undo removes the note and its file, and gives the caption back.
    j.undo().unwrap();
    assert!(!abs.is_file());
    let v = j.memory_view(&m).unwrap();
    assert!(v.items.is_empty() && v.caption.as_deref() == Some("Everyone talked at once."));
    assert!(j.add_note(&m, "x", "  ", false).is_err());
}

#[test]
fn roles_follow_the_kinds_of_content() {
    assert!(roles_for(&[]).contains(&"in the picture".into()) && roles_for(&[]).contains(&"recorded by".into()));
    let pic = roles_for(&["picture".into()]);
    assert_eq!(pic.last().unwrap(), "other");
    assert!(pic.contains(&"taken by".into()) && !pic.contains(&"speaking".into()));
    let mixed = roles_for(&["voice recording".into(), "video".into()]);
    assert_eq!(mixed.iter().filter(|r| *r == "speaking").count(), 1, "the combined list has no repeats");

    let dir = tempfile::tempdir().unwrap();
    let (mut j, items) = journey_with_items(dir.path());
    let walter = j.add_individual("person", "Walter Rowan").unwrap();
    let fam = j.add_family("The Rowans").unwrap();
    let m = j.new_memory_with("Walter's camera", &items[..1], None).unwrap();
    assert!(j.memory_view(&m).unwrap().roles.contains(&"taken by".to_string()));

    j.link_to_memory(&m, &walter, "taken by", None, false).unwrap();
    j.link_to_memory(&m, &fam, "in the picture", None, true).unwrap();
    assert!(j.link_to_memory(&m, &walter, "taken by", None, false).is_err(), "no duplicate link");
    assert!(j.link_to_memory(&m, &walter, "other", None, false).is_err(), "other needs a note");
    assert!(j.link_to_memory(&m, &walter, "juggling", None, false).is_err(), "not a role");
    let place = j.add_place("Boise").unwrap();
    assert!(j.link_to_memory(&m, &place, "in the picture", None, false).is_err(), "places don't take roles");

    j.update_memory_link(&m, &walter, "taken by", "other", Some("set up the camera"), false).unwrap();
    let v = j.memory_view(&m).unwrap();
    let w = v.people.iter().find(|p| p.id == walter).unwrap();
    assert_eq!((w.role.as_str(), w.role_note.as_deref()), ("other", Some("set up the camera")));
    assert!(v.people.iter().find(|p| p.id == fam).unwrap().probably);
    assert_eq!(v.people.iter().find(|p| p.id == fam).unwrap().kind, "family");

    j.unlink_from_memory(&m, &walter, "other").unwrap();
    assert_eq!(j.memory_view(&m).unwrap().people.len(), 1);
}

#[test]
fn assigning_to_a_person_makes_a_memory_not_a_direct_link() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, items) = journey_with_items(dir.path());
    let june = j.add_individual("person", "June Rowan").unwrap();
    let m = j.assign_to_individual(&june, &items[1..2]).unwrap();
    let v = j.memory_view(&m).unwrap();
    assert_eq!(v.name, "June Rowan: hello");
    assert_eq!(v.items.len(), 1);
    assert_eq!(v.people[0].id, june);
    assert_eq!(v.people[0].role, "speaking");
    assert_eq!(state(&j, &items[1]), "in a memory");
    // the only link from content into the Journey is memory_content.
    assert_eq!(count(&j, "SELECT count(*) FROM memory_individual WHERE individual_id IN (SELECT id FROM content)"), 0);

    let m2 = j.assign_to_individual(&june, &[items[0].clone(), items[2].clone()]).unwrap();
    let v = j.memory_view(&m2).unwrap();
    assert_eq!(v.name, "June Rowan: 2 items");
    assert_eq!(v.people[0].role, "in the picture");
    assert!(j.assign_to_individual(&june, &[]).is_err());

    // One action: undo takes the memory away and the items go back to waiting.
    j.undo().unwrap();
    assert_eq!(state(&j, &items[0]), "waiting");
    assert_eq!(count(&j, "SELECT count(*) FROM memory"), 1);
}

#[test]
fn stories_order_memories_and_have_a_focus() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, items) = journey_with_items(dir.path());
    let walter = j.add_individual("person", "Walter Rowan").unwrap();
    let s = j.add_story("Walter's Camera").unwrap();
    j.update_story(&s, &changes(json!({"introduction": "Forty years behind the lens.", "focus_id": walter}))).unwrap();
    let a = j.add_memory("First camera", Some(&s)).unwrap();
    let b = j.new_memory_with("The tripod", &items[..1], None).unwrap();
    j.add_memories_to_story(&s, &[b.clone()]).unwrap();
    let v = j.story_view(&s).unwrap();
    assert_eq!(v.focus.as_ref().unwrap().note, "person");
    assert_eq!(v.memories.iter().map(|m| m.id.clone()).collect::<Vec<_>>(), vec![a.clone(), b.clone()]);
    assert_eq!(v.cover_id.as_deref(), Some(items[0].as_str()), "cover: first picture in the first memory that has one");

    j.move_in_story(&s, &b, -1).unwrap();
    assert_eq!(j.story_view(&s).unwrap().memories[0].id, b);
    j.undo().unwrap();
    assert_eq!(j.story_view(&s).unwrap().memories[0].id, a, "ordering survives undo");

    // A memory can sit in more than one story.
    let s2 = j.add_story("Saturdays").unwrap();
    j.add_memories_to_story(&s2, &[b.clone()]).unwrap();
    assert_eq!(j.memory_view(&b).unwrap().stories.len(), 2);

    let place = j.add_place("Boise").unwrap();
    assert!(j.update_story(&s, &changes(json!({"focus_id": place}))).is_err(), "a place can't be the focus");
    assert!(j.update_story(&s, &changes(json!({"name": ""}))).is_err());

    j.remove_memory_from_story(&s, &a).unwrap();
    assert_eq!(j.story_view(&s).unwrap().memories.len(), 1);
    assert!(j.memory_view(&a).is_ok(), "the memory stays in the Journey");
    assert!(j.memories().unwrap().iter().any(|m| m.id == a && m.note.starts_with("Not in a story yet")));
}

#[test]
fn the_journey_title_introduction_and_featured_items() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    j.update_journey(&changes(json!({"name": "The Rowan Family", "introduction": "Around one table."}))).unwrap();
    let h = j.home().unwrap();
    assert_eq!((h.name.as_str(), h.introduction.as_str()), ("The Rowan Family", "Around one table."));
    assert!(j.update_journey(&changes(json!({"name": ""}))).is_err());

    let mut ids = vec![];
    for i in 0..6 {
        let s = j.add_story(&format!("Story {i}")).unwrap();
        j.set_featured(&s, true).unwrap();
        ids.push(s);
    }
    let extra = j.add_memory("One more", None).unwrap();
    assert!(j.set_featured(&extra, true).is_err(), "up to six");
    j.set_featured(&ids[0], false).unwrap();
    j.set_featured(&extra, true).unwrap();
    j.move_featured(&extra, -10).unwrap();
    let f: Vec<String> = j.home().unwrap().featured.into_iter().map(|e| e.id).collect();
    assert_eq!(f[0], extra);
    assert_eq!(f.len(), 6);
    let walter = j.add_individual("person", "Walter").unwrap();
    assert!(j.set_featured(&walter, true).is_err(), "only stories and memories");
}

#[test]
fn removing_a_story_or_memory_unlinks_only() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, items) = journey_with_items(dir.path());
    let walter = j.add_individual("person", "Walter Rowan").unwrap();
    let s = j.add_story("Walter's Camera").unwrap();
    j.update_story(&s, &changes(json!({"focus_id": walter}))).unwrap();
    let m = j.new_memory_with("The tripod", &items[..2], None).unwrap();
    let other = j.new_memory_with("Also the tripod", &items[1..2], None).unwrap();
    j.add_memories_to_story(&s, &[m.clone(), other.clone()]).unwrap();
    j.link_to_memory(&m, &walter, "taken by", None, false).unwrap();
    j.set_featured(&m, true).unwrap();
    j.set_featured(&s, true).unwrap();

    let w = j.removal_warning(&m).unwrap();
    assert!(w.iter().any(|l| l.starts_with("1 item goes back to waiting")), "{w:?}");
    assert!(w.iter().any(|l| l.contains("also sits in another memory")), "{w:?}");
    assert!(w.iter().any(|l| l.contains("story Walter's Camera")), "{w:?}");
    assert!(w.iter().any(|l| l.contains("featured")), "{w:?}");

    j.remove_item(&m).unwrap();
    assert_eq!(state(&j, &items[0]), "waiting");
    assert_eq!(state(&j, &items[1]), "in a memory");
    assert_eq!(count(&j, "SELECT count(*) FROM memory_individual"), 0);
    assert_eq!(j.story_view(&s).unwrap().memories.len(), 1);
    assert_eq!(count(&j, "SELECT min(position) FROM story_memory"), 0, "the story's order closes the gap");
    assert_eq!(count(&j, "SELECT position FROM featured"), 0);
    assert!(j.individual_view(&walter).is_ok());

    j.undo().unwrap();
    assert_eq!(state(&j, &items[0]), "in a memory");
    assert_eq!(j.memory_view(&m).unwrap().people.len(), 1);
    assert_eq!(j.story_view(&s).unwrap().memories.len(), 2);

    let w = j.removal_warning(&s).unwrap();
    assert!(w.iter().any(|l| l.starts_with("Its 2 memories")), "{w:?}");
    assert!(w.iter().any(|l| l.contains("Walter Rowan")), "{w:?}");
    j.remove_item(&s).unwrap();
    assert!(j.memory_view(&m).is_ok() && j.memory_view(&other).is_ok(), "memories stay");
    assert_eq!(count(&j, "SELECT count(*) FROM featured"), 1);

    // Removing a person (sprint 3) still drops their roles and focus, and keeps the memories.
    j.undo().unwrap();
    j.remove_item(&walter).unwrap();
    assert!(j.memory_view(&m).unwrap().people.is_empty());
    assert!(j.story_view(&s).unwrap().focus.is_none());
}

#[test]
fn the_large_preview_uses_the_journeys_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (j, items) = journey_with_items(dir.path());
    let (path, format) = j.content_file_to_show(&items[0]).unwrap().unwrap();
    assert!(path.starts_with(j.root()) && path.is_file());
    assert_eq!(format, "image/jpeg");
}

#[test]
fn the_sample_family_has_finished_memories() {
    let dir = tempfile::tempdir().unwrap();
    let j = mdp_core::sample::create(&dir.path().join("S")).unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM memory WHERE status = 'complete'"), 2);
    assert_eq!(count(&j, "SELECT count(*) FROM memory WHERE status = 'complete' AND NOT EXISTS (SELECT 1 FROM memory_content WHERE memory_id = memory.id)"), 0);
}

#[test]
fn a_memory_made_from_waiting_can_go_straight_into_a_story() {
    // Sprint 5 feedback round 2: a picture put into a new memory from Waiting
    // showed in no story, so the Waiting dialog can now name one.
    let dir = tempfile::tempdir().unwrap();
    let (mut j, items) = journey_with_items(dir.path());
    let s = j.add_story("Saturdays").unwrap();
    let m = j.new_memory_with("Walter's camera", &items[..1], Some(&s)).unwrap();
    let v = j.story_view(&s).unwrap();
    assert_eq!(v.memories.len(), 1);
    assert_eq!(v.memories[0].id, m);
    assert_eq!(v.memories[0].cover_id.as_deref(), Some(items[0].as_str()), "the story screen shows the memory's picture");

    // One undo takes the whole step back.
    j.undo().unwrap();
    assert!(j.story_view(&s).unwrap().memories.is_empty());
    assert_eq!(state(&j, &items[0]), "waiting");

    assert!(j.new_memory_with("Gone", &items[..1], Some("no-such-story")).is_err());
}

#[test]
fn a_person_page_lists_their_stories() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, items) = journey_with_items(dir.path());
    let walter = j.add_individual("person", "Walter Rowan").unwrap();
    let june = j.add_individual("person", "June Rowan").unwrap();
    let about = j.add_story("Walter's war").unwrap();
    j.update_story(&about, &changes(json!({"focus_id": walter}))).unwrap();
    let saturdays = j.add_story("Saturdays").unwrap();
    let m = j.assign_to_individual(&walter, &items[..1]).unwrap();
    j.add_memories_to_story(&saturdays, &[m]).unwrap();
    j.add_story("Nothing to do with him").unwrap();

    let v = j.individual_view(&walter).unwrap();
    let names: Vec<_> = v.stories.iter().map(|s| (s.name.as_str(), s.note.as_str())).collect();
    assert_eq!(names, [("Saturdays", ""), ("Walter's war", "About them")]);
    assert!(j.individual_view(&june).unwrap().stories.is_empty());
}

/// Sprint 7: a memory with content says what it could
/// still use; Home gathers placeholders, then those, and shows the shelf.
#[test]
fn could_add_lists_what_a_memory_still_lacks_and_home_gathers_them() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, items) = journey_with_items(dir.path());
    let placeholder = j.add_memory("Grandpa's first car", None).unwrap();
    assert!(j.memory_view(&placeholder).unwrap().could_add.is_empty(), "a placeholder needs content first");

    let m = j.new_memory_with("A picture from the box", &[items[0].clone()], None).unwrap();
    assert_eq!(j.memory_view(&m).unwrap().could_add, ["a date", "a place", "people", "a caption", "a story"]);
    let h = j.home().unwrap();
    assert_eq!(h.to_finish.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), [placeholder.as_str()]);
    assert_eq!(h.could_add.len(), 1);
    assert_eq!(h.could_add[0].note, "a date, a place, people, a caption, a story");
    assert!(h.shelf.is_empty(), "no story yet");

    let walter = j.add_individual("Person", "Walter Rowan").unwrap();
    let house = j.add_place("The Rowan house").unwrap();
    let story = j.add_story("Walter's camera").unwrap();
    j.update_memory(&m, &changes(json!({"date": "1962", "caption": "On the lawn.", "place_id": house}))).unwrap();
    j.link_to_memory(&m, &walter, "taken by", None, false).unwrap();
    j.add_memories_to_story(&story, &[m.clone()]).unwrap();
    assert!(j.memory_view(&m).unwrap().could_add.is_empty());
    let h = j.home().unwrap();
    assert!(h.could_add.is_empty() && h.could_add_total == 0);
    // The shelf shows the story with its cover picture.
    assert_eq!(h.shelf.len(), 1);
    assert_eq!((h.shelf[0].name.as_str(), h.shelf[0].note.as_str()), ("Walter's camera", items[0].as_str()));
}

/// Sprint 9: a memory marked finished stops asking for more and
/// leaves Memories to finish; undo and "not finished" bring it back. A
/// placeholder can't be finished.
#[test]
fn a_finished_memory_stops_asking_for_more() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, items) = journey_with_items(dir.path());
    let placeholder = j.add_memory("Grandpa's first car", None).unwrap();
    assert!(j.update_memory(&placeholder, &changes(json!({"finished": true}))).is_err());

    let m = j.new_memory_with("A picture from the box", &[items[0].clone()], None).unwrap();
    j.update_memory(&m, &changes(json!({"finished": true}))).unwrap();
    let v = j.memory_view(&m).unwrap();
    assert!(v.finished && v.could_add.is_empty());
    assert!(j.home().unwrap().could_add.is_empty());
    // Adding to it later doesn't unmark it.
    j.update_memory(&m, &changes(json!({"caption": "On the lawn."}))).unwrap();
    assert!(j.memory_view(&m).unwrap().finished);

    j.undo().unwrap();
    j.undo().unwrap();
    assert!(!j.memory_view(&m).unwrap().finished);
    assert_eq!(j.home().unwrap().could_add.len(), 1);
    j.update_memory(&m, &changes(json!({"finished": true}))).unwrap();
    j.update_memory(&m, &changes(json!({"finished": false}))).unwrap();
    assert!(!j.memory_view(&m).unwrap().finished);
}
