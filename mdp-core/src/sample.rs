//! The sample family: a small Journey built the same way every
//! time, so screens and checks can be tried before real content exists.
//! It is the made-up Rowan family, with a few CC0 pictures and a
//! public-domain recording (credits in `sample-media/CREDITS.txt`) brought in
//! through the normal import.

use std::path::Path;

use serde_json::{json, Value};

use crate::{Journey, Result, Row};

fn row(v: Value) -> Row {
    v.as_object().cloned().unwrap_or_default()
}

pub const NAME: &str = "The Rowan Family (sample)";

pub fn create(folder: &Path) -> Result<Journey> {
    let mut j = Journey::create(folder, NAME)?;
    let jid = j.id().to_string();
    j.act(|a| {
        a.update("journey", crate::id_key(&jid), "introduction",
            json!("Walter and June Rowan, their daughter Ellen, and the people and pets around their kitchen table. This is a sample Journey for trying the app."))?;
        let person = |a: &mut crate::Action, name: &str, given: &str, birth: Option<&str>, death: Option<&str>, desc: &str| -> Result<String> {
            let id = a.create("individual", row(json!({
                "kind": "Person", "name": name, "birth_date_edtf": birth, "birth_date": birth.map(|b| b.trim_end_matches('~').to_string()),
                "death_date_edtf": death, "death_date": death, "description": desc})))?;
            a.insert("person", row(json!({"id": id, "given_name": given, "family_name": "Rowan"})))?;
            Ok(id)
        };
        let walter = person(a, "Walter Rowan", "Walter", Some("1928-03-09"), Some("1999-11-02"), "Took the family photographs for forty years.")?;
        let june = person(a, "June Rowan", "June", Some("1931-04-12"), Some("2014-01-20"), "Kept a green scrapbook of people she called modern pioneers.")?;
        let ellen = person(a, "Ellen Rowan", "Ellen", Some("1958"), None, "Gathered this history.")?;
        let pepper = a.create("individual", row(json!({"kind": "Pet", "name": "Pepper", "description": "The cat under the kitchen table."})))?;
        a.insert("pet", row(json!({"id": pepper, "species": "cat", "joined_date_edtf": "1964"})))?;

        let fam = a.create("family", row(json!({"name": "The Rowans", "description": "Walter, June, Ellen and Pepper."})))?;
        for m in [&walter, &june, &ellen, &pepper] {
            a.insert("family_member", row(json!({"family_id": fam, "individual_id": m})))?;
        }
        for (from, to, kind, q) in [(&walter, &june, "partner", None), (&walter, &ellen, "parent", Some("birth")), (&june, &ellen, "parent", Some("birth"))] {
            a.create("individual_relationship", row(json!({"from_id": from, "to_id": to, "kind": kind, "qualifier": q, "status": "entered"})))?;
        }

        let ogden = a.create("place", row(json!({"name": "Ogden, Utah", "latitude": 41.223, "longitude": -111.974, "description": "Where June and Ellen were born."})))?;
        let boise = a.create("place", row(json!({"name": "Boise, Idaho", "latitude": 43.615, "longitude": -116.202, "description": "Walter's home town."})))?;
        let house = a.create("place", row(json!({"name": "The Rowan house", "exact": 1, "description": "The family home from 1957."})))?;

        let wedding = a.create("event", row(json!({"kind": "wedding", "name": "Wedding of Walter and June Rowan", "start_date": "1956-06-02", "start_date_edtf": "1956-06-02", "place_id": ogden})))?;
        for (i, p) in [&walter, &june].iter().enumerate() {
            a.insert("event_individual", row(json!({"event_id": wedding, "individual_id": p, "position": i})))?;
        }
        let birth = a.create("event", row(json!({"kind": "birth", "name": "Birth of Ellen Rowan", "start_date": "1958", "start_date_edtf": "1958", "place_id": ogden})))?;
        a.insert("event_individual", row(json!({"event_id": birth, "individual_id": ellen, "position": 0})))?;

        let wed_mem = a.create("memory", row(json!({"name": "Walter and June's wedding day", "date": "1956-06-02", "date_edtf": "1956-06-02", "status": "needs content", "place_id": ogden, "event_id": wedding})))?;
        let dinners = a.create("memory", row(json!({"name": "Sunday dinners at the kitchen table", "date_edtf": "196X", "caption": "Everyone talked at once.", "status": "needs content", "place_id": house})))?;
        let camera = a.create("memory", row(json!({"name": "Walter's camera on the lawn", "date": "1962", "date_edtf": "1962", "caption": "Saturdays belonged to Walter's camera.", "status": "needs content", "place_id": house})))?;
        let boise_trip = a.create("memory", row(json!({"name": "Summers in Boise", "date_edtf": "1960/1965", "status": "needs content", "place_id": boise})))?;
        for (m, p, role) in [(&wed_mem, &walter, "in the picture"), (&wed_mem, &june, "in the picture"), (&dinners, &fam, "in the picture"), (&dinners, &pepper, "in the picture"), (&camera, &walter, "taken by"), (&boise_trip, &ellen, "in the picture")] {
            a.insert("memory_individual", row(json!({"memory_id": m, "individual_id": p, "role": role, "certainty": "certain"})))?;
        }

        let table = a.create("story", row(json!({"name": "The Kitchen Table", "introduction": "Three generations around one table.", "focus_id": fam})))?;
        for (i, m) in [&wed_mem, &dinners, &boise_trip].iter().enumerate() {
            a.insert("story_memory", row(json!({"story_id": table, "memory_id": m, "position": i})))?;
        }
        let cam_story = a.create("story", row(json!({"name": "Walter's Camera", "introduction": "Forty years behind the lens.", "focus_id": walter})))?;
        a.insert("story_memory", row(json!({"story_id": cam_story, "memory_id": camera, "position": 0})))?;
        a.insert("featured", row(json!({"item_id": table, "position": 0})))?;
        a.insert("featured", row(json!({"item_id": cam_story, "position": 1})))?;
        Ok((camera, dinners))
    })
    .and_then(|(camera_memory, dinners)| add_media(&mut j, &camera_memory, &dinners))?;
    j.undo_stack.clear();
    Ok(j)
}

const MEDIA: &[(&str, &[u8], &str)] = &[
    ("camera-on-tripod.jpg", include_bytes!("../sample-media/camera-on-tripod.jpg"), "Sample picture: camera on a tripod (photo by Lav Varshney, CC0)"),
    ("tabby-cat.jpg", include_bytes!("../sample-media/tabby-cat.jpg"), "Sample picture: a tabby cat (photo by Stefan van der Walt, CC0)"),
    ("cup-of-coffee.jpg", include_bytes!("../sample-media/cup-of-coffee.jpg"), "Sample picture: a cup of coffee (photo by Rachel Michetti, CC0)"),
    ("sample-recording.mp3", include_bytes!("../sample-media/kennedy-inaugural-1961-excerpt.mp3"), "Sample recording: inaugural address, 1961 (public domain)"),
];

/// Bring the sample pictures and recording in through the normal import.
/// The camera picture goes into Walter's camera memory and the cat into the
/// Sunday dinners (sprint 6, so the sample's export shows two memories), the
/// coffee is set aside, and the recording waits.
fn add_media(j: &mut Journey, camera_memory: &str, dinners: &str) -> Result<()> {
    let tmp = std::env::temp_dir().join(format!("mdp-sample-{}", crate::ids::new_id()));
    std::fs::create_dir_all(&tmp)?;
    let mut paths = Vec::new();
    for (name, bytes, _) in MEDIA {
        let p = tmp.join(name);
        std::fs::write(&p, bytes)?;
        paths.push(p);
    }
    let result = j.import(&paths, &crate::jobs::JobContext::new());
    let _ = std::fs::remove_dir_all(&tmp);
    let (report, _) = result?;
    let ids = report.imported;
    if ids.len() != MEDIA.len() {
        return Ok(());
    }
    j.act(|a| {
        for (id, (_, _, note)) in ids.iter().zip(MEDIA) {
            a.update("content", crate::id_key(id), "source_note", Value::from(*note))?;
        }
        a.update("content", crate::id_key(&ids[0]), "date", "1962".into())?;
        a.update("content", crate::id_key(&ids[0]), "state", "in a memory".into())?;
        a.insert("memory_content", row(json!({"memory_id": camera_memory, "content_id": ids[0], "position": 0})))?;
        a.update("memory", crate::id_key(camera_memory), "status", "complete".into())?;
        a.update("content", crate::id_key(&ids[1]), "state", "in a memory".into())?;
        a.insert("memory_content", row(json!({"memory_id": dinners, "content_id": ids[1], "position": 0})))?;
        a.update("memory", crate::id_key(dinners), "status", "complete".into())?;
        a.update("content", crate::id_key(&ids[2]), "state", "set aside".into())?;
        Ok(())
    })
}
