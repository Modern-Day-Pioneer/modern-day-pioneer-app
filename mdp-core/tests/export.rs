//! Sprint 6: the export and its preview, and the export checks that can be
//! run by reading files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mdp_core::export::{self, Privacy, ABOUT_FILE, FRONT};
use mdp_core::jobs::JobContext;
use mdp_core::{Journey, Row};
use serde_json::{json, Value};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn sample_media() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("sample-media")
}

fn row(v: Value) -> Row {
    v.as_object().cloned().unwrap()
}

struct Built {
    j: Journey,
    camera: String,
    note: String,
    wav: String,
    mp3: String,
    video: String,
    placeholder: String,
    ellen: String,
    house: String,
    ogden: String,
    suggested: String,
    empty_story: String,
    waiting: String,
}

/// The test family: the Rowans, with a living daughter, an exact family
/// home, a suggested relationship, a placeholder memory, an empty story,
/// and one item still waiting.
fn build(dir: &Path) -> Built {
    let mut j = Journey::create(&dir.join("Rowans"), "The Rowan Family").unwrap();
    j.update_journey(&row(json!({"introduction": "Three generations around one kitchen table."}))).unwrap();
    let files = [
        fixtures().join("walter-camera.jpg"),
        fixtures().join("scrapbook-note.txt"),
        fixtures().join("hello.wav"),
        sample_media().join("kennedy-inaugural-1961-excerpt.mp3"),
        fixtures().join("phone-video.mp4"),
        sample_media().join("cup-of-coffee.jpg"),
    ];
    let (r, _) = j.import(&files, &JobContext::new()).unwrap();
    assert_eq!(r.imported.len(), files.len(), "{:?}", r);
    let [camera, note, wav, mp3, video, waiting] = <[String; 6]>::try_from(r.imported.clone()).unwrap();

    let walter = j.add_individual("Person", "Walter Rowan").unwrap();
    j.update_individual(&walter, &row(json!({"gender": "male"}))).unwrap();
    let june = j.add_individual("Person", "June Rowan").unwrap();
    j.update_individual(&june, &row(json!({"gender": "female"}))).unwrap();
    let ellen = j.add_individual("Person", "Ellen Rowan").unwrap();
    let pepper = j.add_individual("Pet", "Pepper").unwrap();
    // Dates set straight in the catalog, so the test does not depend on the date words.
    j.act(|a| {
        for (id, b, d) in [(&walter, Some("1928"), Some("1999-11-02")), (&june, Some("1931-04-12"), Some("2014-01-20")), (&ellen, Some("1958-03-09"), None)] {
            a.update("individual", mdp_core::id_key(id), "birth_date_edtf", json!(b))?;
            a.update("individual", mdp_core::id_key(id), "birth_date", json!(b))?;
            a.update("individual", mdp_core::id_key(id), "death_date_edtf", json!(d))?;
            a.update("individual", mdp_core::id_key(id), "death_date", json!(d))?;
        }
        Ok(())
    })
    .unwrap();
    let fam = j.add_family("The Rowans").unwrap();
    for m in [&walter, &june, &ellen, &pepper] {
        j.add_family_member(&fam, m).unwrap();
    }
    j.add_relationship(&june, "parent", &ellen, Some("birth"), None, None).unwrap();
    let suggested = j
        .act(|a| a.create("individual_relationship", row(json!({"from_id": walter, "to_id": june, "kind": "sibling", "status": "suggested"}))))
        .unwrap();

    let house = j.add_place("The Rowan house").unwrap();
    j.update_place(&house, &row(json!({"address": "1214 Monroe Boulevard, Ogden, Utah", "coordinates": "41.2244, -111.9601", "exact": true}))).unwrap();
    let ogden = j.add_place("Ogden, Utah").unwrap();
    j.update_place(&ogden, &row(json!({"coordinates": "41.223, -111.974"}))).unwrap();
    j.add_event("birth", Some("Birth of Ellen Rowan"), &[ellen.clone()], "March 9, 1958", Some(&ogden)).unwrap();
    j.add_event("wedding", Some("Wedding of Walter and June"), &[walter.clone(), june.clone()], "June 2, 1956", Some(&ogden)).unwrap();

    let cam_mem = j.add_memory("Saturday mornings on the lawn", None).unwrap();
    j.add_to_memory(&cam_mem, &[camera.clone(), note.clone()]).unwrap();
    j.update_memory(&cam_mem, &row(json!({"date": "June 2, 1962", "caption": "Nobody was allowed to walk across the frame.", "place_id": house}))).unwrap();
    j.link_to_memory(&cam_mem, &walter, "taken by", None, false).unwrap();
    j.link_to_memory(&cam_mem, &pepper, "in the picture", None, false).unwrap();

    let voice = j.add_memory("Ellen says hello", None).unwrap();
    j.add_to_memory(&voice, &[wav.clone(), mp3.clone()]).unwrap();
    j.update_memory(&voice, &row(json!({"date": "about 1970"}))).unwrap();
    j.link_to_memory(&voice, &ellen, "speaking", None, false).unwrap();
    j.type_transcript(&mp3, "Ask not what your country can do for you.").unwrap();

    let film = j.add_memory("A film nobody can place", None).unwrap();
    j.add_to_memory(&film, &[video.clone()]).unwrap();

    let placeholder = j.add_memory("Ellen tells the Sunday story", None).unwrap();
    j.link_to_memory(&placeholder, &ellen, "speaking", None, false).unwrap();

    let story = j.add_story("Walter's Saturday camera").unwrap();
    j.update_story(&story, &row(json!({"focus_id": walter, "introduction": "Saturdays belonged to Walter's camera."}))).unwrap();
    j.add_memories_to_story(&story, &[cam_mem.clone()]).unwrap();
    let empty_story = j.add_story("Sunday mornings").unwrap();
    j.add_memories_to_story(&empty_story, &[placeholder.clone()]).unwrap();
    j.set_featured(&story, true).unwrap();
    Built { j, camera, note, wav, mp3, video, placeholder, ellen, house, ogden, suggested, empty_story, waiting }
}

fn find<'a>(cat: &'a Value, id: &str) -> Option<&'a Value> {
    cat["@graph"].as_array().unwrap().iter().find(|o| o["@id"] == format!("mdpid:{id}"))
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}

/// Every `href` and `src` in a page, without its `#part`.
fn links(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attr in ["href=\"", "src=\"", "poster=\""] {
        let mut rest = html;
        while let Some(i) = rest.find(attr) {
            rest = &rest[i + attr.len()..];
            let end = rest.find('"').unwrap();
            let l = rest[..end].split('#').next().unwrap().replace("%20", " ");
            if !l.is_empty() {
                out.push(l);
            }
            rest = &rest[end..];
        }
    }
    out
}

fn sha(p: &Path) -> String {
    mdp_core::import::checksum(p).unwrap().0
}

#[test]
fn the_export_has_the_page_set_catalog_and_guide() {
    let dir = tempfile::tempdir().unwrap();
    let b = build(dir.path());
    // Set MDP_TEST_EXPORT_OUT to keep this export for a look.
    let dest = std::env::var("MDP_TEST_EXPORT_OUT").map(PathBuf::from).unwrap_or_else(|_| dir.path().join("out").join("The Rowan Family export"));
    let _ = std::fs::remove_dir_all(&dest);
    let report = b.j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
    assert!(report.pages >= 8, "{report:?}");
    for f in [FRONT, ABOUT_FILE, "catalog.json", "catalog.js", "context.jsonld", "pages/style.css", "pages/people.html", "pages/places.html",
        "pages/timeline.html", "pages/assorted-memories.html", "pages/about.html"]
    {
        assert!(dest.join(f).is_file(), "missing {f}");
    }
    let cat: Value = serde_json::from_str(&std::fs::read_to_string(dest.join("catalog.json")).unwrap()).unwrap();
    assert_eq!(cat["@context"], "context.jsonld");
    assert_eq!(cat["formatVersion"], mdp_core::format_version_text());
    let js = std::fs::read_to_string(dest.join("catalog.js")).unwrap();
    assert!(js.starts_with("window.CATALOG = {"));

    // only what belongs.
    assert!(find(&cat, &b.placeholder).is_none(), "placeholder memory left out");
    assert!(find(&cat, &b.empty_story).is_none(), "a story with no finished memory is left out");
    assert!(find(&cat, &b.waiting).is_none(), "waiting content left out");
    assert!(find(&cat, &b.suggested).is_none(), "suggested relationship left out");
    for c in [&b.camera, &b.note, &b.wav, &b.mp3, &b.video] {
        assert!(find(&cat, c).is_some(), "content in a memory is exported");
    }
    let transcript = cat["@graph"].as_array().unwrap().iter().find(|o| o["transcriptOf"] == format!("mdpid:{}", b.mp3)).expect("transcript exported");
    assert!(transcript["encoding"][0]["contentUrl"].as_str().unwrap().starts_with("audio/"), "a transcript sits beside its recording");

    // No dangling links: every mdpid: points at an item in the catalog.
    let ids: Vec<String> = cat["@graph"].as_array().unwrap().iter().map(|o| o["@id"].as_str().unwrap().to_string()).collect();
    fn refs(v: &Value, out: &mut Vec<String>, top: bool) {
        match v {
            Value::String(s) if s.starts_with("mdpid:") && !top => out.push(s.clone()),
            Value::Array(a) => a.iter().for_each(|x| refs(x, out, false)),
            Value::Object(m) => m.iter().filter(|(k, _)| *k != "@id").for_each(|(_, x)| refs(x, out, false)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    refs(&cat["@graph"], &mut all, false);
    refs(&cat["featured"], &mut all, false);
    for r in all {
        assert!(ids.contains(&r), "dangling link {r}");
    }

    // dated, safe names in folders by type and year; short paths.
    let mut media = 0;
    for p in walk(&dest) {
        let rel = p.strip_prefix(&dest).unwrap().to_string_lossy().replace('\\', "/");
        assert!(rel.len() < 160, "path too long: {rel}");
        let top = rel.split('/').next().unwrap();
        if ["photos", "audio", "video", "stories"].contains(&top) {
            media += 1;
            let parts: Vec<&str> = rel.split('/').collect();
            assert_eq!(parts.len(), 3, "{rel}");
            let name = parts[2];
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c)), "unsafe name {name}");
            let date = name.split('_').next().unwrap();
            assert!(date == "undated" || date.starts_with(parts[1]) || date.ends_with("0s"), "{rel}");
        }
    }
    assert!(media >= 7, "media files: {media}");
    let pic = &find(&cat, &b.camera).unwrap()["encoding"][0]["contentUrl"];
    assert!(pic.as_str().unwrap().starts_with("photos/1962/1962-06-02_"), "{pic}");

    // the guide is there and names the fields.
    let about = std::fs::read_to_string(dest.join(ABOUT_FILE)).unwrap();
    assert!(about.contains("HOW TO OPEN IT") && about.contains("temporalCoverage:"));
}

#[test]
fn pages_work_offline_with_no_script_and_no_broken_links() {
    let dir = tempfile::tempdir().unwrap();
    let b = build(dir.path());
    let dest = dir.path().join("export");
    b.j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
    let mut pages = 0;
    for p in walk(&dest).into_iter().filter(|p| p.extension().is_some_and(|e| e == "html")) {
        pages += 1;
        let html = std::fs::read_to_string(&p).unwrap();
        // Only the search page has a script (sprint 13); every other page reads without one.
        let search = p.file_name().is_some_and(|n| n == "search.html");
        assert_eq!(html.contains("<script"), search, "a script only on the search page: {}", p.display());
        assert!(!html.contains("http://") && !html.contains("https://"), "nothing online in {}", p.display());
        for l in links(&html) {
            let target = p.parent().unwrap().join(&l);
            assert!(target.exists(), "{} links to missing {l}", p.display());
        }
    }
    assert!(pages >= 8);
    let tl = std::fs::read_to_string(dest.join("pages/timeline.html")).unwrap();
    let wedding = tl.find("Wedding of Walter and June").unwrap();
    let lawn = tl.find("Saturday mornings on the lawn").unwrap();
    let hello = tl.find("Ellen says hello").unwrap();
    assert!(wedding < lawn && lawn < hello, "E-order: the timeline is in date order");
    assert!(!tl.contains("Birth of Ellen Rowan"), "a living person's birth has no date, so it is not on the timeline");
    assert!(tl.contains("The 1950s") && tl.contains("about 1970"));
    let assorted = std::fs::read_to_string(dest.join("pages/assorted-memories.html")).unwrap();
    assert!(assorted.contains("A film nobody can place"));
    let front = std::fs::read_to_string(dest.join(FRONT)).unwrap();
    assert!(front.contains("book face") && front.contains("Walter&#39;s Saturday camera") || front.contains("Walter's Saturday camera"));
}

#[test]
fn privacy_leaves_out_living_dates_and_exact_places_unless_included() {
    let dir = tempfile::tempdir().unwrap();
    let b = build(dir.path());
    let dest = dir.path().join("private");
    let report = b.j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
    assert_eq!(report.left_out.living_hidden, 1);
    assert_eq!(report.left_out.exact_hidden, 1);
    assert!(report.note.contains("Hidden for privacy"), "{}", report.note);
    let cat: Value = serde_json::from_str(&std::fs::read_to_string(dest.join("catalog.json")).unwrap()).unwrap();
    let ellen = find(&cat, &b.ellen).unwrap();
    assert_eq!(ellen["living"], true);
    assert!(ellen.get("birthDate").is_none() && ellen.get("birthDateEdtf").is_none());
    let birth = cat["@graph"].as_array().unwrap().iter().find(|o| o["name"] == "Birth of Ellen Rowan").unwrap();
    assert!(birth.get("startDate").is_none(), "a living person's birth event has no date");
    let house = find(&cat, &b.house).unwrap();
    assert!(house.get("address").is_none() && house.get("geo").is_none(), "the exact place keeps only its name");
    assert!(find(&cat, &b.ogden).unwrap().get("geo").is_some(), "other places keep their coordinates");
    let pages: String = walk(&dest).iter().filter(|p| p.extension().is_some_and(|e| e == "html")).map(|p| std::fs::read_to_string(p).unwrap()).collect();
    assert!(!pages.contains("1214 Monroe") && !pages.contains("1958"), "no address or living date on any page");
    // The picture's own GPS is removed from its copy; the date stays.
    let pic = dest.join(find(&cat, &b.camera).unwrap()["encoding"][0]["contentUrl"].as_str().unwrap());
    let ex = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(std::fs::File::open(&pic).unwrap())).unwrap();
    assert!(ex.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY).is_none(), "GPS removed");

    let open = dir.path().join("open");
    b.j.export(&open, Privacy { living_dates: true, exact_places: true, coffee_table: false }, &JobContext::new()).unwrap();
    let cat: Value = serde_json::from_str(&std::fs::read_to_string(open.join("catalog.json")).unwrap()).unwrap();
    assert_eq!(find(&cat, &b.ellen).unwrap()["birthDateEdtf"], "1958-03-09");
    assert!(find(&cat, &b.house).unwrap().get("geo").is_some());
    let pic = open.join(find(&cat, &b.camera).unwrap()["encoding"][0]["contentUrl"].as_str().unwrap());
    let ex = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(std::fs::File::open(&pic).unwrap())).unwrap();
    assert!(ex.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY).is_some(), "GPS kept when exact places are included");
}

#[test]
fn exported_files_carry_their_facts_and_originals_are_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let b = build(dir.path());
    let before: BTreeMap<String, String> = {
        let mut st = b.j.conn().prepare("SELECT path, sha256 FROM content_file").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    let dest = dir.path().join("export");
    b.j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
    // the Journey's own copies are untouched.
    for (path, sum) in &before {
        assert_eq!(&sha(&mdp_core::folder::absolute(b.j.root(), path)), sum, "{path} changed");
    }
    let cat: Value = serde_json::from_str(&std::fs::read_to_string(dest.join("catalog.json")).unwrap()).unwrap();
    let file_of = |id: &str, fmt: &str| -> PathBuf {
        let enc = find(&cat, id).unwrap()["encoding"].as_array().unwrap().clone();
        let e = enc.iter().find(|e| e["encodingFormat"].as_str().unwrap().starts_with(fmt)).unwrap_or_else(|| panic!("{id} has no {fmt}"));
        let p = dest.join(e["contentUrl"].as_str().unwrap());
        assert_eq!(e["sha256"], sha(&p), "the catalog lists the exported file's checksum");
        p
    };
    // Picture: XMP with the title, people and place; the pixels are unchanged.
    let pic = std::fs::read(file_of(&b.camera, "image/jpeg")).unwrap();
    let text = String::from_utf8_lossy(&pic);
    assert!(text.contains("http://ns.adobe.com/xap/1.0/") && text.contains("<dc:title>"));
    assert!(text.contains("Walter Rowan") && text.contains("Pepper") && text.contains("The Rowan house"));
    assert!(text.contains("<Iptc4xmpExt:PersonInImage><rdf:Bag><rdf:li>Pepper</rdf:li>"));
    let orig = std::fs::read(fixtures().join("walter-camera.jpg")).unwrap();
    let sos = |b: &[u8]| b.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
    assert_eq!(&pic[sos(&pic)..], &orig[sos(&orig)..], "the picture data is copied byte for byte");
    let ex = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(&pic)).unwrap();
    assert!(ex.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY).is_some());
    // Text: a heading.
    let note = std::fs::read_to_string(file_of(&b.note, "text/plain")).unwrap();
    assert!(note.lines().next().unwrap().len() > 0 && note.contains("\r\n\r\n"));
    // MP3: ID3 with the title and the transcript.
    use id3::TagLike;
    let tag = id3::Tag::read_from_path(file_of(&b.mp3, "audio/mpeg")).unwrap();
    assert!(tag.title().is_some());
    assert!(tag.lyrics().any(|l| l.text.contains("Ask not")), "the MP3 carries its transcript");
    // WAV: a RIFF INFO list.
    let wav = std::fs::read(file_of(&b.wav, "audio/wav")).unwrap();
    assert!(wav.windows(4).any(|w| w == b"INFO") && wav.windows(4).any(|w| w == b"INAM"));
    assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize, wav.len() - 8);
}

#[test]
fn a_new_export_replaces_the_old_only_when_complete_and_is_the_same_each_time() {
    let dir = tempfile::tempdir().unwrap();
    let b = build(dir.path());
    let dest = dir.path().join("Exports").join("Rowans");
    b.j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
    let first: Vec<(PathBuf, Vec<u8>)> = walk(&dest).into_iter().map(|p| (p.strip_prefix(&dest).unwrap().to_path_buf(), std::fs::read(&p).unwrap())).collect();
    std::fs::write(dest.join("stray.txt"), "left from before").unwrap();

    // A canceled export leaves the previous one exactly as it was.
    let ctx = JobContext::new();
    ctx.cancel();
    assert!(matches!(b.j.export(&dest, Privacy::default(), &ctx), Err(mdp_core::Error::Canceled)));
    assert!(dest.join("stray.txt").exists());
    let siblings: Vec<String> = std::fs::read_dir(dir.path().join("Exports")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(siblings, vec!["Rowans".to_string()], "nothing half-written is left beside it");

    // exporting again gives the same files, and replaces the old folder whole.
    b.j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
    assert!(!dest.join("stray.txt").exists());
    let second: Vec<(PathBuf, Vec<u8>)> = walk(&dest).into_iter().map(|p| (p.strip_prefix(&dest).unwrap().to_path_buf(), std::fs::read(&p).unwrap())).collect();
    assert_eq!(first.len(), second.len());
    for ((a, x), (b2, y)) in first.iter().zip(&second) {
        assert_eq!(a, b2);
        assert!(x == y, "{} differs between two exports", a.display());
    }
    let siblings: Vec<String> = std::fs::read_dir(dir.path().join("Exports")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(siblings, vec!["Rowans".to_string()]);

    // A folder holding other files is never replaced.
    let docs = dir.path().join("Documents");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("taxes.xlsx"), "important").unwrap();
    let e = b.j.export(&docs, Privacy::default(), &JobContext::new()).unwrap_err();
    assert!(e.to_string().contains("already holds other files"), "{e}");
    assert!(docs.join("taxes.xlsx").exists());
}

/// A folder whose parent the app may not write to (one made at the top of a
/// drive, say): the export is built inside the folder instead of beside it.
#[cfg(unix)]
#[test]
fn an_export_folder_whose_parent_is_locked_is_built_inside_it() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let b = build(dir.path());
    let top = dir.path().join("Drive");
    let dest = top.join("Family History");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::set_permissions(&top, std::fs::Permissions::from_mode(0o555)).unwrap();
    let locked = std::fs::create_dir(top.join("probe")).is_err();
    let result = (|| {
        b.j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
        let first: Vec<(PathBuf, Vec<u8>)> = walk(&dest).into_iter().map(|p| (p.strip_prefix(&dest).unwrap().to_path_buf(), std::fs::read(&p).unwrap())).collect();
        assert!(dest.join(FRONT).is_file());
        // A canceled export leaves the first one as it was.
        let ctx = JobContext::new();
        ctx.cancel();
        assert!(matches!(b.j.export(&dest, Privacy::default(), &ctx), Err(mdp_core::Error::Canceled)));
        // Exporting again replaces it with the same files and leaves nothing extra.
        b.j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
        let second: Vec<(PathBuf, Vec<u8>)> = walk(&dest).into_iter().map(|p| (p.strip_prefix(&dest).unwrap().to_path_buf(), std::fs::read(&p).unwrap())).collect();
        assert_eq!(first, second);
        let inside: Vec<String> = std::fs::read_dir(&dest).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(!inside.iter().any(|n| n.contains("export in progress") || n.contains("previous export")), "{inside:?}");
    })();
    let _ = std::fs::set_permissions(&top, std::fs::Permissions::from_mode(0o755));
    let () = result;
    if !locked {
        eprintln!("running as root, so the parent could not be locked; checked the normal path only");
    }
}

/// Exporting again into the same folder while something (OneDrive, File
/// Explorer) briefly holds it, or with an old "previous export" folder that
/// could not be removed, still replaces the export.
#[cfg(unix)]
#[test]
fn exporting_again_waits_for_a_briefly_locked_folder() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let b = build(dir.path());
    let top = dir.path().join("OneDrive");
    let dest = top.join("Export");
    b.j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
    let stuck = top.join("Export (previous export)");
    std::fs::create_dir_all(&stuck).unwrap();
    std::fs::write(stuck.join("still syncing.tmp"), "x").unwrap();

    // Lock the folder for a moment while the export runs.
    let t = top.clone();
    let lock = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        std::fs::set_permissions(&t, std::fs::Permissions::from_mode(0o555)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        std::fs::set_permissions(&t, std::fs::Permissions::from_mode(0o755)).unwrap();
    });
    let r = b.j.export(&dest, Privacy::default(), &JobContext::new());
    lock.join().unwrap();
    r.unwrap();
    assert!(dest.join(FRONT).is_file());
    assert!(stuck.join("still syncing.tmp").exists(), "a folder that is not an export is left alone");
    let mut names: Vec<String> = std::fs::read_dir(&top).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, vec!["Export".to_string(), "Export (previous export)".to_string()]);
}

#[test]
fn the_preview_uses_the_same_pages_and_points_at_the_working_files() {
    let dir = tempfile::tempdir().unwrap();
    let b = build(dir.path());
    let (front, report) = b.j.preview(Privacy::default()).unwrap();
    assert!(front.is_file());
    assert!(front.starts_with(b.j.root().join("cache")));
    let root = front.parent().unwrap();
    assert!(report.note.starts_with("Left out:"), "{}", report.note);
    assert!(report.note.contains("1 memory with nothing in them yet") && report.note.contains("not in a memory"), "{}", report.note);
    let mut saw_working = false;
    for p in walk(root).into_iter().filter(|p| p.extension().is_some_and(|e| e == "html")) {
        let html = std::fs::read_to_string(&p).unwrap();
        assert!(html.contains("class=\"pv-banner\""), "the preview's note is on every page");
        for l in links(&html) {
            saw_working |= l.contains("managed-content/");
            assert!(p.parent().unwrap().join(&l).exists(), "{} links to missing {l}", p.display());
        }
    }
    assert!(saw_working, "media are shown from the Journey's own files");
    assert!(!root.join("photos").exists(), "nothing is copied for the preview");
    // The export itself never carries the note.
    let dest = dir.path().join("export");
    b.j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
    assert!(!std::fs::read_to_string(dest.join(FRONT)).unwrap().contains("pv-banner\""));
}

#[test]
fn names_and_dates_follow_d10() {
    assert_eq!(export::date_part(Some("1962-06-02"), None), "1962-06-02");
    assert_eq!(export::date_part(Some("1913-06"), None), "1913-06");
    assert_eq!(export::date_part(Some("1923~"), None), "1923");
    assert_eq!(export::date_part(Some("197X"), None), "1970s");
    assert_eq!(export::date_part(Some("1960/1965"), None), "1960");
    assert_eq!(export::date_part(Some("../1900"), None), "1900");
    assert_eq!(export::date_part(None, None), "undated");
    assert_eq!(export::slug("José's Café — 1962!", 40), "joses-cafe-1962");
    assert_eq!(
        export::export_name("voice recording", "\"Ask not\": 1961 inaugural address (excerpt)", "k7f3m2qa", Some("1961-01-20"), None, "MP3"),
        "audio/1961/1961-01-20_ask-not-1961-inaugural-addre_k7f3m2qa.mp3"
    );
    assert_eq!(export::export_name("picture", "Cup", "abcdefgh", None, None, "jpg"), "photos/undated/undated_cup_abcdefgh.jpg");
}

#[test]
fn the_sample_family_exports() {
    // the sample family exports its two finished memories; set
    // MDP_EXPORT_OUT to keep the export for a look.
    let dir = tempfile::tempdir().unwrap();
    let j = mdp_core::sample::create(&dir.path().join("Sample")).unwrap();
    let dest = std::env::var("MDP_EXPORT_OUT").map(PathBuf::from).unwrap_or_else(|_| dir.path().join("export"));
    let r = j.export(&dest, Privacy::default(), &JobContext::new()).unwrap();
    let cat: Value = serde_json::from_str(&std::fs::read_to_string(dest.join("catalog.json")).unwrap()).unwrap();
    assert_eq!(cat["@graph"].as_array().unwrap().iter().filter(|o| o["@type"] == "Memory").count(), 2, "{}", r.note);
    if let Ok(p) = std::env::var("MDP_PREVIEW_OUT") {
        let (front, _) = j.preview(Privacy::default()).unwrap();
        let _ = std::fs::remove_dir_all(&p);
        std::fs::rename(front.parent().unwrap(), &p).unwrap();
    }
}
