//! Modern Day Pioneer core: the Journey, its catalog and its project folder.
//! No screens and no Tauri here, so every rule can be checked by tests.

pub mod backup;
pub mod bulk;
pub mod cache;
pub mod convert;
pub mod dates;
pub mod dng;
pub mod documents;
pub mod export;
pub mod files;
pub mod folder;
pub mod history;
pub mod import;
pub mod ids;
pub mod jobs;
pub mod journey;
pub mod jsonl;
pub mod memories;
pub mod people;
pub mod pictures;
pub mod places;
pub mod rebuild;
pub mod sample;
pub mod snapshot;
pub mod sorting;
pub mod stories;
pub mod summary;
pub mod takeout;
pub mod takeout_zip;
pub mod towns;
pub mod voice;

pub use history::{id_key, Action, HistoryEntry, Row};
pub use journey::{Journey, OpenMode};

/// Catalog format version ("Format version and upgrade rule").
pub const FORMAT_MAJOR: u32 = 1;
pub const FORMAT_MINOR: u32 = 5;

/// `PRAGMA user_version` holds the format version as major * 100 + minor (1.0 -> 100).
pub fn format_user_version(major: u32, minor: u32) -> i64 {
    (major * 100 + minor) as i64
}

pub fn format_version_text() -> String {
    format!("{FORMAT_MAJOR}.{FORMAT_MINOR}")
}

/// Stop before bringing in a backup or export written by a newer app with a
/// higher major format version, so nothing is half brought in.
/// A newer minor version only adds fields, so it is let through.
pub fn check_not_newer(version: Option<&str>, what: &str) -> Result<()> {
    let major = version.and_then(|v| v.split('.').next()).and_then(|m| m.trim().parse::<u32>().ok());
    match major {
        Some(m) if m > FORMAT_MAJOR => Err(Error::Invalid(format!(
            "This {what} was made by a newer version of Modern Day Pioneer (format {}). Update the app to open it.",
            version.unwrap_or_default()
        ))),
        _ => Ok(()),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("file: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("Canceled.")]
    Canceled,
}

pub type Result<T> = std::result::Result<T, Error>;
