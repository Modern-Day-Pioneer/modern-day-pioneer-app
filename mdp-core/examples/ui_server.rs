//! Serves the app's screens (`app/mdp-app/ui`) to a plain browser with the
//! real core behind them, for checking screens and accessibility without
//! Windows: `cargo run --example ui_server -- <port>`. The browser side
//! replaces Tauri's `invoke` with `POST /invoke/<command>` (see
//! `app/mdp-app/ui-check/`). Only the commands the shell and the screens of
//! sprints 3 to 6 need are answered; this is a test tool, not part of the app.
//! Sprint 5's microphone and speech model are stood in for: a recording is a
//! tone as long as the time between Record and Stop, and a transcript is made
//! at once from fixed lines.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

use mdp_core::{Journey, Row};
use serde_json::{json, Value};

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

fn o(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(str::to_string)
}

fn row(v: &Value, k: &str) -> Row {
    v.get(k).and_then(|x| x.as_object()).cloned().unwrap_or_default()
}

fn n(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(|x| x.as_i64()).unwrap_or(0)
}

fn b(v: &Value, k: &str) -> bool {
    v.get(k).and_then(|x| x.as_bool()).unwrap_or(false)
}

/// Undo `%XX` escapes in a URL path.
fn unescape(p: &str) -> String {
    let bytes = p.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(x) = u8::from_str_radix(&p[i + 1..i + 3], 16) {
                out.push(x);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn list(v: &Value, k: &str) -> Vec<String> {
    v.get(k).and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

#[derive(Default)]
struct Fake {
    recording: Option<(std::time::Instant, Option<String>)>,
    finished: Vec<String>,
    auto: bool,
    /// "Make a transcript" runs for 3 seconds, so the check sees its progress.
    job: Option<(String, std::time::Instant)>,
    /// The first one asked for fails, so the check sees the error and Try again.
    tried: bool,
    last_error: Option<(String, String)>,
}

static FAKE: std::sync::Mutex<Option<Fake>> = std::sync::Mutex::new(None);

/// The last export (sprint 6): its report or error, and what it left out.
type ExportResult = (Result<mdp_core::export::Report, String>, Vec<Value>);
static EXPORT: std::sync::Mutex<Option<ExportResult>> = std::sync::Mutex::new(None);

/// The last backup or restore (sprint 7): its kind and the file or folder made.
static BACKUP: std::sync::Mutex<Option<(&'static str, Result<String, String>)>> = std::sync::Mutex::new(None);
static LAST_BACKUP: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// The last folder looked through and the last import (sprint 11), as the app's status calls give them.
static SCAN: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);
static IMPORTED: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);
/// The suggestions switch (Settings), kept for the run.
static SUGGEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Where the stand-in exports go: beside the sample Journey.
fn export_dir(j: &Journey) -> PathBuf {
    j.root().with_file_name(format!("{} export", j.root().file_name().unwrap_or_default().to_string_lossy()))
}

fn stand_in(j: &mut Journey, id: &str) -> Result<(), String> {
    use mdp_core::voice::Line;
    let lines = vec![
        Line { start: 0.0, end: 3.0, text: "This is a stand-in transcript made by the screen check.".into() },
        Line { start: 3.2, end: 6.0, text: "The real one comes from whisper.cpp on the PC.".into() },
        Line { start: 9.0, end: 12.0, text: "A pause of two seconds starts a new paragraph.".into() },
    ];
    j.save_transcript(id, &lines, "stand-in (screen check)").map_err(|e| e.to_string())?;
    FAKE.lock().unwrap().get_or_insert_with(|| Fake { auto: true, ..Fake::default() }).finished.push(id.to_string());
    Ok(())
}

fn voice(j: &mut Journey, cmd: &str, a: &Value) -> Result<Value, String> {
    let mut g = FAKE.lock().unwrap();
    let f = g.get_or_insert_with(|| Fake { auto: true, ..Fake::default() });
    if let Some((id, t)) = f.job.clone().filter(|(_, t)| t.elapsed().as_secs_f64() >= 3.0) {
        let _ = t;
        f.job = None;
        if !f.tried {
            f.tried = true;
            f.last_error = Some((id, "Stand-in recording: No speech was heard in this recording.".into()));
        } else {
            f.last_error = None;
            drop(g);
            stand_in(j, &id)?;
            g = FAKE.lock().unwrap();
        }
    }
    let f = g.as_mut().unwrap();
    Ok(match cmd {
        "voice_status" => json!({
            "recording": f.recording.as_ref().map(|(t, m)| json!({"seconds": t.elapsed().as_secs_f64(), "level": 0.3, "mic": "Stand-in microphone", "memory": m})),
            "model": "small.en",
            "models": [{"name": "medium.en", "size": "about 1.5 GB", "present": false, "suggested": true},
                       {"name": "small.en", "size": "about 470 MB", "present": true, "suggested": false}],
            "folder": "models", "downloading": null, "auto": f.auto, "offer_model": false, "suggested": "medium.en",
            "transcribing": f.job.as_ref().map(|(id, t)| json!({"id": id, "name": "Stand-in recording",
                "percent": (t.elapsed().as_secs_f64() / 3.0 * 100.0) as i32, "stage": "Listening", "elapsed": t.elapsed().as_secs()})),
            "queued": [], "last_error": f.last_error.as_ref().map(|e| &e.1), "error_id": f.last_error.as_ref().map(|e| &e.0),
            "finished": std::mem::take(&mut f.finished)}),
        "record_start" => {
            f.recording = Some((std::time::Instant::now(), o(a, "memory").filter(|m| !m.is_empty())));
            json!({"mic": "Stand-in microphone"})
        }
        "record_stop" => {
            let (t, memory) = f.recording.take().ok_or("Nothing is being recorded.")?;
            let auto = f.auto;
            drop(g);
            let wav = std::env::temp_dir().join(format!("mdp-ui-rec-{}.wav", mdp_core::ids::new_id()));
            let mut w = mdp_core::voice::WavFile::create(&wav, 16_000).map_err(|e| e.to_string())?;
            let n = (t.elapsed().as_secs_f64().max(1.0) * 16_000.0) as usize;
            w.write(&(0..n).map(|i| (i as f32 * 0.05).sin() * 0.2).collect::<Vec<_>>()).map_err(|e| e.to_string())?;
            w.finish().map_err(|e| e.to_string())?;
            let id = j.add_recording(&wav, memory.as_deref(), None).map_err(|e| e.to_string())?;
            if auto {
                stand_in(j, &id)?;
            }
            json!(id)
        }
        "transcribe" => {
            f.job = Some((s(a, "id"), std::time::Instant::now()));
            Value::Null
        }
        "set_auto_transcribe" => {
            f.auto = b(a, "on");
            Value::Null
        }
        "cancel_transcribe" => {
            if f.last_error.as_ref().is_some_and(|e| e.0 == s(a, "id")) {
                f.last_error = None;
            }
            Value::Null
        }
        "choose_model" | "cancel_download" | "clear_download" | "model_offer_answered" => Value::Null,
        _ => return Err(format!("ui_server stands in for {cmd} only in the app")),
    })
}

fn call(j: &mut Journey, cmd: &str, a: &Value) -> Result<Value, String> {
    let e = |e: mdp_core::Error| e.to_string();
    let to = |v: Result<Value, serde_json::Error>| v.map_err(|e| e.to_string());
    Ok(match cmd {
        "startup" => json!({"journey": j.home().map_err(e)?, "problem": null}),
        "home" => to(serde_json::to_value(j.home().map_err(e)?))?,
        "stories" => to(serde_json::to_value(j.stories().map_err(e)?))?,
        "people" => to(serde_json::to_value(j.people_by_family().map_err(e)?))?,
        "browse" => to(serde_json::to_value(j.browse(&list(a, "kinds"), 300).map_err(e)?))?,
        "search" => to(serde_json::to_value(j.search(&s(a, "text"), 20).map_err(e)?))?,
        "item" => to(serde_json::to_value(j.item(&s(a, "id")).map_err(e)?))?,
        "keep_awake" | "media_base" => Value::Null,
        "about" => json!({"version": "0.5.0", "build": "Local build", "feedback": std::env::var("MDP_TEST_FEEDBACK").unwrap_or_default(), "releases": "https://github.com/mbwamkali/modern-day-pioneer-app/releases"}),
        "check_for_update" => Value::Null,
        "open_link" => Value::Null,
        "undo" => json!(j.undo().map_err(e)?),
        "choices" => json!({
            "relationship_kinds": mdp_core::people::RELATIONSHIP_KINDS, "parent_qualifiers": mdp_core::people::PARENT_QUALIFIERS,
            "genders": mdp_core::people::GENDERS, "event_kinds": mdp_core::places::EVENT_KINDS}),
        "read_date" => to(serde_json::to_value(mdp_core::dates::parse(&s(a, "text")).map_err(e)?))?,
        "item_table" => json!(j.item_table(&s(a, "id")).map_err(e)?),
        "add_individual" => json!(j.add_individual(&s(a, "kind"), &s(a, "name")).map_err(e)?),
        "individual_view" => to(serde_json::to_value(j.individual_view(&s(a, "id")).map_err(e)?))?,
        "update_individual" => to(serde_json::to_value(j.update_individual(&s(a, "id"), &row(a, "changes")).map_err(e)?))?,
        "add_relationship" => json!(j
            .add_relationship(&s(a, "from"), &s(a, "kind"), &s(a, "to"), o(a, "qualifier").as_deref(), o(a, "certainty").as_deref(), o(a, "sourceNote").as_deref())
            .map_err(e)?),
        "update_relationship" => to(serde_json::to_value(j.update_relationship(&s(a, "id"), &row(a, "changes")).map_err(e)?))?,
        "remove_relationship" => to(serde_json::to_value(j.remove_relationship(&s(a, "id")).map_err(e)?))?,
        "add_family" => json!(j.add_family(&s(a, "name")).map_err(e)?),
        "family_view" => to(serde_json::to_value(j.family_view(&s(a, "id")).map_err(e)?))?,
        "update_family" => to(serde_json::to_value(j.update_family(&s(a, "id"), &row(a, "changes")).map_err(e)?))?,
        "add_family_member" => to(serde_json::to_value(j.add_family_member(&s(a, "family"), &s(a, "individual")).map_err(e)?))?,
        "remove_family_member" => to(serde_json::to_value(j.remove_family_member(&s(a, "family"), &s(a, "individual")).map_err(e)?))?,
        "relate_families" => to(serde_json::to_value(j.relate_families(&s(a, "a"), &s(a, "b")).map_err(e)?))?,
        "unrelate_families" => to(serde_json::to_value(j.unrelate_families(&s(a, "a"), &s(a, "b")).map_err(e)?))?,
        "places_and_events" => to(serde_json::to_value(j.places_and_events().map_err(e)?))?,
        "add_place" => json!(j.add_place(&s(a, "name")).map_err(e)?),
        "place_view" => to(serde_json::to_value(j.place_view(&s(a, "id")).map_err(e)?))?,
        "update_place" => to(serde_json::to_value(j.update_place(&s(a, "id"), &row(a, "changes")).map_err(e)?))?,
        "merge_places" => to(serde_json::to_value(j.merge_places(&s(a, "keep"), &s(a, "other")).map_err(e)?))?,
        "add_event" => json!(j
            .add_event(&s(a, "kind"), o(a, "name").as_deref(), &list(a, "people"), &s(a, "when"), o(a, "place").as_deref())
            .map_err(e)?),
        "event_view" => to(serde_json::to_value(j.event_view(&s(a, "id")).map_err(e)?))?,
        "update_event" => to(serde_json::to_value(j.update_event(&s(a, "id"), &row(a, "changes")).map_err(e)?))?,
        "set_event_people" => to(serde_json::to_value(j.set_event_people(&s(a, "id"), &list(a, "people")).map_err(e)?))?,
        "removal_warning" => to(serde_json::to_value(j.removal_warning(&s(a, "id")).map_err(e)?))?,
        "remove_item" => to(serde_json::to_value(j.remove_item(&s(a, "id")).map_err(e)?))?,
        "memories" => to(serde_json::to_value(j.memories().map_err(e)?))?,
        "turn_picture" => json!(j.turn_picture(&s(a, "id"), n(a, "quarters")).map_err(e)?),
        "portraits" => to(serde_json::to_value(j.portraits().map_err(e)?))?,
        "portrait_choices" => to(serde_json::to_value(j.portrait_choices(&s(a, "id")).map_err(e)?))?,
        "set_portrait" => to(serde_json::to_value(j.set_portrait(&s(a, "id"), o(a, "picture").as_deref().filter(|p| !p.is_empty())).map_err(e)?))?,
        "add_family_members" => json!(j.add_family_members(&s(a, "family"), &list(a, "individuals")).map_err(e)?),
        "memory_list" => to(serde_json::to_value(j.memory_list().map_err(e)?))?,
        "memory_view" => to(serde_json::to_value(j.memory_view(&s(a, "id")).map_err(e)?))?,
        "add_memory" => json!(j.add_memory(&s(a, "name"), o(a, "story").as_deref()).map_err(e)?),
        "update_memory" => to(serde_json::to_value(j.update_memory(&s(a, "id"), &row(a, "changes")).map_err(e)?))?,
        "add_to_memory" => to(serde_json::to_value(j.add_to_memory(&s(a, "memory"), &list(a, "content")).map_err(e)?))?,
        "remove_from_memory" => to(serde_json::to_value(j.remove_from_memory(&s(a, "memory"), &s(a, "content")).map_err(e)?))?,
        "move_in_memory" => to(serde_json::to_value(j.move_in_memory(&s(a, "memory"), &s(a, "content"), n(a, "delta")).map_err(e)?))?,
        "add_note" => json!(j.add_note(&s(a, "memory"), &s(a, "title"), &s(a, "text"), b(a, "fromCaption")).map_err(e)?),
        "link_to_memory" => to(serde_json::to_value(
            j.link_to_memory(&s(a, "memory"), &s(a, "who"), &s(a, "role"), o(a, "roleNote").as_deref(), b(a, "probably")).map_err(e)?))?,
        "update_memory_link" => to(serde_json::to_value(
            j.update_memory_link(&s(a, "memory"), &s(a, "who"), &s(a, "oldRole"), &s(a, "role"), o(a, "roleNote").as_deref(), b(a, "probably")).map_err(e)?))?,
        "unlink_from_memory" => to(serde_json::to_value(j.unlink_from_memory(&s(a, "memory"), &s(a, "who"), &s(a, "role")).map_err(e)?))?,
        "new_memory_with" => json!(j.new_memory_with(&s(a, "name"), &list(a, "content"), o(a, "story").as_deref()).map_err(e)?),
        "assign_to_individual" => json!(j.assign_to_individual(&s(a, "who"), &list(a, "content")).map_err(e)?),
        "link_names_to_memory" => to(serde_json::to_value(j.link_names_to_memory(&s(a, "memory"), &s(a, "names"), &s(a, "role"), o(a, "roleNote").as_deref(), b(a, "probably")).map_err(e)?))?,
        "name_hints" => to(serde_json::to_value(j.name_hints().map_err(e)?))?,
        "assign_to_names" => json!(j.assign_to_names(&s(a, "names"), &list(a, "content")).map_err(e)?),
        "story_view" => to(serde_json::to_value(j.story_view(&s(a, "id")).map_err(e)?))?,
        "add_story" => json!(j.add_story(&s(a, "name")).map_err(e)?),
        "add_tribute" => json!(j.add_tribute(&s(a, "about"), &s(a, "name")).map_err(e)?),
        "tributes_about" => to(serde_json::to_value(j.tributes_about(&s(a, "about")).map_err(e)?))?,
        "set_memory_note" => to(serde_json::to_value(j.set_memory_note(&s(a, "story"), &s(a, "memory"), &s(a, "note")).map_err(e)?))?,
        "update_story" => to(serde_json::to_value(j.update_story(&s(a, "id"), &row(a, "changes")).map_err(e)?))?,
        "add_memories_to_story" => to(serde_json::to_value(j.add_memories_to_story(&s(a, "story"), &list(a, "memories")).map_err(e)?))?,
        "remove_memory_from_story" => to(serde_json::to_value(j.remove_memory_from_story(&s(a, "story"), &s(a, "memory")).map_err(e)?))?,
        "move_in_story" => to(serde_json::to_value(j.move_in_story(&s(a, "story"), &s(a, "memory"), n(a, "delta")).map_err(e)?))?,
        "update_journey" => to(serde_json::to_value(j.update_journey(&row(a, "changes")).map_err(e)?))?,
        "set_featured" => to(serde_json::to_value(j.set_featured(&s(a, "id"), b(a, "on")).map_err(e)?))?,
        "move_featured" => to(serde_json::to_value(j.move_featured(&s(a, "id"), n(a, "delta")).map_err(e)?))?,
        // Test only (sprint 10): bring in files the check made, and set a recording's quiet start.
        "test_import" => {
            let paths: Vec<PathBuf> = list(a, "paths").into_iter().map(PathBuf::from).collect();
            json!(j.import(&paths, &mdp_core::jobs::JobContext::new()).map_err(e)?.0.imported)
        }
        "test_quiet" => to(serde_json::to_value(j.set_quiet_start(&s(a, "id"), a["seconds"].as_f64().unwrap_or(0.0)).map_err(e)?))?,
        "waiting_groups" => to(serde_json::to_value(j.waiting_groups().map_err(e)?))?,
        "memory_from_group" => json!(j.memory_from_group(&s(a, "name"), &list(a, "content"), o(a, "date").as_deref(), o(a, "place").as_deref()).map_err(e)?),
        "memory_each" => json!(j.memory_each(&list(a, "content")).map_err(e)?),
        "memory_proposals" => to(serde_json::to_value(j.memory_proposals(&s(a, "id")).map_err(e)?))?,
        "suggested_relationships" => to(serde_json::to_value(j.suggested_relationships(&s(a, "id")).map_err(e)?))?,
        "answer_suggestion" => {
            let sug: mdp_core::sorting::SuggestedRelationship = serde_json::from_value(a["suggestion"].clone()).map_err(|x| x.to_string())?;
            json!(j.answer_suggestion(&sug, b(a, "accept")).map_err(e)?)
        }
        "merge_individuals" => to(serde_json::to_value(j.merge_individuals(&s(a, "keep"), &s(a, "other")).map_err(e)?))?,
        "set_play_full" => to(serde_json::to_value(j.set_play_full(&s(a, "id"), b(a, "full")).map_err(e)?))?,
        "raw_content" => to(serde_json::to_value(j.raw_content(&s(a, "kind")).map_err(e)?))?,
        "show_content" => {
            let id = s(a, "id");
            let (kind, name, date): (String, String, Option<String>) = j
                .conn()
                .query_row("SELECT kind, name, date FROM content WHERE id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(|x| x.to_string())?;
            let file = j.content_file_to_show(&id).map_err(e)?;
            let path = file.as_ref().map(|f| f.0.display().to_string());
            let text = if kind == "text" { path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) } else { None };
            let (start, quiet) = j.play_from(&id).map_err(e)?;
            json!({"kind": kind, "name": name, "date": date, "path": if kind == "text" { None } else { path }, "text": text, "start": start, "quiet": quiet})
        }
        "set_state" => to(serde_json::to_value(j.set_content_state(&list(a, "ids"), &s(a, "to")).map_err(e)?))?,
        "delete_raw" => { j.delete_content(&list(a, "ids")).map_err(e)?; Value::Null }
        "thumbnail" => Value::Null,
        "waiting" | "missing_files" => json!([]),
        "import_status" => IMPORTED.lock().unwrap().clone().unwrap_or(Value::Null),
        "clear_import" => {
            *IMPORTED.lock().unwrap() = None;
            Value::Null
        }
        // Sprint 11: bulk folder import, import plans and exported histories,
        // run at once (the app runs them on their own thread).
        "folder_kind" => json!(if mdp_core::bulk::is_export(std::path::Path::new(&s(a, "folder"))) { "export" } else { "folder" }),
        "start_scan" => {
            let opts: mdp_core::bulk::ScanOptions = serde_json::from_value(a["options"].clone()).unwrap_or_default();
            let ctx = mdp_core::jobs::JobContext::new();
            let zips: Vec<std::path::PathBuf> = a["zips"].as_array().map(|v| v.iter().filter_map(|z| z.as_str().map(Into::into)).collect()).unwrap_or_default();
            let plan = if zips.is_empty() {
                mdp_core::bulk::scan(j, std::path::Path::new(&s(a, "folder")), opts, &ctx)
            } else {
                mdp_core::bulk::Known::read(j).and_then(|k| mdp_core::bulk::scan_zips(&k, j.root(), &zips, opts, &ctx))
            };
            let skipped: Vec<Value> = ctx.skipped().into_iter().map(|k| json!({"item": k.item, "reason": k.reason})).collect();
            *SCAN.lock().unwrap() = Some(match plan {
                Ok(p) => json!({"running": false, "done": 1, "total": 1, "message": "", "plan": p, "error": null, "skipped": skipped}),
                Err(x) => json!({"running": false, "done": 1, "total": 1, "message": "", "plan": null, "error": x.to_string(), "skipped": skipped}),
            });
            Value::Null
        }
        "scan_status" => SCAN.lock().unwrap().clone().unwrap_or(Value::Null),
        "cancel_scan" => {
            *SCAN.lock().unwrap() = None;
            Value::Null
        }
        "check_plan_names" => {
            let mut plan: mdp_core::bulk::Plan = serde_json::from_value(a["plan"].clone()).map_err(|x| x.to_string())?;
            let known = mdp_core::bulk::Known::read(j).map_err(e)?;
            let names = mdp_core::bulk::check_names(&mut plan, &known);
            let places = mdp_core::bulk::check_places(&mut plan, &known, a["towns"].as_bool().unwrap_or(true));
            json!({"plan": plan, "names": names, "places": places})
        }
        "open_plan" => {
            let path = s(a, "path");
            let text = std::fs::read_to_string(&path).map_err(|x| x.to_string())?;
            to(serde_json::to_value(mdp_core::bulk::from_csv(&text, std::path::Path::new(&path)).map_err(e)?))?
        }
        "save_plan" => {
            let plan: mdp_core::bulk::Plan = serde_json::from_value(a["plan"].clone()).map_err(|x| x.to_string())?;
            let path = PathBuf::from(s(a, "path"));
            std::fs::write(&path, mdp_core::bulk::to_csv(&plan, &path)).map_err(|x| x.to_string())?;
            Value::Null
        }
        "save_sample_plan" => {
            let path = PathBuf::from(s(a, "path"));
            let (csv, readme) = mdp_core::bulk::sample_plan();
            std::fs::write(&path, csv).map_err(|x| x.to_string())?;
            let read_me = path.with_file_name("import-plan read-me.txt");
            std::fs::write(&read_me, readme).map_err(|x| x.to_string())?;
            json!(read_me.display().to_string())
        }
        "start_plan_import" => {
            let plan: mdp_core::bulk::Plan = serde_json::from_value(a["plan"].clone()).map_err(|x| x.to_string())?;
            let ctx = mdp_core::jobs::JobContext::new();
            let batch = mdp_core::import::prepare(j.root(), &mdp_core::bulk::plan_files(&plan), &ctx).map_err(e)?;
            let r = j.finish_plan(batch, &plan, b(a, "addMissing")).map_err(e)?;
            *IMPORTED.lock().unwrap() = Some(json!({"running": false, "done": 1, "total": 1, "message": "", "error": null, "summary": {
                "imported": r.imported, "duplicates": r.duplicates, "copies": 0, "skipped": [], "canceled": false, "note": null,
                "memories": r.memories.len(), "matched": 0, "added": 0, "notes": r.notes}}));
            Value::Null
        }
        "look_at_export" => to(serde_json::to_value(mdp_core::rebuild::look(j, std::path::Path::new(&s(a, "folder"))).map_err(e)?))?,
        "start_export_import" => {
            let merge: std::collections::HashMap<String, String> = serde_json::from_value(a["merge"].clone()).unwrap_or_default();
            let ctx = mdp_core::jobs::JobContext::new();
            let leave: std::collections::HashSet<String> = list(a, "leaveOut").into_iter().collect();
            let batch = mdp_core::rebuild::prepare(j.root(), std::path::Path::new(&s(a, "folder")), &leave, &ctx).map_err(e)?;
            let r = j.finish_export(batch, &merge).map_err(e)?;
            *IMPORTED.lock().unwrap() = Some(json!({"running": false, "done": 1, "total": 1, "message": "", "error": null, "summary": {
                "imported": r.files, "duplicates": r.already, "copies": 0, "skipped": [], "canceled": false, "note": null,
                "memories": 0, "matched": r.matched, "added": r.added, "notes": r.notes}}));
            Value::Null
        }
        "transcript" => to(serde_json::to_value(j.transcript_of(&s(a, "id")).map_err(e)?))?,
        "correct_transcript" => {
            let lines: Vec<mdp_core::voice::Line> = serde_json::from_value(a["lines"].clone()).map_err(|x| x.to_string())?;
            to(serde_json::to_value(j.correct_transcript(&s(a, "id"), &lines, b(a, "checked")).map_err(e)?))?
        }
        "type_transcript" => {
            voice(j, "cancel_transcribe", a)?;
            to(serde_json::to_value(j.type_transcript(&s(a, "id"), &s(a, "text")).map_err(e)?))?
        }
        "delete_transcript" => {
            voice(j, "cancel_transcribe", a)?;
            to(serde_json::to_value(j.delete_transcript(&s(a, "id")).map_err(e)?))?
        }
        "delete_recording" => to(serde_json::to_value(j.delete_recording(&s(a, "id")).map_err(e)?))?,
        "save_settings" => {
            SUGGEST.store(a["settings"]["suggestions"].as_bool().unwrap_or(true), std::sync::atomic::Ordering::SeqCst);
            Value::Null
        }
        "backup_info" => {
            let dir = j.root().parent().unwrap().join("Backups");
            json!({"suggested": dir.join(mdp_core::backup::suggested_name(&j.name().map_err(e)?)).display().to_string(), "last": *LAST_BACKUP.lock().unwrap()})
        }
        "start_backup" => {
            let dest = PathBuf::from(s(a, "dest"));
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p).map_err(|x| x.to_string())?;
            }
            let r = mdp_core::backup::backup(j.root(), &dest, &mdp_core::jobs::JobContext::new()).map(|p| p.display().to_string()).map_err(e);
            if r.is_ok() {
                *LAST_BACKUP.lock().unwrap() = Some(mdp_core::journey::now());
            }
            *BACKUP.lock().unwrap() = Some(("backup", r));
            Value::Null
        }
        "restore_plan" => {
            let (name, created) = mdp_core::backup::describe(&PathBuf::from(s(a, "archive"))).map_err(e)?;
            let base = j.root().parent().unwrap().join(format!("{name} (restored)"));
            let mut folder = base.clone();
            let mut k = 2;
            while folder.exists() {
                folder = PathBuf::from(format!("{} ({k})", base.display()));
                k += 1;
            }
            json!({"name": name, "created": created, "folder": folder.display().to_string()})
        }
        "start_restore" => {
            let r = mdp_core::backup::restore(&PathBuf::from(s(a, "archive")), &PathBuf::from(s(a, "folder")), &mdp_core::jobs::JobContext::new())
                .map(|p| p.display().to_string())
                .map_err(e);
            *BACKUP.lock().unwrap() = Some(("restore", r));
            Value::Null
        }
        "backup_status" => match BACKUP.lock().unwrap().as_ref() {
            None => Value::Null,
            Some((kind, r)) => json!({"kind": kind, "running": false, "done": 1, "total": 1, "message": "",
                "path": r.as_ref().ok(), "error": r.as_ref().err(), "canceled": false}),
        },
        "clear_backup" => {
            *BACKUP.lock().unwrap() = None;
            Value::Null
        }
        "cancel_backup" | "show_backup" | "open_log_folder" => Value::Null,
        "log_event" => {
            eprintln!("log: [{}] {}", a["area"].as_str().unwrap_or(""), a["text"].as_str().unwrap_or(""));
            Value::Null
        }
        "version_note" => json!({"before": std::env::var("MDP_TEST_BEFORE").ok(), "now": "0.0.0"}),
        "get_settings" => json!({"author": j.author(), "living_people": "leave out", "exact_places": "town only", "front_door": "bookshelf",
            "suggestions": SUGGEST.load(std::sync::atomic::Ordering::SeqCst),
            "journey_folder": j.root().display().to_string(), "export_folder": "", "folder_warning": null}),
        "export_info" => {
            let left = j.export_left_out(mdp_core::export::Privacy::default()).map_err(e)?;
            let folder = export_dir(j);
            json!({"note": left.sentence(), "left_out": left, "exported": mdp_core::export::is_export(&folder), "folder": folder.display().to_string()})
        }
        "start_export" => {
            let folder = export_dir(j);
            let ctx = mdp_core::jobs::JobContext::new();
            // The checks can ask for the coffee table front door (sprint 13).
            let privacy = mdp_core::export::Privacy::default().with_front_door(a["front_door"].as_str().unwrap_or("bookshelf"));
            let r = j.export(&folder, privacy, &ctx).map_err(e);
            *EXPORT.lock().unwrap() = Some((r, ctx.skipped().into_iter().map(|s| json!({"item": s.item, "reason": s.reason})).collect()));
            Value::Null
        }
        "export_status" => match EXPORT.lock().unwrap().as_ref() {
            None => Value::Null,
            Some((r, skipped)) => json!({"running": false, "done": 1, "total": 1, "message": "", "folder": export_dir(j).display().to_string(),
                "report": r.as_ref().ok(), "error": r.as_ref().err(), "canceled": false, "skipped": skipped}),
        },
        "clear_export" => {
            *EXPORT.lock().unwrap() = None;
            Value::Null
        }
        "cancel_export" | "open_export" | "show_export_folder" => Value::Null,
        "open_preview" => json!(j.preview(mdp_core::export::Privacy::default()).map_err(e)?.1.note),
        c if c.starts_with("record_") || c.starts_with("voice_") || ["transcribe", "set_auto_transcribe", "choose_model", "download_model",
            "use_model_file", "cancel_download", "clear_download", "cancel_transcribe", "show_models_folder", "model_offer_answered"].contains(&c) => voice(j, c, a)?,
        other => return Err(format!("ui_server does not answer {other}")),
    })
}

fn main() {
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).unwrap_or(8765);
    let ui = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mdp-app/ui");
    let check = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mdp-app/ui-check");
    let dir = std::env::temp_dir().join(format!("mdp-ui-{}", std::process::id()));
    let mut j = mdp_core::sample::create(&dir).expect("sample");
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("port");
    eprintln!("ui_server on http://127.0.0.1:{port}/");
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut first = String::new();
        if reader.read_line(&mut first).is_err() {
            continue;
        }
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                break;
            }
            if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0u8; len];
        let _ = reader.read_exact(&mut body);
        let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
        let (status, ctype, out): (&str, &str, Vec<u8>) = if let Some(cmd) = path.strip_prefix("/invoke/") {
            let args: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            match call(&mut j, cmd, &args) {
                Ok(v) => ("200 OK", "application/json", serde_json::to_vec(&v).unwrap()),
                Err(m) => ("400 Bad Request", "text/plain; charset=utf-8", m.into_bytes()),
            }
        } else {
            let name = if path == "/" { "index.html" } else { path.trim_start_matches('/') };
            // The large preview loads Journey files by path (the app uses Tauri's asset protocol).
            let file = if let Some(p) = name.strip_prefix("file/") {
                PathBuf::from(unescape(p))
            } else if let Some(p) = name.strip_prefix("check/") {
                check.join(p)
            } else {
                ui.join(name)
            };
            match std::fs::read(&file) {
                Ok(b) => {
                    let t = match file.extension().and_then(|e| e.to_str()) {
                        Some("html") => "text/html; charset=utf-8",
                        Some("css") => "text/css",
                        Some("js") => "text/javascript",
                        Some("jpg") | Some("jpeg") => "image/jpeg",
                        Some("mp3") => "audio/mpeg",
                        Some("flac") => "audio/flac",
                        Some("png") => "image/png",
                        Some("wav") => "audio/wav",
                        Some("mp4") => "video/mp4",
                        _ => "application/octet-stream",
                    };
                    ("200 OK", t, b)
                }
                Err(_) => ("404 Not Found", "text/plain", b"not found".to_vec()),
            }
        };
        let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", out.len());
        let _ = stream.write_all(&out);
    }
}
