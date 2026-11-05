//! A Journey or an export from someone else can't point the app at files
//! outside its own folder.

use std::collections::HashSet;
use std::path::Path;

use mdp_core::export::Privacy;
use mdp_core::folder::{absolute, is_inside, is_working_path};
use mdp_core::jobs::JobContext;
use mdp_core::{rebuild, Journey};
use serde_json::Value;

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

#[test]
fn working_paths_always_stay_inside_the_journey() {
    let root = Path::new("/journeys/J");
    for bad in ["../outside.txt", "managed-content/../../outside.txt", "/etc/passwd", "C:/Users/x/secret.docx", "managed-content/..\\..\\x", "C:\\x", "./../x"] {
        let p = absolute(root, bad);
        assert!(p.starts_with(root), "{bad} -> {}", p.display());
        assert!(!p.components().any(|c| matches!(c, std::path::Component::ParentDir)), "{bad} -> {}", p.display());
        assert!(!is_working_path(bad), "{bad}");
    }
    assert!(!is_inside("../x") && !is_inside("/x") && !is_inside("a//b") && !is_inside("D:/x"));
    assert!(is_inside("photos/1962/x.jpg"));
    assert!(is_working_path("managed-content/photos/1962/k7f3m2qa.jpg"));
    assert_eq!(absolute(root, "managed-content/photos/1962/k7f3m2qa.jpg"), root.join("managed-content").join("photos").join("1962").join("k7f3m2qa.jpg"));
}

#[test]
fn a_catalog_pointing_outside_is_refused_and_nothing_moves() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "not part of any Journey").unwrap();
    let folder = dir.path().join("J");
    {
        let mut j = Journey::create(&folder, "Rowans").unwrap();
        j.import(&[fixtures().join("walter-camera.jpg")], &JobContext::new()).unwrap();
        // A hand-made catalog: the file row now leads out of the Journey.
        j.conn().execute("UPDATE content_file SET path = '../outside.txt'", []).unwrap();
    }
    let err = Journey::open(&folder).err().expect("the Journey should not open");
    assert!(err.to_string().contains("outside the Journey folder"), "{err}");
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "not part of any Journey");
}

fn set_urls(v: &mut Value, to: &str) {
    match v {
        Value::Object(m) => {
            if m.contains_key("contentUrl") {
                m.insert("contentUrl".into(), to.into());
                m.remove("sha256");
            }
            m.values_mut().for_each(|x| set_urls(x, to));
        }
        Value::Array(a) => a.iter_mut().for_each(|x| set_urls(x, to)),
        _ => {}
    }
}

#[test]
fn an_export_pointing_outside_brings_nothing_in() {
    let t = tempfile::tempdir().unwrap();
    let src = mdp_core::sample::create(&t.path().join("sample")).unwrap();
    let export = t.path().join("export");
    src.export(&export, Privacy { living_dates: true, exact_places: true, coffee_table: false, scrapbook: false, checksums: false }, &JobContext::new()).unwrap();
    std::fs::write(t.path().join("secret.txt"), "private").unwrap();
    let cat = export.join(rebuild::EXPORT_CATALOG);
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&cat).unwrap()).unwrap();
    set_urls(&mut v, "../secret.txt");
    std::fs::write(&cat, serde_json::to_string(&v).unwrap()).unwrap();

    let j = Journey::create(&t.path().join("rebuilt"), "Rebuilt").unwrap();
    let ctx = JobContext::new();
    let eb = rebuild::prepare(j.root(), &export, &HashSet::new(), &ctx).unwrap();
    assert!(eb.files.is_empty(), "{} files came in", eb.files.len());
    assert!(ctx.skipped().iter().any(|s| s.reason.contains("outside its own folder")));
}
