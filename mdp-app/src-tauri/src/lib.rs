//! The app shell and, from
//! sprint 2, import and the Waiting screen.
//! Sprint 3 screens are in `people.rs`, sprint 4 screens in `memories.rs`,
//! sprint 5 in `voice.rs`, sprint 6 (export and preview) in `export.rs` and
//! sprint 7 (backup and restore) in `backup.rs`, sprint 11 (bulk folder
//! import, import plans and exported histories) in `bulk.rs`.
//!
//! A thin layer over `mdp-core`: each command opens, reads or changes the
//! current Journey and returns plain data for the screens in `ui/`.

mod applog;
mod backup;
mod bulk;
mod export;
mod memories;
mod mobile;
mod people;
mod release;
mod voice;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mdp_core::files::FileCheck;
use mdp_core::jobs::JobContext;
use mdp_core::summary::{Entry, FamilyGroup, Home, RawItem};
use mdp_core::{folder, sample, Journey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, State};

/// Settings kept on this device: the author name and the last Journey.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct DeviceSettings {
    author: String,
    last_journey: Option<String>,
    /// The speech model chosen in Settings (sprint 5), for example "medium.en".
    speech_model: Option<String>,
    /// Transcribe each new recording on its own; on unless turned off.
    auto_transcribe: Option<bool>,
    /// The first-run offer to download the speech model was answered.
    model_offered: bool,
    /// The day the app last asked for a newer version (sprint 12).
    update_checked: Option<String>,
}

/// Settings kept with each Journey, in its `settings.json`, so they
/// travel with the project and its backups.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct JourneySettings {
    /// "leave out" (default) or "include" details of living people.
    living_people: String,
    /// "town only" (default) or "exact" for places marked exact.
    exact_places: String,
    /// Show the "could add" suggestions in this Journey.
    suggestions: bool,
    /// Chosen on the first export.
    export_folder: Option<String>,
    /// When this Journey was last backed up, and the folder used (sprint 7).
    last_backup: Option<String>,
    backup_folder: Option<String>,
    /// The phone (sprint 9): a folder picked with Android's folder picker
    /// that gets a copy of each export, and its name for showing.
    export_copy: Option<String>,
    export_copy_name: Option<String>,
    /// The app version that last opened this Journey (sprint 12, upgrade safety).
    opened_with: Option<String>,
    /// The export's front door: "bookshelf" (default) or "coffee table" (sprint 13).
    front_door: String,
}

impl Default for JourneySettings {
    fn default() -> Self {
        JourneySettings { living_people: "leave out".into(), exact_places: "town only".into(), suggestions: true, export_folder: None, last_backup: None, backup_folder: None, export_copy: None, export_copy_name: None, opened_with: None, front_door: "bookshelf".into() }
    }
}

#[derive(Default)]
struct AppState {
    journey: Option<Journey>,
    /// The import running now, or the last one finished.
    import: Option<ImportJob>,
    /// The last check of the Journey's files.
    files: FileCheck,
    /// The folder being looked through for a bulk import (sprint 11).
    scan: Option<bulk::ScanJob>,
    /// A Journey made just now (no "new version" backup offer for it).
    fresh: Option<PathBuf>,
}

#[derive(Clone)]
struct ImportJob {
    ctx: JobContext,
    result: Arc<Mutex<Option<Res<ImportSummary>>>>,
}

#[derive(Serialize, Clone, Default)]
struct ImportSummary {
    imported: usize,
    duplicates: usize,
    copies: usize,
    skipped: Vec<SkippedView>,
    canceled: bool,
    /// Set when copies could not be made (for example, FFmpeg is missing).
    note: Option<String>,
    /// Memories made by a bulk import or an exported history (sprint 11).
    memories: usize,
    /// People, places and items matched to ones already in the Journey.
    matched: usize,
    /// Items an exported history added (people, places, memories, files…).
    added: usize,
    /// Things from an import plan or export that could not be used, in words.
    notes: Vec<String>,
}

#[derive(Serialize, Clone)]
struct SkippedView {
    item: String,
    reason: String,
}

type Shared = Mutex<AppState>;
type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn device_file(app: &AppHandle) -> Res<PathBuf> {
    let dir = app.path().app_config_dir().map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    Ok(dir.join("settings.json"))
}

fn load_device(app: &AppHandle) -> DeviceSettings {
    let mut s: DeviceSettings = device_file(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if s.author.trim().is_empty() {
        s.author = mdp_core::journey::default_author();
    }
    s
}

fn save_device(app: &AppHandle, s: &DeviceSettings) -> Res<()> {
    std::fs::write(device_file(app)?, serde_json::to_string_pretty(s).map_err(err)?).map_err(err)
}

fn journey_settings_file(root: &Path) -> PathBuf {
    root.join("settings.json")
}

fn load_journey_settings(root: &Path) -> JourneySettings {
    std::fs::read_to_string(journey_settings_file(root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Where this Journey's export goes: the folder chosen in Settings, or
/// Documents\Modern Day Pioneer\Exports\<name>.
fn export_folder(app: &AppHandle, j: &Journey) -> Res<String> {
    let js = load_journey_settings(j.root());
    Ok(match js.export_folder {
        Some(f) => f,
        None => default_export_folder(app, &j.name().map_err(err)?),
    })
}

/// A Journey name made safe as a Windows folder name.
fn folder_name(name: &str) -> String {
    let clean: String = name.trim().chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '-' } else { c }).collect();
    let clean = clean.trim_end_matches(['.', ' ']).to_string();
    if clean.is_empty() { "My Journey".into() } else { clean }
}

fn default_export_folder(app: &AppHandle, name: &str) -> String {
    documents(app).join("Modern Day Pioneer").join("Exports").join(folder_name(name)).display().to_string()
}

/// The export goes into a folder named after the Journey inside the place
/// chosen, so its files never land loose on a drive or in a busy folder. A
/// folder that already holds an export, or already has the Journey's name,
/// is used as it is, so earlier choices keep working.
fn export_target(chosen: &str, name: &str) -> String {
    let chosen = Path::new(chosen.trim());
    let wanted = folder_name(name);
    let named = chosen.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&wanted));
    if named || mdp_core::export::is_export(chosen) {
        chosen.display().to_string()
    } else {
        chosen.join(wanted).display().to_string()
    }
}

/// Settings: the export folder for a place just picked.
#[tauri::command]
fn export_target_for(state: State<Shared>, chosen: String) -> Res<String> {
    let s = state.lock().map_err(err)?;
    let j = s.journey.as_ref().ok_or("No Journey is open.")?;
    Ok(export_target(&chosen, &j.name().map_err(err)?))
}

/// On the phone this is the app's own Documents folder on the phone's
/// storage, which a PC can reach over USB (Jared, 2026-10-19).
fn documents(app: &AppHandle) -> PathBuf {
    app.path().document_dir().or_else(|_| app.path().home_dir()).unwrap_or_else(|_| PathBuf::from("."))
}

/// Where a new Journey goes by default: Documents\Modern Day Pioneer\<name>.
fn suggested(app: &AppHandle, name: &str) -> PathBuf {
    let clean: String = name
        .trim()
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '-' } else { c })
        .collect();
    let clean = clean.trim_end_matches(['.', ' ']).to_string();
    let base = documents(app).join("Modern Day Pioneer");
    let first = base.join(if clean.is_empty() { "My Journey".to_string() } else { clean });
    // Never reuse a folder that already holds something.
    let mut candidate = first.clone();
    let mut n = 2;
    while candidate.exists() && std::fs::read_dir(&candidate).map(|mut d| d.next().is_some()).unwrap_or(true) {
        candidate = PathBuf::from(format!("{} ({n})", first.display()));
        n += 1;
    }
    candidate
}

fn remember(app: &AppHandle, j: &mut Journey) -> Res<Home> {
    // Find moved files again before anything else reads them.
    if let Ok(check) = j.check_files(&JobContext::new()) {
        if let Ok(mut s) = app.state::<Shared>().try_lock() {
            s.files = check;
        }
    }
    let mut d = load_device(app);
    j.set_author(&d.author);
    j.defer_copy(true);
    voice::rescue_recordings(j);
    d.last_journey = Some(j.root().display().to_string());
    save_device(app, &d)?;
    j.home().map_err(err)
}

fn with_journey<T>(state: &State<Shared>, f: impl FnOnce(&mut Journey) -> mdp_core::Result<T>) -> Res<T> {
    let mut s = state.lock().map_err(err)?;
    let j = s.journey.as_mut().ok_or("No Journey is open.")?;
    f(j).map_err(err)
}

#[derive(Serialize)]
struct Startup {
    journey: Option<Home>,
    /// Why the last Journey could not be opened, if it could not.
    problem: Option<String>,
}

/// Open the last Journey, if there is one.
#[tauri::command]
fn startup(app: AppHandle, state: State<Shared>) -> Res<Startup> {
    let mut s = state.lock().map_err(err)?;
    if let Some(j) = s.journey.as_ref() {
        return Ok(Startup { journey: Some(j.home().map_err(err)?), problem: None });
    }
    let Some(last) = load_device(&app).last_journey else { return Ok(Startup { journey: None, problem: None }) };
    match Journey::open(Path::new(&last)) {
        Ok(mut j) => {
            let home = remember(&app, &mut j)?;
            s.journey = Some(j);
            Ok(Startup { journey: Some(home), problem: None })
        }
        Err(e) => Ok(Startup { journey: None, problem: Some(format!("The last Journey could not be opened from {last}: {e}")) }),
    }
}

#[derive(Serialize)]
struct Place {
    folder: String,
    /// The folder as the screen shows it (shorter on the phone).
    shown: String,
    warning: Option<String>,
}

#[tauri::command]
fn suggest_folder(app: AppHandle, name: String) -> Place {
    let f = suggested(&app, &name);
    let folder = f.display().to_string();
    Place { warning: folder::cloud_sync_warning(&f), shown: mobile::shown(&app, &folder), folder }
}

#[tauri::command]
fn check_folder(folder: String) -> Option<String> {
    folder::cloud_sync_warning(Path::new(&folder))
}

#[tauri::command]
fn create_journey(app: AppHandle, state: State<Shared>, name: String, folder: Option<String>) -> Res<Home> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Please give the Journey a name.".into());
    }
    let target = folder.filter(|f| !f.trim().is_empty()).map(PathBuf::from).unwrap_or_else(|| suggested(&app, &name));
    let mut j = Journey::create(&target, &name).map_err(err)?;
    let home = remember(&app, &mut j)?;
    let mut s = state.lock().map_err(err)?;
    s.fresh = Some(j.root().to_path_buf());
    s.journey = Some(j);
    Ok(home)
}

/// Record the version opening this Journey, and say which one opened it
/// before when that differs (sprint 12): the app then offers a backup
/// before a new version changes anything. `fresh` is a Journey just made.
fn note_version(app: &AppHandle, root: &Path, fresh: bool) -> Option<String> {
    let now = app.package_info().version.to_string();
    let mut js = load_journey_settings(root);
    let before = js.opened_with.clone();
    if before.as_deref() == Some(now.as_str()) {
        return None;
    }
    js.opened_with = Some(now);
    if let Ok(text) = serde_json::to_string_pretty(&js) {
        let _ = std::fs::write(journey_settings_file(root), text);
    }
    if fresh {
        return None;
    }
    Some(before.unwrap_or_else(|| "an earlier version".into()))
}

#[derive(Serialize, Default)]
struct VersionNote {
    /// The version that opened this Journey before, when it was another one.
    before: Option<String>,
    now: String,
}

/// Asked once after a Journey opens.
#[tauri::command]
fn version_note(app: AppHandle, state: State<Shared>) -> Res<VersionNote> {
    let now = app.package_info().version.to_string();
    let mut s = state.lock().map_err(err)?;
    let root = s.journey.as_mut().ok_or("No Journey is open.")?.root().to_path_buf();
    let fresh = s.fresh.take().is_some_and(|f| f == root);
    drop(s);
    Ok(VersionNote { before: note_version(&app, &root, fresh), now })
}

#[tauri::command]
fn open_journey(app: AppHandle, state: State<Shared>, folder: String) -> Res<Home> {
    let mut j = Journey::open(Path::new(&folder)).map_err(err)?;
    let home = remember(&app, &mut j)?;
    state.lock().map_err(err)?.journey = Some(j);
    Ok(home)
}

/// Open the sample family, making it the first time.
#[tauri::command]
fn open_sample(app: AppHandle, state: State<Shared>) -> Res<Home> {
    let target = documents(&app).join("Modern Day Pioneer").join("Sample - The Rowan Family");
    let made = !target.join(mdp_core::journey::CATALOG_FILE).is_file();
    let mut j = if made { sample::create(&target).map_err(err)? } else { Journey::open(&target).map_err(err)? };
    let home = remember(&app, &mut j)?;
    let mut s = state.lock().map_err(err)?;
    if made {
        s.fresh = Some(j.root().to_path_buf());
    }
    s.journey = Some(j);
    Ok(home)
}

#[tauri::command]
fn home(state: State<Shared>) -> Res<Home> {
    with_journey(&state, |j| j.home())
}

#[tauri::command]
fn stories(state: State<Shared>) -> Res<Vec<Entry>> {
    with_journey(&state, |j| j.stories())
}

#[tauri::command]
fn people(state: State<Shared>) -> Res<Vec<FamilyGroup>> {
    with_journey(&state, |j| j.people_by_family())
}

#[tauri::command]
fn waiting(state: State<Shared>) -> Res<Vec<Entry>> {
    with_journey(&state, |j| j.waiting())
}

#[tauri::command]
fn browse(state: State<Shared>, kinds: Vec<String>) -> Res<Vec<mdp_core::cache::Hit>> {
    with_journey(&state, |j| j.browse(&kinds, 300))
}

#[tauri::command]
fn search(state: State<Shared>, text: String) -> Res<Vec<mdp_core::cache::Hit>> {
    with_journey(&state, |j| j.search(&text, 20))
}

#[tauri::command]
fn item(state: State<Shared>, id: String) -> Res<Option<Value>> {
    with_journey(&state, |j| j.item(&id))
}

#[tauri::command]
fn raw_content(state: State<Shared>, kind: String) -> Res<Vec<RawItem>> {
    with_journey(&state, |j| j.raw_content(&kind))
}

#[tauri::command]
fn set_state(state: State<Shared>, ids: Vec<String>, to: String) -> Res<()> {
    with_journey(&state, |j| j.set_content_state(&ids, &to))
}

/// The screen asks first; the original is never touched.
#[tauri::command]
fn delete_raw(state: State<Shared>, ids: Vec<String>) -> Res<()> {
    with_journey(&state, |j| j.delete_content(&ids))
}

/// A small preview as a data URL, or nothing for recordings and text.
#[tauri::command]
fn thumbnail(state: State<Shared>, id: String) -> Res<Option<String>> {
    use base64::Engine;
    let path = with_journey(&state, |j| {
        // No FFmpeg on the phone: pictures get their preview from the image library.
        Ok(mdp_core::convert::thumbnail(j, &id)?.or_else(|| if cfg!(mobile) { mobile::picture_thumbnail(j, &id) } else { None }))
    })?;
    Ok(path.and_then(|p| std::fs::read(p).ok()).map(|b| {
        format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))
    }))
}

#[tauri::command]
fn missing_files(state: State<Shared>) -> Res<Vec<String>> {
    Ok(state.lock().map_err(err)?.files.missing.clone())
}

/// Start bringing in files and folders. The copying
/// runs on its own thread without holding the Journey, so the app stays
/// usable; recording the batch and making export copies follow.
#[tauri::command]
fn start_import(app: AppHandle, state: State<Shared>, paths: Vec<String>) -> Res<()> {
    begin_import(app, state, What::Paths(paths))
}

/// What an import brings in: files and folders, an import plan (sprint 11),
/// or an exported history with the matches to use (sprint 11).
pub(crate) enum What {
    Paths(Vec<String>),
    Plan(mdp_core::bulk::Plan, bool),
    Export(PathBuf, std::collections::HashMap<String, String>, std::collections::HashSet<String>),
}

pub(crate) fn begin_import(app: AppHandle, state: State<Shared>, what: What) -> Res<()> {
    let mut s = state.lock().map_err(err)?;
    if s.import.as_ref().is_some_and(|job| job.result.lock().map(|r| r.is_none()).unwrap_or(false)) {
        return Err("Something is already being brought in. Please wait for it to finish.".into());
    }
    let root = s.journey.as_ref().ok_or("No Journey is open.")?.root().to_path_buf();
    let job = ImportJob { ctx: JobContext::new(), result: Arc::new(Mutex::new(None)) };
    s.import = Some(job.clone());
    drop(s);
    match &what {
        What::Paths(paths) => applog::log("import", &format!("Bringing in {} item(s): {}", paths.len(), paths.join("; "))),
        What::Plan(plan, add) => applog::log("import", &format!("Bringing in {} of {} file(s) from an import plan at {} (add missing: {add})", plan.rows.iter().filter(|r| r.include).count(), plan.rows.len(), plan.base)),
        What::Export(dir, merge, leave) => applog::log("import", &format!("Bringing in the exported history at {} ({} matches used, {} items left out)", dir.display(), merge.len(), leave.len())),
    }
    std::thread::spawn(move || {
        let out = match what {
            What::Paths(paths) => {
                // Android's pickers give content:// addresses: copy those in first.
                job.ctx.message("Reading the files…".to_string());
                mobile::local_copies(&app, &paths).and_then(|(paths, scratch)| {
                    let out = run_import(&app, &root, Step::Paths(&paths), &job.ctx);
                    if let Some(dir) = scratch {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                    out
                })
            }
            What::Plan(plan, add) => run_import(&app, &root, Step::Plan(&plan, add), &job.ctx),
            What::Export(dir, merge, leave) => run_import(&app, &root, Step::Export(&dir, &merge, &leave), &job.ctx),
        };
        applog::outcome("import", "Import", &out, |s| {
            let skipped: Vec<String> = s.skipped.iter().map(|k| serde_json::to_string(k).unwrap_or_default()).collect();
            format!("{} brought in, {} duplicates, {} copies, {} memories, {} matched, skipped [{}]{}{}{}", s.imported, s.duplicates, s.copies, s.memories, s.matched, skipped.join(", "), if s.canceled { ", canceled" } else { "" }, s.note.as_ref().map(|n| format!(", note: {n}")).unwrap_or_default(), if s.notes.is_empty() { String::new() } else { format!(", notes: {}", s.notes.join(" | ")) })
        });
        if let Ok(mut r) = job.result.lock() {
            *r = Some(out);
        }
    });
    Ok(())
}

enum Step<'a> {
    Paths(&'a [PathBuf]),
    Plan(&'a mdp_core::bulk::Plan, bool),
    Export(&'a Path, &'a std::collections::HashMap<String, String>, &'a std::collections::HashSet<String>),
}

fn run_import(app: &AppHandle, root: &Path, step: Step, ctx: &JobContext) -> Res<ImportSummary> {
    let state = app.state::<Shared>();
    let with = |f: &mut dyn FnMut(&mut Journey) -> mdp_core::Result<()>| -> Res<()> {
        let mut s = state.lock().map_err(err)?;
        let j = s.journey.as_mut().filter(|j| j.root() == root).ok_or("The Journey was closed during the import.")?;
        f(j).map_err(err)
    };
    let mut sum = ImportSummary::default();
    let canceled = |e: mdp_core::Error, sum: &mut ImportSummary| -> Res<bool> {
        match e {
            mdp_core::Error::Canceled => {
                sum.canceled = true;
                Ok(true)
            }
            e => Err(err(e)),
        }
    };
    match step {
        Step::Paths(paths) => {
            let batch = match mdp_core::import::prepare(root, paths, ctx) {
                Ok(b) => b,
                Err(e) => return canceled(e, &mut sum).map(|_| sum),
            };
            let mut batch = Some(batch);
            with(&mut |j| {
                if let Some(b) = batch.take() {
                    let report = j.finish_import(b)?;
                    sum.imported = report.imported.len();
                    sum.duplicates = report.duplicates.len();
                }
                Ok(())
            })?;
        }
        Step::Plan(plan, add) => {
            let files = mdp_core::bulk::plan_files(plan);
            mdp_core::bulk::check_space(root, mdp_core::bulk::total_size(&files)).map_err(err)?;
            let batch = match mdp_core::import::prepare(root, &files, ctx) {
                Ok(b) => b,
                Err(e) => return canceled(e, &mut sum).map(|_| sum),
            };
            let mut batch = Some(batch);
            with(&mut |j| {
                if let Some(b) = batch.take() {
                    let report = j.finish_plan(b, plan, add)?;
                    sum.imported = report.imported;
                    sum.duplicates = report.duplicates;
                    sum.memories = report.memories.len();
                    sum.notes = report.notes;
                }
                Ok(())
            })?;
        }
        Step::Export(dir, merge, leave) => {
            let batch = match mdp_core::rebuild::prepare(root, dir, leave, ctx) {
                Ok(b) => b,
                Err(e) => return canceled(e, &mut sum).map(|_| sum),
            };
            let mut batch = Some(batch);
            with(&mut |j| {
                if let Some(b) = batch.take() {
                    let report = j.finish_export(b, merge)?;
                    sum.imported = report.files;
                    sum.duplicates = report.already;
                    sum.matched = report.matched;
                    sum.added = report.added;
                    sum.notes = report.notes;
                }
                Ok(())
            })?;
        }
    }
    sum.skipped = ctx.skipped().into_iter().map(|k| SkippedView { item: k.item, reason: k.reason }).collect();
    // Export copies for HEIC, HEVC, MOV, Opus, M4A, DNG and documents.
    let Some(tool) = mdp_core::convert::find_tool() else {
        // The phone has no FFmpeg; the copies are made when the Journey is opened on a PC.
        if cfg!(mobile) {
            return Ok(sum);
        }
        sum.note = Some("Copies of HEIC, HEVC, Opus, DNG and document files could not be made because the conversion tool is missing. The files are kept.".into());
        return Ok(sum);
    };
    let mut tasks = Vec::new();
    with(&mut |j| {
        tasks = j.conversions_needed()?;
        Ok(())
    })?;
    if tasks.is_empty() {
        return Ok(sum);
    }
    let before = ctx.skipped().len();
    ctx.message(format!("Making copies of {} files in formats every computer can open…", tasks.len()));
    match mdp_core::convert::prepare(root, &tasks, &tool, ctx) {
        Ok(b) => {
            let mut b = Some(b);
            with(&mut |j| {
                if let Some(b) = b.take() {
                    sum.copies = j.finish_conversions(b)?;
                }
                Ok(())
            })?
        }
        Err(mdp_core::Error::Canceled) => sum.note = Some("Making copies was canceled; it will be tried again on the next import.".into()),
        Err(e) => sum.note = Some(format!("Copies could not be made: {e}")),
    }
    sum.skipped.extend(ctx.skipped().into_iter().skip(before).map(|k| SkippedView { item: k.item, reason: k.reason }));
    Ok(sum)
}

#[derive(Serialize)]
struct ImportStatus {
    running: bool,
    done: u64,
    total: u64,
    message: String,
    summary: Option<ImportSummary>,
    error: Option<String>,
}

#[tauri::command]
fn import_status(state: State<Shared>) -> Res<Option<ImportStatus>> {
    let s = state.lock().map_err(err)?;
    let Some(job) = s.import.as_ref() else { return Ok(None) };
    let p = job.ctx.progress();
    let result = job.result.lock().map_err(err)?.clone();
    Ok(Some(ImportStatus {
        running: result.is_none(),
        done: p.done,
        total: p.total,
        message: p.message,
        summary: result.as_ref().and_then(|r| r.as_ref().ok().cloned()),
        error: result.and_then(|r| r.err()),
    }))
}

#[tauri::command]
fn cancel_import(state: State<Shared>) -> Res<()> {
    if let Some(job) = state.lock().map_err(err)?.import.as_ref() {
        job.ctx.cancel();
    }
    Ok(())
}

/// Clear a finished import's summary once the screen has shown it.
#[tauri::command]
fn clear_import(state: State<Shared>) -> Res<()> {
    let mut s = state.lock().map_err(err)?;
    if s.import.as_ref().is_some_and(|job| job.result.lock().map(|r| r.is_some()).unwrap_or(false)) {
        s.import = None;
    }
    Ok(())
}

/// Folder watching, done by looking every 30 seconds
/// while a Journey is open and nothing is being brought in.
/// Write the plain-text copy a couple of seconds after the last change
///, so edits stay quick in a large Journey.
fn write_copy_later(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let state = app.state::<Shared>();
        let Ok(mut s) = state.try_lock() else { continue };
        if let Some(j) = s.journey.as_mut() {
            if j.copy_due(std::time::Duration::from_secs(2)) {
                if let Err(e) = j.write_copy() {
                    applog::log("error", &format!("The plain-text copy could not be written: {e}"));
                }
            }
        }
    });
}

fn watch_files(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(30));
        let state = app.state::<Shared>();
        let Ok(mut s) = state.lock() else { continue };
        let busy = s.import.as_ref().is_some_and(|job| job.result.lock().map(|r| r.is_none()).unwrap_or(false));
        if busy {
            continue;
        }
        let Some(j) = s.journey.as_mut() else { continue };
        if let Ok(check) = j.check_files(&JobContext::new()) {
            s.files = check;
        }
        drop(s);
        voice::check_quiet_starts(&app);
    });
}

#[tauri::command]
fn undo(state: State<Shared>) -> Res<bool> {
    with_journey(&state, |j| j.undo())
}

#[derive(Serialize, Deserialize)]
struct SettingsView {
    author: String,
    living_people: String,
    exact_places: String,
    /// "bookshelf" or "coffee table" (sprint 13).
    #[serde(default)]
    front_door: String,
    suggestions: bool,
    journey_folder: String,
    export_folder: String,
    folder_warning: Option<String>,
    /// As the screen shows them (shorter on the phone).
    #[serde(default)]
    journey_folder_shown: String,
    #[serde(default)]
    export_folder_shown: String,
}

#[tauri::command]
fn get_settings(app: AppHandle, state: State<Shared>) -> Res<SettingsView> {
    let d = load_device(&app);
    let s = state.lock().map_err(err)?;
    let j = s.journey.as_ref().ok_or("No Journey is open.")?;
    let js = load_journey_settings(j.root());
    let export = export_folder(&app, j)?;
    Ok(SettingsView {
        author: d.author,
        living_people: js.living_people,
        exact_places: js.exact_places,
        front_door: js.front_door,
        suggestions: js.suggestions,
        journey_folder: j.root().display().to_string(),
        journey_folder_shown: mobile::shown(&app, &j.root().display().to_string()),
        export_folder_shown: match js.export_copy_name {
            Some(name) => format!("{}, with a copy in {name}", mobile::shown(&app, &export)),
            None => mobile::shown(&app, &export),
        },
        export_folder: export,
        folder_warning: folder::cloud_sync_warning(j.root()),
    })
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<Shared>, settings: SettingsView) -> Res<()> {
    let mut d = load_device(&app);
    let author = settings.author.trim();
    if !author.is_empty() {
        d.author = author.to_string();
    }
    save_device(&app, &d)?;
    let mut s = state.lock().map_err(err)?;
    let j = s.journey.as_mut().ok_or("No Journey is open.")?;
    j.set_author(&d.author);
    let mut js = load_journey_settings(j.root());
    if ["leave out", "include"].contains(&settings.living_people.as_str()) {
        js.living_people = settings.living_people;
    }
    if ["town only", "exact"].contains(&settings.exact_places.as_str()) {
        js.exact_places = settings.exact_places;
    }
    if ["bookshelf", "coffee table"].contains(&settings.front_door.as_str()) {
        js.front_door = settings.front_door;
    }
    js.suggestions = settings.suggestions;
    let default_export = default_export_folder(&app, &j.name().map_err(err)?);
    // The phone has no typed folders; its export copy is chosen with `pick_export_copy`.
    if cfg!(mobile) {
        return std::fs::write(journey_settings_file(j.root()), serde_json::to_string_pretty(&js).map_err(err)?).map_err(err);
    }
    let name = j.name().map_err(err)?;
    js.export_folder = Some(settings.export_folder)
        .filter(|f| !f.trim().is_empty())
        .map(|f| export_target(&f, &name))
        .filter(|f| *f != default_export);
    std::fs::write(journey_settings_file(j.root()), serde_json::to_string_pretty(&js).map_err(err)?).map_err(err)
}

#[tauri::command]
fn show_journey_folder(app: AppHandle, state: State<Shared>) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let s = state.lock().map_err(err)?;
    let j = s.journey.as_ref().ok_or("No Journey is open.")?;
    app.opener().reveal_item_in_dir(j.root().join("READ-ME.txt")).map_err(err)
}

/// The phone: choose a folder that gets a copy of each export (Android's
/// folder picker). Async, as the picker answers later and a sync command
/// would hold the screen's thread.
#[tauri::command]
async fn pick_export_copy(app: AppHandle) -> Res<Option<String>> {
    let picked = mobile::pick_folder(&app)?;
    let Some(tree) = picked.tree else { return Ok(None) };
    let name = picked.name.or_else(|| mobile::folder_name(&app, &tree)).unwrap_or_else(|| "the folder picked".into());
    let state = app.state::<Shared>();
    let s = state.lock().map_err(err)?;
    let j = s.journey.as_ref().ok_or("No Journey is open.")?;
    let mut js = load_journey_settings(j.root());
    js.export_copy = Some(tree);
    js.export_copy_name = Some(name.clone());
    std::fs::write(journey_settings_file(j.root()), serde_json::to_string_pretty(&js).map_err(err)?).map_err(err)?;
    Ok(Some(name))
}

#[derive(Serialize)]
struct About {
    /// For example "0.2.57": 0, the sprint, and the GitHub build number.
    version: String,
    /// "Build 57 (64af7c1, 2026-10-16)", or "Local build" when built on a PC.
    build: String,
    /// Where "Send feedback" writes to; empty hides it (sprint 12).
    feedback: String,
    /// The public releases page.
    releases: String,
}

fn about_info(app: &AppHandle) -> About {
    let version = app.package_info().version.to_string();
    let build = match (option_env!("MDP_BUILD"), option_env!("MDP_COMMIT"), option_env!("MDP_BUILD_DATE")) {
        (Some(n), Some(c), Some(d)) => format!("Build {n} ({c}, {d})"),
        (Some(n), _, _) => format!("Build {n}"),
        _ => "Local build".into(),
    };
    About { version, build, feedback: release::FEEDBACK_EMAIL.into(), releases: release::releases_page() }
}

/// The phone's media server address (see mobile::start_media_server).
struct MediaBase(Option<String>);

#[tauri::command]
fn media_base(app: AppHandle) -> Option<String> {
    app.try_state::<MediaBase>().and_then(|m| m.0.clone())
}

/// Keep the phone's screen on while long work runs; nothing on the PC.
#[tauri::command(async)]
fn keep_awake(app: AppHandle, on: bool) -> Res<()> {
    if cfg!(mobile) {
        mobile::keep_awake(&app, on)
    } else {
        Ok(())
    }
}

#[tauri::command]
fn about(app: AppHandle) -> About {
    about_info(&app)
}

/// Follow the Windows text size setting: Settings > Accessibility > Text size.
#[cfg(windows)]
fn text_scale() -> f64 {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Microsoft\Accessibility", "/v", "TextScaleFactor"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    out.ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|t| t.split_whitespace().last().map(|s| s.to_string()))
        .and_then(|hex| u32::from_str_radix(hex.trim_start_matches("0x"), 16).ok())
        .map(|pct| (pct.clamp(100, 225)) as f64 / 100.0)
        .unwrap_or(1.0)
}

#[cfg(not(windows))]
fn text_scale() -> f64 {
    1.0
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(mobile::plugin())
        .manage(Shared::default())
        .manage(voice::Voice::default())
        .manage(export::Exports::default())
        .manage(backup::Backups::default())
        .setup(|app| {
            watch_files(app.handle().clone());
            write_copy_later(app.handle().clone());
            // The version in the window title, so testers can always see what they run.
            let info = about_info(app.handle());
            applog::init(app.handle(), &info.version);
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title(&format!("Modern Day Pioneer {} ({})", info.version, info.build.to_lowercase()));
            }
            // The phone shows the preview inside the app, read through the
            // asset protocol from the app's private storage (sprint 9).
            #[cfg(mobile)]
            for dir in [app.path().document_dir(), app.path().app_data_dir()].into_iter().flatten() {
                let _ = app.asset_protocol_scope().allow_directory(dir, true);
            }
            #[cfg(mobile)]
            {
                let roots = [app.path().document_dir(), app.path().app_data_dir(), app.path().app_cache_dir()].into_iter().flatten().collect();
                app.manage(MediaBase(mobile::start_media_server(roots)));
            }
            let scale = text_scale();
            if (scale - 1.0).abs() > f64::EPSILON {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.set_zoom(scale);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            startup,
            media_base,
            keep_awake,
            applog::log_event,
            applog::open_log_folder,
            suggest_folder,
            check_folder,
            create_journey,
            open_journey,
            version_note,
            release::check_for_update,
            release::open_link,
            open_sample,
            home,
            stories,
            people,
            waiting,
            browse,
            search,
            item,
            raw_content,
            set_state,
            delete_raw,
            thumbnail,
            missing_files,
            start_import,
            bulk::start_scan,
            bulk::scan_status,
            bulk::cancel_scan,
            bulk::folder_kind,
            bulk::open_plan,
            bulk::check_plan_names,
            bulk::save_plan,
            bulk::save_sample_plan,
            bulk::start_plan_import,
            bulk::look_at_export,
            bulk::start_export_import,
            import_status,
            cancel_import,
            clear_import,
            about,
            undo,
            get_settings,
            save_settings,
            show_journey_folder,
            pick_export_copy,
            export_target_for,
            people::choices,
            people::read_date,
            people::item_table,
            people::add_individual,
            people::individual_view,
            people::update_individual,
            people::add_relationship,
            people::relationships_of,
            people::update_relationship,
            people::remove_relationship,
            people::add_family,
            people::family_view,
            people::update_family,
            people::add_family_member,
            people::remove_family_member,
            people::relate_families,
            people::unrelate_families,
            people::places_and_events,
            people::add_place,
            people::place_view,
            people::update_place,
            people::merge_places,
            people::suggested_relationships,
            people::answer_suggestion,
            people::merge_individuals,
            people::add_event,
            people::event_view,
            people::update_event,
            people::set_event_people,
            people::removal_warning,
            people::remove_item,
            memories::memories,
            memories::memory_list,
            memories::turn_picture,
            people::portraits,
            people::portrait_choices,
            people::set_portrait,
            people::add_family_members,
            memories::memory_view,
            memories::add_memory,
            memories::update_memory,
            memories::add_to_memory,
            memories::remove_from_memory,
            memories::move_in_memory,
            memories::add_note,
            memories::link_to_memory,
            memories::update_memory_link,
            memories::unlink_from_memory,
            memories::new_memory_with,
            memories::assign_to_individual,
            memories::assign_to_names,
            memories::name_hints,
            memories::link_names_to_memory,
            memories::story_view,
            memories::add_story,
            memories::update_story,
            memories::add_tribute,
            memories::tributes_about,
            memories::set_memory_note,
            memories::add_memories_to_story,
            memories::remove_memory_from_story,
            memories::move_in_story,
            memories::update_journey,
            memories::set_featured,
            memories::move_featured,
            memories::show_content,
            memories::set_play_full,
            memories::waiting_groups,
            memories::memory_from_group,
            memories::memory_each,
            memories::memory_proposals,
            voice::voice_status,
            voice::record_start,
            voice::record_stop,
            voice::set_auto_transcribe,
            voice::choose_model,
            voice::model_offer_answered,
            voice::type_transcript,
            voice::delete_transcript,
            voice::delete_recording,
            voice::download_model,
            voice::use_model_file,
            voice::cancel_download,
            voice::clear_download,
            voice::show_models_folder,
            voice::transcribe,
            voice::cancel_transcribe,
            voice::transcript,
            voice::correct_transcript,
            export::export_info,
            export::start_export,
            export::export_status,
            export::cancel_export,
            export::clear_export,
            export::open_export,
            export::show_export_folder,
            export::open_preview,
            export::phone_preview,
            backup::backup_info,
            backup::start_backup,
            backup::restore_plan,
            backup::start_restore,
            backup::backup_status,
            backup::cancel_backup,
            backup::clear_backup,
            backup::show_backup
        ])
        .build(tauri::generate_context!())
        .expect("error while running Modern Day Pioneer")
        .run(|app, event| {
            // Closing the app always leaves a current plain-text copy.
            if let tauri::RunEvent::Exit = event {
                if let Ok(mut s) = app.state::<Shared>().lock() {
                    if let Some(j) = s.journey.as_mut() {
                        let _ = j.write_copy();
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_goes_in_a_folder_named_after_the_journey() {
        let base = std::env::temp_dir().join("mdp-export-target-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let b = base.display().to_string();
        let want = base.join("The Rowan Family").display().to_string();
        // A place picked gets the Journey's folder inside it.
        assert_eq!(export_target(&b, "The Rowan Family"), want);
        // Saving again keeps it (the name is already there).
        assert_eq!(export_target(&want, "The Rowan Family"), want);
        // Names Windows refuses are made safe.
        assert_eq!(export_target(&b, "Mom/Dad: 1960s?. "), base.join("Mom-Dad- 1960s-").display().to_string());
        assert_eq!(folder_name("  "), "My Journey");
        // A folder that already holds an export is used as it is.
        let old = base.join("Family export");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(mdp_core::export::ABOUT_FILE), "x").unwrap();
        std::fs::write(old.join("catalog.json"), "{}").unwrap();
        assert_eq!(export_target(&old.display().to_string(), "The Rowan Family"), old.display().to_string());
        let _ = std::fs::remove_dir_all(&base);
    }
}
