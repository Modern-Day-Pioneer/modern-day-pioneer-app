//! Sprint 11 (release 2): bulk folder import with a preview, import plans
//! (CSV), and exported histories brought back in (mdp_core::bulk
//! and mdp_core::rebuild). Looking through a folder runs on its own thread
//! like an import; the import itself is an ordinary import job.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mdp_core::bulk::{self, Plan, ScanOptions};
use mdp_core::jobs::JobContext;
use mdp_core::rebuild;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{applog, begin_import, err, Res, Shared, SkippedView, What};

#[derive(Clone)]
pub(crate) struct ScanJob {
    ctx: JobContext,
    result: Arc<Mutex<Option<Res<Plan>>>>,
}

impl ScanJob {
    /// Stop looking through the folder (the Journey is changing).
    pub(crate) fn cancel(&self) {
        self.ctx.cancel();
    }
}

#[derive(Serialize)]
pub(crate) struct ScanStatus {
    running: bool,
    done: u64,
    total: u64,
    message: String,
    plan: Option<Plan>,
    error: Option<String>,
    skipped: Vec<SkippedView>,
}

/// "export" for a history the app exported, "folder" for any other folder.
#[tauri::command]
pub(crate) fn folder_kind(folder: String) -> String {
    if bulk::is_export(std::path::Path::new(&folder)) { "export" } else { "folder" }.into()
}

/// Start looking through a folder, or through chosen zip files (`zips`), for
/// the preview. Only the names of people and places are read from the
/// Journey; the rest runs without holding it.
#[tauri::command]
pub(crate) fn start_scan(state: State<Shared>, folder: String, zips: Option<Vec<String>>, options: ScanOptions) -> Res<()> {
    let mut s = state.lock().map_err(err)?;
    let j = s.journey.as_ref().ok_or("No Journey is open.")?;
    let known = bulk::Known::read(j).map_err(err)?;
    let root = j.root().to_path_buf();
    if let Some(old) = s.scan.take() {
        old.ctx.cancel();
    }
    let job = ScanJob { ctx: JobContext::new(), result: Arc::new(Mutex::new(None)) };
    s.scan = Some(job.clone());
    drop(s);
    let zips: Vec<PathBuf> = zips.unwrap_or_default().into_iter().map(PathBuf::from).collect();
    applog::log("import", &if zips.is_empty() { format!("Looking through {folder} for a bulk import") } else { format!("Looking inside {} zip file(s) for a bulk import", zips.len()) });
    std::thread::spawn(move || {
        let out = if zips.is_empty() { bulk::scan_with(&known, &root, &PathBuf::from(&folder), options, &job.ctx) } else { bulk::scan_zips(&known, &root, &zips, options, &job.ctx) }.map_err(err);
        if let Ok(mut r) = job.result.lock() {
            *r = Some(out);
        }
    });
    Ok(())
}

/// Polled while a folder is looked through: off the main thread, so a busy
/// Journey never freezes the window.
#[tauri::command(async)]
pub(crate) fn scan_status(state: State<'_, Shared>) -> Res<Option<ScanStatus>> {
    let s = state.lock().map_err(err)?;
    let Some(job) = s.scan.as_ref() else { return Ok(None) };
    let p = job.ctx.progress();
    let result = job.result.lock().map_err(err)?.clone();
    Ok(Some(ScanStatus {
        running: result.is_none(),
        done: p.done,
        total: p.total,
        message: p.message,
        skipped: job.ctx.skipped().into_iter().map(|k| SkippedView { item: k.item, reason: k.reason }).collect(),
        plan: result.as_ref().and_then(|r| r.as_ref().ok().cloned()),
        error: result.and_then(|r| r.err()),
    }))
}

#[tauri::command]
pub(crate) fn cancel_scan(state: State<Shared>) -> Res<()> {
    let mut s = state.lock().map_err(err)?;
    if let Some(job) = s.scan.take() {
        job.ctx.cancel();
    }
    // Previews made for the list go with it (sprint 18).
    if let Some(j) = s.journey.as_ref() {
        mdp_core::preview::clear(j.root());
    }
    Ok(())
}

// ---------- Previews before anything is copied in (sprint 18) ----------
// Read from the originals only; what they need is kept in the Journey's
// throwaway cache/preview folder. They run off the main thread, so the
// screen stays quick while FFmpeg works.

/// The open Journey's folder and the full path of a row's file.
fn preview_paths(app: &AppHandle, base: &str, file: &str) -> Res<(PathBuf, PathBuf)> {
    let s = app.state::<Shared>();
    let s = s.lock().map_err(err)?;
    let root = s.journey.as_ref().ok_or("No Journey is open.")?.root().to_path_buf();
    Ok((root, bulk::resolve(base, file)))
}

async fn off_main<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?
}

/// A small picture of a row's picture or video, as a data URL, or nothing.
#[tauri::command]
pub(crate) async fn preview_thumb(app: AppHandle, base: String, file: String) -> Res<Option<String>> {
    use base64::Engine;
    let (root, src) = preview_paths(&app, &base, &file)?;
    off_main(move || {
        let path = mdp_core::preview::thumbnail(&root, &src).map_err(err)?;
        Ok(path.and_then(|p| std::fs::read(p).ok()).map(|b| format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))))
    })
    .await
}

/// The first lines of a row's text file or document, or nothing.
#[tauri::command]
pub(crate) async fn preview_lines(app: AppHandle, base: String, file: String) -> Res<Option<String>> {
    let (root, src) = preview_paths(&app, &base, &file)?;
    off_main(move || mdp_core::preview::first_lines(&root, &src).map_err(err)).await
}

/// The path to play a row's recording or video from: the original itself,
/// or for a file inside a zip, that one file pulled out into the preview
/// folder. Only this file is opened to the screen, read only.
#[tauri::command]
pub(crate) async fn preview_play(app: AppHandle, base: String, file: String) -> Res<String> {
    let (root, src) = preview_paths(&app, &base, &file)?;
    let path = off_main(move || mdp_core::preview::readable(&root, &src).map_err(err)).await?;
    app.asset_protocol_scope().allow_file(&path).map_err(err)?;
    Ok(path.display().to_string())
}

#[derive(Serialize)]
pub(crate) struct CheckedNames {
    plan: Plan,
    names: Vec<bulk::FoundName>,
    places: Vec<bulk::FoundPlace>,
}

/// The names in a plan's People, once each, with the rows' notes worked
/// out again from the Journey and the choices made (Names found), and the
/// same for Google's locations (Places found; `towns` suggests the nearest
/// town from the list built in).
#[tauri::command]
pub(crate) fn check_plan_names(state: State<Shared>, mut plan: Plan, towns: Option<bool>) -> Res<CheckedNames> {
    let s = state.lock().map_err(err)?;
    let known = bulk::Known::read(s.journey.as_ref().ok_or("No Journey is open.")?).map_err(err)?;
    drop(s);
    let names = bulk::check_names(&mut plan, &known);
    let places = bulk::check_places(&mut plan, &known, towns.unwrap_or(true));
    Ok(CheckedNames { plan, names, places })
}

#[tauri::command]
pub(crate) fn open_plan(path: String) -> Res<Plan> {
    let bytes = std::fs::read(&path).map_err(|e| format!("The import plan could not be read ({e})."))?;
    let text = String::from_utf8(bytes).unwrap_or_else(|e| e.into_bytes().iter().map(|&c| c as char).collect());
    bulk::from_csv(&text, std::path::Path::new(&path)).map_err(err)
}

#[tauri::command]
pub(crate) fn save_plan(path: String, plan: Plan) -> Res<()> {
    let p = PathBuf::from(&path);
    std::fs::write(&p, bulk::to_csv(&plan, &p)).map_err(|e| format!("The import plan could not be saved ({e})."))
}

/// Save a blank sample plan, with its read-me beside it.
#[tauri::command]
pub(crate) fn save_sample_plan(path: String) -> Res<String> {
    let p = PathBuf::from(&path);
    let (csv, readme) = bulk::sample_plan();
    std::fs::write(&p, csv).map_err(|e| format!("The sample plan could not be saved ({e})."))?;
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "import-plan".into());
    let read_me = p.with_file_name(format!("{stem} read-me.txt"));
    std::fs::write(&read_me, readme).map_err(|e| format!("The read-me could not be saved ({e})."))?;
    Ok(read_me.display().to_string())
}

#[tauri::command]
pub(crate) fn start_plan_import(app: AppHandle, state: State<Shared>, plan: Plan, add_missing: bool) -> Res<()> {
    if !plan.rows.iter().any(|r| r.include) {
        return Err("Tick at least one file first.".into());
    }
    begin_import(app, state, What::Plan(plan, add_missing))
}

#[tauri::command]
pub(crate) fn look_at_export(state: State<Shared>, folder: String) -> Res<rebuild::Look> {
    let s = state.lock().map_err(err)?;
    let j = s.journey.as_ref().ok_or("No Journey is open.")?;
    rebuild::look(j, std::path::Path::new(&folder)).map_err(err)
}

/// Bring an exported history in; `merge` maps export IDs to the people,
/// places and families already in the Journey that the person ticked, and
/// `leave_out` lists the export IDs they chose not to bring in.
#[tauri::command]
pub(crate) fn start_export_import(app: AppHandle, state: State<Shared>, folder: String, merge: HashMap<String, String>, leave_out: Vec<String>) -> Res<()> {
    begin_import(app, state, What::Export(PathBuf::from(folder), merge, leave_out.into_iter().collect()))
}
