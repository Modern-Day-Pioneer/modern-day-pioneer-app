//! Sprint 7 screens: back up and restore.
//! The archive itself is `mdp_core::backup`; this file runs it as a
//! background job with progress, like the export.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mdp_core::backup;
use mdp_core::jobs::JobContext;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{documents, err, journey_settings_file, load_journey_settings, Res, Shared};

#[derive(Default)]
pub struct Backups {
    job: Mutex<Option<Job>>,
}

#[derive(Clone)]
struct Job {
    /// "backup" or "restore".
    kind: &'static str,
    ctx: JobContext,
    result: Arc<Mutex<Option<Result<String, String>>>>,
}

#[derive(Serialize)]
pub struct BackupInfo {
    /// Where a new backup goes by default, with today's file name.
    suggested: String,
    /// When this Journey was last backed up from this app, if ever.
    last: Option<String>,
}

#[tauri::command]
pub fn backup_info(app: AppHandle, state: State<Shared>) -> Res<BackupInfo> {
    let s = state.lock().map_err(err)?;
    let j = s.journey.as_ref().ok_or("No Journey is open.")?;
    let js = load_journey_settings(j.root());
    let name = backup::suggested_name(&j.name().map_err(err)?);
    let folder = js.backup_folder.map(PathBuf::from).unwrap_or_else(|| documents(&app).join("Modern Day Pioneer").join("Backups"));
    Ok(BackupInfo { suggested: folder.join(name).display().to_string(), last: js.last_backup })
}

fn busy(slot: &Option<Job>) -> bool {
    slot.as_ref().is_some_and(|j| j.result.lock().map(|r| r.is_none()).unwrap_or(false))
}

/// Back up the open Journey into `dest`, a new .zip file.
#[tauri::command]
pub fn start_backup(app: AppHandle, state: State<Shared>, backups: State<Backups>, dest: String) -> Res<()> {
    let mut slot = backups.job.lock().map_err(err)?;
    if busy(&slot) {
        return Err("A backup or restore is already running. Please wait for it to finish.".into());
    }
    let root = {
        let mut s = state.lock().map_err(err)?;
        let j = s.journey.as_mut().ok_or("No Journey is open.")?;
        j.write_copy().map_err(err)?; // a backup always carries a current copy
        j.root().to_path_buf()
    };
    // Android's save picker gives a content:// address: write the backup in
    // the app's cache, then copy it there (sprint 9).
    let picked = crate::mobile::is_picked(&dest).then(|| dest.clone());
    let mut dest = match &picked {
        Some(_) => crate::mobile::staging_file(&app, "backup", "backup.zip")?,
        None => PathBuf::from(dest),
    };
    if dest.extension().is_none() {
        dest.set_extension("zip");
    }
    if dest.exists() {
        return Err(format!("{} already exists. Choose another name.", dest.display()));
    }
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p).map_err(err)?;
    }
    let job = Job { kind: "backup", ctx: JobContext::new(), result: Arc::new(Mutex::new(None)) };
    *slot = Some(job.clone());
    drop(slot);
    crate::applog::log("backup", &format!("Backing up {} to {}", root.display(), dest.display()));
    std::thread::spawn(move || {
        let out = backup::backup(&root, &dest, &job.ctx).map(|p| p.display().to_string()).map_err(|e| match e {
            mdp_core::Error::Canceled => "canceled".to_string(),
            e => e.to_string(),
        });
        let out = match (&picked, out) {
            (Some(uri), Ok(local)) => {
                let copied = crate::mobile::copy_out(&app, Path::new(&local), uri).map(|_| uri.clone());
                if let Some(dir) = Path::new(&local).parent() {
                    let _ = std::fs::remove_dir_all(dir);
                }
                copied
            }
            (_, out) => out,
        };
        crate::applog::outcome("backup", "Backup", &out, |p| p.clone());
        if let Ok(path) = &out {
            // Remember when and where, so Settings can say "last backed up".
            let mut js = load_journey_settings(&root);
            js.last_backup = Some(chrono_now());
            if picked.is_none() {
                js.backup_folder = PathBuf::from(path).parent().map(|p| p.display().to_string());
            }
            if let Ok(text) = serde_json::to_string_pretty(&js) {
                let _ = std::fs::write(journey_settings_file(&root), text);
            }
        }
        if let Ok(mut r) = job.result.lock() {
            *r = Some(out);
        }
    });
    Ok(())
}

fn chrono_now() -> String {
    mdp_core::journey::now()
}

#[derive(Serialize)]
pub struct RestorePlan {
    /// The backed-up Journey's name and when the backup was made.
    name: String,
    created: String,
    /// A new, empty folder to restore into.
    folder: String,
    /// The backup to restore: on the phone, a copy in the app's cache.
    archive: String,
}

/// Read a backup's name and suggest a new folder for it (never over a Journey).
#[tauri::command]
// Async: on the phone it copies the picked backup in, which waits on Android.
pub async fn restore_plan(app: AppHandle, archive: String) -> Res<RestorePlan> {
    let archive = if crate::mobile::is_picked(&archive) {
        crate::mobile::local_file(&app, &archive, "backup.zip")?
    } else {
        PathBuf::from(&archive)
    };
    let (name, created) = backup::describe(&archive).map_err(err)?;
    let folder = crate::suggested(&app, &format!("{name} (restored)")).display().to_string();
    Ok(RestorePlan { name, created, folder, archive: archive.display().to_string() })
}

/// Restore `archive` into `folder` (new or empty). The screen opens it when done.
#[tauri::command]
pub fn start_restore(backups: State<Backups>, archive: String, folder: String) -> Res<()> {
    let mut slot = backups.job.lock().map_err(err)?;
    if busy(&slot) {
        return Err("A backup or restore is already running. Please wait for it to finish.".into());
    }
    let job = Job { kind: "restore", ctx: JobContext::new(), result: Arc::new(Mutex::new(None)) };
    *slot = Some(job.clone());
    drop(slot);
    crate::applog::log("restore", &format!("Restoring {archive} into {folder}"));
    std::thread::spawn(move || {
        let out = backup::restore(&PathBuf::from(&archive), &PathBuf::from(&folder), &job.ctx)
            .map(|p| p.display().to_string())
            .map_err(|e| match e {
                mdp_core::Error::Canceled => "canceled".to_string(),
                e => e.to_string(),
            });
        crate::applog::outcome("restore", "Restore", &out, |p| p.clone());
        if let Ok(mut r) = job.result.lock() {
            *r = Some(out);
        }
    });
    Ok(())
}

#[derive(Serialize)]
pub struct BackupStatus {
    kind: String,
    running: bool,
    done: u64,
    total: u64,
    message: String,
    /// The backup file or restored folder, when finished.
    path: Option<String>,
    error: Option<String>,
    canceled: bool,
}

#[tauri::command]
pub fn backup_status(backups: State<Backups>) -> Res<Option<BackupStatus>> {
    let slot = backups.job.lock().map_err(err)?;
    let Some(job) = slot.as_ref() else { return Ok(None) };
    let p = job.ctx.progress();
    let result = job.result.lock().map_err(err)?.clone();
    let canceled = matches!(&result, Some(Err(e)) if e == "canceled");
    Ok(Some(BackupStatus {
        kind: job.kind.into(),
        running: result.is_none(),
        done: p.done,
        total: p.total,
        message: p.message,
        path: result.as_ref().and_then(|r| r.as_ref().ok().cloned()),
        error: result.and_then(|r| r.err()).filter(|e| e != "canceled"),
        canceled,
    }))
}

#[tauri::command]
pub fn cancel_backup(backups: State<Backups>) -> Res<()> {
    if let Some(job) = backups.job.lock().map_err(err)?.as_ref() {
        job.ctx.cancel();
    }
    Ok(())
}

/// Forget a finished job once the screen has shown it.
#[tauri::command]
pub fn clear_backup(backups: State<Backups>) -> Res<()> {
    let mut slot = backups.job.lock().map_err(err)?;
    if !busy(&slot) {
        *slot = None;
    }
    Ok(())
}

/// Show a finished backup file in File Explorer.
#[tauri::command]
pub fn show_backup(app: AppHandle, path: String) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().reveal_item_in_dir(PathBuf::from(path)).map_err(err)
}
