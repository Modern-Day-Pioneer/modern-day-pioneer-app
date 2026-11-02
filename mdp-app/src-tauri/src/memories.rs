//! Sprint 4 screens: memories, roles, stories, the Journey's title and
//! featured items, and the Waiting screen's "put into a memory", "assign to
//! a person" and large preview. Each command is
//! a thin call into `mdp-core`, which holds the rules and the tests.

use mdp_core::memories::MemoryView;
use mdp_core::stories::StoryView;
use mdp_core::summary::Entry;
use mdp_core::Row;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{with_journey, Res, Shared};

#[tauri::command]
pub fn memories(state: State<Shared>) -> Res<Vec<Entry>> {
    with_journey(&state, |j| j.memories())
}

#[tauri::command]
pub fn memory_list(state: State<Shared>) -> Res<Vec<mdp_core::stories::MemoryRow>> {
    with_journey(&state, |j| j.memory_list())
}

#[tauri::command]
pub fn memory_view(state: State<Shared>, id: String) -> Res<MemoryView> {
    with_journey(&state, |j| j.memory_view(&id))
}

#[tauri::command]
pub fn add_memory(state: State<Shared>, name: String, story: Option<String>) -> Res<String> {
    with_journey(&state, |j| j.add_memory(&name, story.as_deref()))
}

#[tauri::command]
pub fn update_memory(state: State<Shared>, id: String, changes: Row) -> Res<()> {
    with_journey(&state, |j| j.update_memory(&id, &changes))
}

#[tauri::command]
pub fn add_to_memory(state: State<Shared>, memory: String, content: Vec<String>) -> Res<()> {
    with_journey(&state, |j| j.add_to_memory(&memory, &content))
}

#[tauri::command]
pub fn remove_from_memory(state: State<Shared>, memory: String, content: String) -> Res<()> {
    with_journey(&state, |j| j.remove_from_memory(&memory, &content))
}

#[tauri::command]
pub fn move_in_memory(state: State<Shared>, memory: String, content: String, delta: i64) -> Res<()> {
    with_journey(&state, |j| j.move_in_memory(&memory, &content, delta))
}

#[tauri::command]
pub fn add_note(state: State<Shared>, memory: String, title: String, text: String, from_caption: bool) -> Res<String> {
    with_journey(&state, |j| j.add_note(&memory, &title, &text, from_caption))
}

#[tauri::command]
pub fn link_to_memory(state: State<Shared>, memory: String, who: String, role: String, role_note: Option<String>, probably: bool) -> Res<()> {
    with_journey(&state, |j| j.link_to_memory(&memory, &who, &role, role_note.as_deref(), probably))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn update_memory_link(
    state: State<Shared>,
    memory: String,
    who: String,
    old_role: String,
    role: String,
    role_note: Option<String>,
    probably: bool,
) -> Res<()> {
    with_journey(&state, |j| j.update_memory_link(&memory, &who, &old_role, &role, role_note.as_deref(), probably))
}

#[tauri::command]
pub fn unlink_from_memory(state: State<Shared>, memory: String, who: String, role: String) -> Res<()> {
    with_journey(&state, |j| j.unlink_from_memory(&memory, &who, &role))
}

#[tauri::command]
pub fn new_memory_with(state: State<Shared>, name: String, content: Vec<String>, story: Option<String>) -> Res<String> {
    with_journey(&state, |j| j.new_memory_with(&name, &content, story.as_deref()))
}

#[tauri::command]
pub fn assign_to_individual(state: State<Shared>, who: String, content: Vec<String>) -> Res<String> {
    with_journey(&state, |j| j.assign_to_individual(&who, &content))
}

#[tauri::command]
pub fn link_names_to_memory(state: State<Shared>, memory: String, names: String, role: String, role_note: Option<String>, probably: bool) -> Res<mdp_core::memories::Assigned> {
    with_journey(&state, |j| j.link_names_to_memory(&memory, &names, &role, role_note.as_deref(), probably))
}

#[tauri::command]
pub fn name_hints(state: State<Shared>) -> Res<Vec<mdp_core::memories::NameHint>> {
    with_journey(&state, |j| j.name_hints())
}

#[tauri::command]
pub fn assign_to_names(state: State<Shared>, names: String, content: Vec<String>) -> Res<mdp_core::memories::Assigned> {
    with_journey(&state, |j| j.assign_to_names(&names, &content))
}

#[tauri::command]
pub fn story_view(state: State<Shared>, id: String) -> Res<StoryView> {
    with_journey(&state, |j| j.story_view(&id))
}

#[tauri::command]
pub fn add_story(state: State<Shared>, name: String) -> Res<String> {
    with_journey(&state, |j| j.add_story(&name))
}

/// "In memory of" pages (sprint 13).
#[tauri::command]
pub fn add_tribute(state: State<Shared>, about: String, name: String) -> Res<String> {
    with_journey(&state, |j| j.add_tribute(&about, &name))
}

#[tauri::command]
pub fn tributes_about(state: State<Shared>, about: String) -> Res<Vec<Entry>> {
    with_journey(&state, |j| j.tributes_about(&about))
}

#[tauri::command]
pub fn set_memory_note(state: State<Shared>, story: String, memory: String, note: String) -> Res<()> {
    with_journey(&state, |j| j.set_memory_note(&story, &memory, &note))
}

#[tauri::command]
pub fn update_story(state: State<Shared>, id: String, changes: Row) -> Res<()> {
    with_journey(&state, |j| j.update_story(&id, &changes))
}

#[tauri::command]
pub fn add_memories_to_story(state: State<Shared>, story: String, memories: Vec<String>) -> Res<()> {
    with_journey(&state, |j| j.add_memories_to_story(&story, &memories))
}

#[tauri::command]
pub fn remove_memory_from_story(state: State<Shared>, story: String, memory: String) -> Res<()> {
    with_journey(&state, |j| j.remove_memory_from_story(&story, &memory))
}

#[tauri::command]
pub fn move_in_story(state: State<Shared>, story: String, memory: String, delta: i64) -> Res<()> {
    with_journey(&state, |j| j.move_in_story(&story, &memory, delta))
}

#[tauri::command]
pub fn update_journey(state: State<Shared>, changes: Row) -> Res<()> {
    with_journey(&state, |j| j.update_journey(&changes))
}

#[tauri::command]
pub fn set_featured(state: State<Shared>, id: String, on: bool) -> Res<()> {
    with_journey(&state, |j| j.set_featured(&id, on))
}

#[tauri::command]
pub fn move_featured(state: State<Shared>, id: String, delta: i64) -> Res<()> {
    with_journey(&state, |j| j.move_featured(&id, delta))
}

/// What the large preview shows for one item.
#[derive(Serialize)]
pub struct Shown {
    kind: String,
    name: String,
    date: Option<String>,
    /// The Journey's copy to load through the asset protocol (pictures, recordings, videos).
    path: Option<String>,
    /// The words of a text item.
    text: Option<String>,
    /// For a recording: where to start playing (its quiet start skipped)
    /// and the quiet start found, in seconds; 0 when there is none.
    start: f64,
    quiet: f64,
}

/// Text items are shown up to this size; longer ones are cut with a note.
const TEXT_LIMIT: usize = 200_000;

#[tauri::command]
pub fn show_content(app: AppHandle, state: State<Shared>, id: String) -> Res<Shown> {
    let (kind, name, date, file, (start, quiet)) = with_journey(&state, |j| {
        let (kind, name, date): (String, String, Option<String>) =
            j.conn().query_row("SELECT kind, name, date FROM content WHERE id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok((kind, name, date, j.content_file_to_show(&id)?, j.play_from(&id)?))
    })?;
    let Some((path, format)) = file else {
        return Ok(Shown { kind, name, date, path: None, text: None, start, quiet });
    };
    if kind == "text" && format != "text/plain" {
        // A document (PDF, Word) whose readable copies are not made yet.
        let text = "This document is kept as it is. Its text and page pictures are made on a PC, where they show here and in the export.".to_string();
        return Ok(Shown { kind, name, date, path: None, text: Some(text), start, quiet });
    }
    if kind == "text" {
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        if text.len() > TEXT_LIMIT {
            let mut cut = TEXT_LIMIT;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            text.push_str("\n\n[The rest is in the file.]");
        }
        return Ok(Shown { kind, name, date, path: None, text: Some(text), start, quiet });
    }
    // Only the file being shown is opened to the screen.
    app.asset_protocol_scope().allow_file(&path).map_err(|e| e.to_string())?;
    Ok(Shown { kind, name, date, path: Some(path.display().to_string()), text: None, start, quiet })
}

/// Sprint 10: play a recording whole, or skip its quiet start again.
#[tauri::command]
pub fn set_play_full(state: State<Shared>, id: String, full: bool) -> Res<()> {
    with_journey(&state, |j| j.set_play_full(&id, full))
}

// ---------- Sprint 10: sorting and tidying ----------

#[tauri::command]
pub fn waiting_groups(state: State<Shared>) -> Res<Vec<mdp_core::sorting::Group>> {
    with_journey(&state, |j| j.waiting_groups())
}

#[tauri::command]
pub fn memory_from_group(state: State<Shared>, name: String, content: Vec<String>, date: Option<String>, place: Option<String>) -> Res<String> {
    with_journey(&state, |j| j.memory_from_group(&name, &content, date.as_deref(), place.as_deref()))
}

#[tauri::command]
pub fn memory_each(state: State<Shared>, content: Vec<String>) -> Res<Vec<String>> {
    with_journey(&state, |j| j.memory_each(&content))
}

#[tauri::command]
pub fn memory_proposals(state: State<Shared>, id: String) -> Res<Vec<mdp_core::sorting::Proposal>> {
    with_journey(&state, |j| j.memory_proposals(&id))
}

/// Sprint 8: turn a picture by quarter turns; the original is not changed.
#[tauri::command]
pub fn turn_picture(state: State<Shared>, id: String, quarters: i64) -> Res<i64> {
    with_journey(&state, |j| j.turn_picture(&id, quarters))
}
