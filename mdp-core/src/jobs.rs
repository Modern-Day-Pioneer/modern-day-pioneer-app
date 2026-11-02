//! Background jobs.
//!
//! Long work (backup, import, transcription, export) runs on its own thread
//! with progress and cancel. A job writes into a [`Staging`] folder or file
//! and only moves it into place when it finishes, so canceling or failing
//! leaves nothing half-written. One failed item is skipped and reported.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::{Error, Result};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    /// What the job is doing now, in words a person can read.
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skipped {
    pub item: String,
    pub reason: String,
}

#[derive(Default)]
struct Shared {
    progress: Progress,
    skipped: Vec<Skipped>,
}

/// Given to the job's code: report progress, skip items, and check for cancel.
#[derive(Clone)]
pub struct JobContext {
    cancel: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
}

impl JobContext {
    pub fn new() -> JobContext {
        JobContext { cancel: Arc::new(AtomicBool::new(false)), shared: Arc::default() }
    }

    pub fn is_canceled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Returns `Err(Error::Canceled)` once cancel has been asked for.
    pub fn check(&self) -> Result<()> {
        if self.is_canceled() {
            Err(Error::Canceled)
        } else {
            Ok(())
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn set_total(&self, total: u64) {
        self.shared.lock().unwrap().progress.total = total;
    }

    pub fn advance(&self, message: impl Into<String>) {
        let mut s = self.shared.lock().unwrap();
        s.progress.done += 1;
        s.progress.message = message.into();
    }

    pub fn message(&self, message: impl Into<String>) {
        self.shared.lock().unwrap().progress.message = message.into();
    }

    /// Record one item that failed and carry on with the rest.
    pub fn skip(&self, item: impl Into<String>, reason: impl Into<String>) {
        let mut s = self.shared.lock().unwrap();
        s.skipped.push(Skipped { item: item.into(), reason: reason.into() });
        s.progress.done += 1;
    }

    pub fn progress(&self) -> Progress {
        self.shared.lock().unwrap().progress.clone()
    }

    pub fn skipped(&self) -> Vec<Skipped> {
        self.shared.lock().unwrap().skipped.clone()
    }
}

impl Default for JobContext {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum Outcome<T> {
    Finished { value: T, skipped: Vec<Skipped> },
    Canceled,
    Failed(Error),
}

pub struct JobHandle<T> {
    ctx: JobContext,
    thread: JoinHandle<Result<T>>,
}

impl<T> JobHandle<T> {
    pub fn progress(&self) -> Progress {
        self.ctx.progress()
    }
    pub fn cancel(&self) {
        self.ctx.cancel()
    }
    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }
    /// Wait for the job to end.
    pub fn wait(self) -> Outcome<T> {
        match self.thread.join() {
            Ok(Ok(value)) => Outcome::Finished { value, skipped: self.ctx.skipped() },
            Ok(Err(Error::Canceled)) => Outcome::Canceled,
            Ok(Err(e)) => Outcome::Failed(e),
            Err(_) => Outcome::Failed(Error::Invalid("The job stopped unexpectedly.".into())),
        }
    }
}

/// Start a job on its own thread.
pub fn spawn<T, F>(f: F) -> JobHandle<T>
where
    T: Send + 'static,
    F: FnOnce(&JobContext) -> Result<T> + Send + 'static,
{
    let ctx = JobContext::new();
    let job_ctx = ctx.clone();
    let thread = std::thread::spawn(move || f(&job_ctx));
    JobHandle { ctx, thread }
}

/// A file or folder being written by a job. It is written under a temporary
/// name beside its final place, moved there by [`Staging::commit`], and
/// removed if the job ends any other way.
pub struct Staging {
    temp: PathBuf,
    dest: PathBuf,
    /// The destination is an existing empty folder: the work is moved into
    /// it, not renamed onto it (an empty folder the person just made is often
    /// still open in File Explorer or the folder picker, so it can't be removed).
    into_existing: bool,
    done: bool,
}

impl Staging {
    pub fn new(dest: &Path) -> Result<Staging> {
        if dest.exists() {
            return Err(Error::Invalid(format!("{} already exists.", dest.display())));
        }
        Staging::make(dest, false)
    }

    /// As `new`, but `dest` may also be an existing empty folder.
    pub fn new_or_empty(dest: &Path) -> Result<Staging> {
        if dest.is_dir() && std::fs::read_dir(dest)?.next().is_none() {
            return Staging::make(dest, true);
        }
        Staging::new(dest)
    }

    fn make(dest: &Path, into_existing: bool) -> Result<Staging> {
        let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let temp = dest.with_file_name(format!("{name}.partial-{}", crate::ids::new_id()));
        if let Some(p) = temp.parent() {
            std::fs::create_dir_all(p)?;
        }
        Ok(Staging { temp, dest: dest.to_path_buf(), into_existing, done: false })
    }

    /// Where the job writes.
    pub fn path(&self) -> &Path {
        &self.temp
    }

    pub fn commit(mut self) -> Result<PathBuf> {
        if self.into_existing {
            // All or nothing: if one entry can't be moved, the ones already
            // moved go back, so the folder is left as it was.
            let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
            for e in std::fs::read_dir(&self.temp)? {
                let e = e?;
                let to = self.dest.join(e.file_name());
                if let Err(err) = patiently(|| std::fs::rename(e.path(), &to)) {
                    for (from, to) in moved.iter().rev() {
                        let _ = patiently(|| std::fs::rename(to, from));
                    }
                    return Err(err.into());
                }
                moved.push((e.path(), to));
            }
            patiently(|| std::fs::remove_dir(&self.temp))?;
        } else {
            patiently(|| std::fs::rename(&self.temp, &self.dest))?;
        }
        self.done = true;
        Ok(self.dest.clone())
    }
}

/// Retry a file operation for a few seconds: on Windows, antivirus, search
/// indexing, OneDrive and File Explorer briefly hold files that were just
/// written, and renames then fail with "Access is denied" (os error 5).
pub(crate) fn patiently<T>(mut f: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut wait = 50;
    for _ in 0..12 {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(e),
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(wait));
                wait = (wait * 2).min(2000);
            }
        }
    }
    f()
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.done {
            if self.temp.is_dir() {
                // Restored copies are read-only, which would stop the removal on Windows.
                crate::import::clear_readonly_tree(&self.temp);
                let _ = std::fs::remove_dir_all(&self.temp);
            } else {
                let _ = std::fs::remove_file(&self.temp);
            }
        }
    }
}
