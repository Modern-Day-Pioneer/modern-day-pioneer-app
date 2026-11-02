//! The app's log (Jared, 2026-10-19): one plain text file a day in the app's
//! own data folder, `%LOCALAPPDATA%\org.moderndaypioneer.app\logs`, recording
//! each import, backup, restore and export and every error the screens show,
//! so a failure on a tester's PC can be traced afterwards. It never leaves the
//! PC; Settings has a button that opens the folder.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::{err, Res};

/// How many days of logs are kept.
const KEEP: usize = 14;

static DIR: OnceLock<PathBuf> = OnceLock::new();
static WRITE: Mutex<()> = Mutex::new(());

/// Called once at start: finds the folder, drops old files, notes the version.
pub fn init(app: &AppHandle, version: &str) {
    let Ok(dir) = app.path().app_local_data_dir().map(|d| d.join("logs")) else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mut old: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "log")).collect())
        .unwrap_or_default();
    old.sort();
    while old.len() > KEEP {
        let _ = std::fs::remove_file(old.remove(0));
    }
    let _ = DIR.set(dir);
    log("app", &format!("Started Modern Day Pioneer {version} on {} {}", std::env::consts::OS, std::env::consts::ARCH));
}

/// One line: time, area, text. Failing to log never stops the app.
pub fn log(area: &str, text: &str) {
    let Some(dir) = DIR.get() else { return };
    let _guard = WRITE.lock();
    let now = chrono::Local::now();
    let path = dir.join(format!("mdp-{}.log", now.format("%Y-%m-%d")));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} [{area}] {}", now.format("%H:%M:%S%.3f"), text.replace('\n', " | "));
    }
}

/// Logs how a background job ended.
pub fn outcome<T>(area: &str, what: &str, out: &Result<T, String>, ok: impl FnOnce(&T) -> String) {
    match out {
        Ok(v) => log(area, &format!("{what} finished: {}", ok(v))),
        Err(e) if e == "canceled" => log(area, &format!("{what} canceled")),
        Err(e) => log(area, &format!("ERROR {what} failed: {e}")),
    }
}

/// The screens report the errors they show.
#[tauri::command]
pub fn log_event(area: String, text: String) {
    log(&area, &text);
}

#[tauri::command]
pub fn open_log_folder(app: AppHandle) -> Res<()> {
    let dir = DIR.get().ok_or("The log folder could not be made.")?;
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(err)
}
