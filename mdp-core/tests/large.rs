//! The large test project, scaled down. Run on purpose, not with every test:
//!
//!     MDP_LARGE=2000 cargo test --release --test large -- --ignored --nocapture
//!
//! MDP_LARGE is the number of pictures (default 2,000; the full project is
//! 20,000). People, memories and stories scale with it (one person per 40
//! pictures, one memory per 10, one story per 400). It prints how long each
//! step took and checks the screens' reads stay near a second.

use std::path::PathBuf;
use std::time::Instant;

use mdp_core::export::Privacy;
use mdp_core::jobs::JobContext;
use mdp_core::Journey;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
#[ignore]
fn a_large_journey_stays_quick() {
    let n: usize = std::env::var("MDP_LARGE").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
    let dir = tempfile::tempdir().unwrap();
    // Distinct pictures: the same JPEG with a different comment after its end,
    // so each has its own checksum and none is taken for a duplicate.
    let src = std::fs::read(fixtures().join("walter-camera.jpg")).unwrap();
    let pics = dir.path().join("pictures");
    std::fs::create_dir_all(&pics).unwrap();
    for i in 0..n {
        let mut b = src.clone();
        b.extend_from_slice(format!("picture {i}").as_bytes());
        std::fs::write(pics.join(format!("IMG_{i:05}.jpg")), b).unwrap();
    }
    let mut j = Journey::create(&dir.path().join("Large"), "The Large Family").unwrap();
    let t = Instant::now();
    let (r, _) = j.import(&[pics.clone()], &JobContext::new()).unwrap();
    let import = t.elapsed();
    assert_eq!(r.imported.len(), n);

    let t = Instant::now();
    let people: Vec<String> = (0..n.div_ceil(40)).map(|i| j.add_individual("Person", &format!("Person {i}")).unwrap()).collect();
    let stories: Vec<String> = (0..n.div_ceil(400)).map(|i| j.add_story(&format!("Story {i}")).unwrap()).collect();
    for (k, chunk) in r.imported.chunks(10).enumerate() {
        let m = j.new_memory_with(&format!("Memory {k}"), chunk, Some(&stories[k % stories.len()])).unwrap();
        j.link_to_memory(&m, &people[k % people.len()], "in the picture", None, false).unwrap();
    }
    let build = t.elapsed();

    let time = |what: &str, f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        let d = t.elapsed();
        println!("{what}: {:.2} s", d.as_secs_f64());
        d
    };
    println!("{n} pictures, {} people, {} memories, {} stories", people.len(), n.div_ceil(10), stories.len());
    println!("import: {:.1} s; adding people, memories and stories: {:.1} s", import.as_secs_f64(), build.as_secs_f64());
    let home = time("home", &mut || {
        j.home().unwrap();
    });
    let search = time("search", &mut || {
        j.search("memory 12", 20).unwrap();
    });
    let lists = time("stories and memories lists", &mut || {
        j.stories().unwrap();
        j.memories().unwrap();
    });
    let story = time("one story", &mut || {
        j.story_view(&stories[0]).unwrap();
    });
    // One edit, with the plain-text copy written after every action (as the
    // core tests do) and deferred to a pause (as the app does; edit speed plan).
    let edit_now = time("one edit, copy written now", &mut || {
        let mut r = mdp_core::Row::new();
        r.insert("description".into(), "Edited".into());
        j.update_individual(&people[0], &r).unwrap();
    });
    j.defer_copy(true);
    let edit = time("one edit, copy written after a pause", &mut || {
        let mut r = mdp_core::Row::new();
        r.insert("description".into(), "Edited again".into());
        j.update_individual(&people[0], &r).unwrap();
    });
    time("writing the copy", &mut || {
        j.write_copy().unwrap();
    });
    j.defer_copy(false);
    time("preview", &mut || {
        j.preview(Privacy::default()).unwrap();
    });
    let out = dir.path().join("Large export");
    time("export", &mut || {
        let r = j.export(&out, Privacy::default(), &JobContext::new()).unwrap();
        assert_eq!(r.files, n);
    });
    let backup = dir.path().join("large.zip");
    time("backup", &mut || {
        mdp_core::backup::backup(j.root(), &backup, &JobContext::new()).unwrap();
    });
    // The screens read these; about a second or less, with room for slow machines.
    for (what, d) in [("home", home), ("search", search), ("lists", lists), ("story", story), ("edit", edit), ("edit with copy", edit_now)] {
        assert!(d.as_secs_f64() < 2.0, "{what} took {:.2} s", d.as_secs_f64());
    }
}
