//! Sprint 6 screens: export and preview.
//! The rules and templates are in `mdp_core::export`; this file runs them.
//!
//! The export reads the catalog under the Journey's lock (quick), then
//! copies files and writes pages on its own thread without it, so the app
//! stays usable. The preview is made in the Journey's cache by the same
//! generator and shown in a second window.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mdp_core::export::{self, Privacy, Report, FRONT};
use mdp_core::jobs::{JobContext, Skipped};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::{err, journey_settings_file, load_journey_settings, Res, Shared};

#[derive(Default)]
pub struct Exports {
    job: Mutex<Option<Job>>,
}

#[derive(Clone)]
struct Job {
    ctx: JobContext,
    folder: String,
    result: Arc<Mutex<Option<Result<Report, String>>>>,
}

fn privacy_of(root: &std::path::Path) -> Privacy {
    let js = load_journey_settings(root);
    Privacy::from_settings(&js.living_people, &js.exact_places).with_front_door(&js.front_door).with_export_style(&js.export_style).with_checksums(js.export_checksums)
}

#[derive(Serialize)]
pub struct ExportInfo {
    /// The one-line note of what is left out.
    note: String,
    left_out: export::LeftOut,
    /// Where the export goes.
    folder: String,
    /// An export is already there.
    exported: bool,
}

#[tauri::command]
pub fn export_info(app: AppHandle, state: State<Shared>) -> Res<ExportInfo> {
    let s = state.lock().map_err(err)?;
    let j = s.journey.as_ref().ok_or("No Journey is open.")?;
    let left = j.export_left_out(privacy_of(j.root())).map_err(err)?;
    let folder = crate::export_folder(&app, j)?;
    let exported = export::is_export(std::path::Path::new(&folder));
    let folder = match load_journey_settings(j.root()).export_copy_name {
        Some(name) => format!("{}, with a copy in {name}", crate::mobile::shown(&app, &folder)),
        None => crate::mobile::shown(&app, &folder),
    };
    Ok(ExportInfo { note: left.sentence(), left_out: left, exported, folder })
}

#[tauri::command]
pub fn start_export(app: AppHandle, state: State<Shared>, exports: State<Exports>) -> Res<()> {
    let mut slot = exports.job.lock().map_err(err)?;
    if slot.as_ref().is_some_and(|j| j.result.lock().map(|r| r.is_none()).unwrap_or(false)) {
        return Err("An export is already being made. Please wait for it to finish.".into());
    }
    let (plan, folder, copy_to) = {
        let s = state.lock().map_err(err)?;
        let j = s.journey.as_ref().ok_or("No Journey is open.")?;
        let plan = export::plan(j, privacy_of(j.root())).map_err(err)?;
        let folder = crate::export_folder(&app, j)?;
        let copy_to = load_journey_settings(j.root()).export_copy.map(|t| (t, j.name().unwrap_or_default()));
        // The folder is remembered from the first export on.
        let mut js = load_journey_settings(j.root());
        if js.export_folder.is_none() {
            js.export_folder = Some(folder.clone());
            let _ = std::fs::write(journey_settings_file(j.root()), serde_json::to_string_pretty(&js).map_err(err)?);
        }
        (plan, folder, copy_to)
    };
    let job = Job { ctx: JobContext::new(), folder: folder.clone(), result: Arc::new(Mutex::new(None)) };
    *slot = Some(job.clone());
    drop(slot);
    crate::applog::log("export", &format!("Exporting to {folder}"));
    std::thread::spawn(move || {
        let out = export::write(&plan, &PathBuf::from(&folder), &job.ctx).map_err(|e| match e {
            mdp_core::Error::Canceled => "canceled".to_string(),
            e => e.to_string(),
        });
        // The phone: then a copy into the folder picked in Settings.
        let out = match (out, copy_to) {
            (Ok(report), Some((tree, name))) => {
                job.ctx.message("Saving a copy in the folder you picked…".to_string());
                crate::mobile::copy_tree_out(&app, std::path::Path::new(&folder), &tree, &format!("{name} export"))
                    .map(|_| report)
                    .map_err(|e| format!("The export was made in the app, but the copy in the folder you picked failed: {e}"))
            }
            (out, _) => out,
        };
        crate::applog::outcome("export", "Export", &out, |r| format!("{} files", r.files));
        if let Ok(mut r) = job.result.lock() {
            *r = Some(out);
        }
    });
    Ok(())
}

#[derive(Serialize)]
pub struct ExportStatus {
    running: bool,
    done: u64,
    total: u64,
    message: String,
    folder: String,
    report: Option<Report>,
    error: Option<String>,
    canceled: bool,
    skipped: Vec<SkippedView>,
}

#[derive(Serialize)]
pub struct SkippedView {
    item: String,
    reason: String,
}

#[tauri::command]
pub fn export_status(app: AppHandle, exports: State<Exports>) -> Res<Option<ExportStatus>> {
    let slot = exports.job.lock().map_err(err)?;
    let Some(job) = slot.as_ref() else { return Ok(None) };
    let p = job.ctx.progress();
    let result = job.result.lock().map_err(err)?.clone();
    let canceled = matches!(&result, Some(Err(e)) if e == "canceled");
    let skipped: Vec<Skipped> = job.ctx.skipped();
    Ok(Some(ExportStatus {
        running: result.is_none(),
        done: p.done,
        total: p.total,
        message: p.message,
        folder: crate::mobile::shown(&app, &job.folder),
        report: result.as_ref().and_then(|r| r.as_ref().ok().cloned()),
        error: result.and_then(|r| r.err()).filter(|e| e != "canceled"),
        canceled,
        skipped: skipped.into_iter().map(|s| SkippedView { item: s.item, reason: s.reason }).collect(),
    }))
}

#[tauri::command]
pub fn cancel_export(exports: State<Exports>) -> Res<()> {
    if let Some(job) = exports.job.lock().map_err(err)?.as_ref() {
        job.ctx.cancel();
    }
    Ok(())
}

/// Forget a finished export's result once the screen has shown it.
#[tauri::command]
pub fn clear_export(exports: State<Exports>) -> Res<()> {
    let mut slot = exports.job.lock().map_err(err)?;
    if slot.as_ref().is_some_and(|j| j.result.lock().map(|r| r.is_some()).unwrap_or(true)) {
        *slot = None;
    }
    Ok(())
}

/// Open the exported history's front door in the computer's browser.
#[tauri::command]
pub fn open_export(app: AppHandle, state: State<Shared>) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let folder = {
        let s = state.lock().map_err(err)?;
        let j = s.journey.as_ref().ok_or("No Journey is open.")?;
        crate::export_folder(&app, j)?
    };
    let front = PathBuf::from(folder).join(FRONT);
    if !front.is_file() {
        return Err("There is no export yet. Choose Export first.".into());
    }
    app.opener().open_path(front.display().to_string(), None::<&str>).map_err(err)
}

/// Show the export folder in File Explorer.
#[tauri::command]
pub fn show_export_folder(app: AppHandle, state: State<Shared>) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let folder = {
        let s = state.lock().map_err(err)?;
        let j = s.journey.as_ref().ok_or("No Journey is open.")?;
        crate::export_folder(&app, j)?
    };
    let about = PathBuf::from(&folder).join(export::ABOUT_FILE);
    if about.is_file() {
        app.opener().reveal_item_in_dir(about).map_err(err)
    } else {
        Err("There is no export yet. Choose Export first.".into())
    }
}

/// Make the preview and show it in its own window. `browser` opens it
/// in the computer's browser instead.
// Async: building a window inside a sync command deadlocks on Windows.
#[tauri::command]
pub async fn open_preview(app: AppHandle, browser: Option<bool>) -> Res<String> {
    let (front, report, inside) = {
        let state = app.state::<Shared>();
        let s = state.lock().map_err(err)?;
        let j = s.journey.as_ref().ok_or("No Journey is open.")?;
        let (front, report) = j.preview(privacy_of(j.root())).map_err(err)?;
        (front, report, j.root().to_path_buf())
    };
    if browser.unwrap_or(false) {
        use tauri_plugin_opener::OpenerExt;
        app.opener().open_path(front.display().to_string(), None::<&str>).map_err(err)?;
        return Ok(report.note);
    }
    let url = tauri::Url::from_file_path(&front).map_err(|_| "The preview's folder could not be opened.".to_string())?;
    if let Some(w) = app.get_webview_window("preview") {
        let _ = w.close();
    }
    // A file the window can't show (a Word original, a DNG) would start a
    // download, which freezes the window on Windows: it opens in its own
    // program instead, but only a file inside the Journey folder: the
    // pages link only to the Journey's own files, so anything else is refused.
    let opener = app.clone();
    let window = WebviewWindowBuilder::new(&app, "preview", WebviewUrl::External(url))
        .on_navigation(move |u| {
            if u.scheme() != "file" || shows_in_window(u.path()) {
                return true;
            }
            if let Some(p) = u.to_file_path().ok().filter(|p| p.starts_with(&inside) && !p.components().any(|c| c == std::path::Component::ParentDir)) {
                use tauri_plugin_opener::OpenerExt;
                let _ = opener.opener().open_path(p.display().to_string(), None::<&str>);
            }
            false
        })
        .on_download(|_, _| false);
    // The phone has no second window; its screen uses `phone_preview` instead.
    #[cfg(desktop)]
    let window = window.title("Preview of the export").inner_size(1000.0, 800.0);
    window.build().map_err(err)?;
    Ok(report.note)
}

/// Files the preview window shows itself; anything else opens in its own program.
fn shows_in_window(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    ["html", "htm", "css", "js", "json", "jsonld", "txt", "vtt", "pdf", "jpg", "jpeg", "png", "gif", "webp", "avif", "bmp", "svg", "mp3", "m4a", "wav", "flac", "ogg", "oga", "opus", "mp4", "m4v", "webm"]
        .contains(&ext.as_str())
}

#[derive(Serialize)]
pub struct PhonePreview {
    /// The preview's front page, shown in a frame over the app.
    front: String,
    note: String,
}

/// Make the preview for the phone, which has no second window (sprint 9):
/// the screen shows `front` in a frame through the asset protocol.
#[tauri::command]
pub fn phone_preview(state: State<Shared>) -> Res<PhonePreview> {
    let s = state.lock().map_err(err)?;
    let j = s.journey.as_ref().ok_or("No Journey is open.")?;
    let (front, report) = j.preview(privacy_of(j.root())).map_err(err)?;
    Ok(PhonePreview { front: front.display().to_string(), note: report.note })
}
