//! Upgrade safety: every saved release sample in
//! `tests/fixtures/releases/<format>/` must still restore, open, upgrade,
//! export and come back in with this build, and no field may disappear.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use mdp_core::export::Privacy;
use mdp_core::jobs::JobContext;
use mdp_core::{backup, rebuild, Journey, OpenMode};
use rusqlite::Connection;

const TABLES: [&str; 10] = ["individual", "family", "place", "event", "story", "memory", "content", "content_file", "memory_content", "memory_individual"];

fn samples() -> Vec<(String, PathBuf)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/releases");
    let mut v: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .map(|p| (p.file_name().unwrap().to_string_lossy().to_string(), p))
        .collect();
    v.sort();
    v
}

fn unzip(archive: &Path, dest: &Path) {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive).unwrap()).unwrap();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let out = dest.join(f.enclosed_name().unwrap());
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let mut bytes = vec![];
        f.read_to_end(&mut bytes).unwrap();
        std::fs::write(out, bytes).unwrap();
    }
}

fn counts(c: &Connection) -> HashMap<&'static str, i64> {
    TABLES.iter().map(|t| (*t, c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).unwrap())).collect()
}

/// Every "table.column" in a catalog.
fn columns(c: &Connection) -> BTreeSet<String> {
    let mut st = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'").unwrap();
    let tables: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    let mut out = BTreeSet::new();
    for t in tables {
        let mut st = c.prepare(&format!("PRAGMA table_info({t})")).unwrap();
        for col in st.query_map([], |r| r.get::<_, String>(1)).unwrap() {
            out.insert(format!("{t}.{}", col.unwrap()));
        }
    }
    out
}

fn user_version(c: &Connection) -> i64 {
    c.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap()
}

#[test]
fn a_sample_is_saved_for_the_current_format() {
    let have: Vec<String> = samples().into_iter().map(|(v, _)| v).collect();
    assert!(have.contains(&mdp_core::format_version_text()),
        "No release sample for format {}. Run: cargo run --example release_sample -- tests/fixtures/releases/{}",
        mdp_core::format_version_text(), mdp_core::format_version_text());
}

#[test]
fn every_release_sample_still_opens_and_comes_back_in() {
    let t = tempfile::tempdir().unwrap();
    let fresh = Journey::create(&t.path().join("fresh"), "Fresh").unwrap();
    let fresh_columns = columns(fresh.conn());
    let current = mdp_core::format_user_version(mdp_core::FORMAT_MAJOR, mdp_core::FORMAT_MINOR);
    for (version, dir) in samples() {
        let w = t.path().join(&version);
        // The backup restores, and the old catalog's counts and fields are noted before opening.
        let root = backup::restore(&dir.join("journey.mdp-backup.zip"), &w.join("restored"), &JobContext::new())
            .unwrap_or_else(|e| panic!("{version}: restore failed: {e}"));
        let (before, old_columns, old_version) = {
            let c = Connection::open(root.join(mdp_core::journey::CATALOG_FILE)).unwrap();
            (counts(&c), columns(&c), user_version(&c))
        };
        assert!(before["individual"] > 0 && before["memory"] > 0, "{version}: the sample is empty");

        // It opens with this build, is upgraded, and nothing is lost.
        let mut j = Journey::open(&root).unwrap_or_else(|e| panic!("{version}: open failed: {e}"));
        assert_eq!(j.mode(), OpenMode::ReadWrite, "{version}");
        assert_eq!(user_version(j.conn()), current, "{version}: not upgraded");
        assert_eq!(counts(j.conn()), before, "{version}: counts changed on upgrade");
        if old_version < current {
            let snaps = std::fs::read_dir(root.join(mdp_core::folder::SNAPSHOT_DIR)).unwrap()
                .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("catalog-before-format"))
                .count();
            assert!(snaps > 0, "{version}: no snapshot was saved before the upgrade");
        }

        // Add only: no field of an older format may disappear, in an upgraded Journey or a new one.
        let upgraded = columns(j.conn());
        let lost: Vec<_> = old_columns.difference(&upgraded).collect();
        assert!(lost.is_empty(), "{version}: fields lost on upgrade: {lost:?}. Renaming or removing a field needs a new major version and a decision.");
        let missing: Vec<_> = old_columns.difference(&fresh_columns).collect();
        assert!(missing.is_empty(), "{version}: fields a new Journey no longer has: {missing:?}");

        // It can still be changed and exported.
        j.add_individual("person", "Added after the update").unwrap();
        j.export(&w.join("export-again"), Privacy::from_settings("include", "exact"), &JobContext::new())
            .unwrap_or_else(|e| panic!("{version}: export after upgrade failed: {e}"));

        // The export that version made comes back in as a full rebuild.
        let old_export = w.join("old-export");
        unzip(&dir.join("export.zip"), &old_export);
        let mut r = Journey::create(&w.join("rebuilt"), "Rebuilt").unwrap();
        let look = rebuild::look(&r, &old_export).unwrap_or_else(|e| panic!("{version}: export not recognized: {e}"));
        let eb = rebuild::prepare(r.root(), &old_export, &HashSet::new(), &JobContext::new()).unwrap();
        let report = r.finish_export(eb, &HashMap::new()).unwrap();
        assert!(report.files > 0, "{version}: {report:?}");
        let after = counts(r.conn());
        for k in ["individual", "family", "place", "event"] {
            assert_eq!(after[k], before[k], "{version}: {k} from the old export");
        }
        assert_eq!(after["memory"] as usize, look.memories, "{version}");
        assert_eq!(after["story"] as usize, look.stories, "{version}");
    }
}

#[test]
fn files_from_a_newer_app_are_refused_or_read_only() {
    let t = tempfile::tempdir().unwrap();
    let (_, dir) = samples().pop().unwrap();

    // A backup with a newer major format stops before anything is written.
    let future = t.path().join("future.mdp-backup.zip");
    {
        let mut src = zip::ZipArchive::new(std::fs::File::open(dir.join("journey.mdp-backup.zip")).unwrap()).unwrap();
        let mut out = zip::ZipWriter::new(std::fs::File::create(&future).unwrap());
        for i in 0..src.len() {
            let mut f = src.by_index(i).unwrap();
            let name = f.name().unwrap().to_string();
            let mut bytes = vec![];
            f.read_to_end(&mut bytes).unwrap();
            if name == backup::MANIFEST {
                let mut m: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                m["formatVersion"] = "2.0".into();
                bytes = serde_json::to_vec(&m).unwrap();
            }
            out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut out, &bytes).unwrap();
        }
        out.finish().unwrap();
    }
    let dest = t.path().join("future-restored");
    let err = backup::restore(&future, &dest, &JobContext::new()).unwrap_err().to_string();
    assert!(err.contains("newer version"), "{err}");
    assert!(!dest.join(mdp_core::journey::CATALOG_FILE).exists());

    // An export with a newer major format is not brought in.
    let export = t.path().join("future-export");
    unzip(&dir.join("export.zip"), &export);
    let p = export.join(rebuild::EXPORT_CATALOG);
    let mut cat: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    cat["formatVersion"] = "2.0".into();
    std::fs::write(&p, serde_json::to_string(&cat).unwrap()).unwrap();
    let j = Journey::create(&t.path().join("j"), "J").unwrap();
    let err = rebuild::look(&j, &export).err().expect("a newer export is refused").to_string();
    assert!(err.contains("newer version"), "{err}");

    // A newer minor format only adds fields, so it is still let through.
    assert!(mdp_core::check_not_newer(Some(&format!("{}.99", mdp_core::FORMAT_MAJOR)), "backup").is_ok());

    // A Journey folder from a newer app opens read-only.
    let root = backup::restore(&dir.join("journey.mdp-backup.zip"), &t.path().join("newer"), &JobContext::new()).unwrap();
    Connection::open(root.join(mdp_core::journey::CATALOG_FILE)).unwrap().pragma_update(None, "user_version", 200).unwrap();
    let j = Journey::open(&root).unwrap();
    assert_eq!(j.mode(), OpenMode::ReadOnly);
}
