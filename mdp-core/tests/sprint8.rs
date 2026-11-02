//! Sprint 8 (release 1.1, Windows polish): sorting and filtering memories,
//! GIF pictures, adding several people to a family, portraits, and turning
//! a picture without changing the original.

use std::path::{Path, PathBuf};

use mdp_core::export::Privacy;
use mdp_core::jobs::JobContext;
use mdp_core::pictures::turned_orientation;
use mdp_core::Journey;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// A one-pixel GIF.
const GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1,
    0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B,
];

fn orientation(path: &Path) -> Option<u32> {
    let f = std::fs::File::open(path).ok()?;
    let e = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(f)).ok()?;
    e.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?.value.get_uint(0)
}

fn jpgs(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(jpgs(&p));
        } else if p.extension().is_some_and(|x| x == "jpg") {
            out.push(p);
        }
    }
    out
}

#[test]
fn turns_add_up_from_the_cameras_own_flag() {
    assert_eq!(turned_orientation(1, 1), 6);
    assert_eq!(turned_orientation(1, -1), 8);
    assert_eq!(turned_orientation(6, 1), 3);
    assert_eq!(turned_orientation(8, 1), 1);
    assert_eq!(turned_orientation(2, 1), 7, "a mirrored picture stays mirrored");
    assert_eq!(turned_orientation(1, 4), 1);
}

#[test]
fn memories_list_with_people_stories_and_date_added() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let june = j.add_individual("Person", "June").unwrap();
    let story = j.add_story("Kitchen").unwrap();
    let a = j.add_memory("Apples", Some(&story)).unwrap();
    let b = j.add_memory("Bread", None).unwrap();
    j.link_to_memory(&b, &june, "in the picture", None, false).unwrap();
    let rows = j.memory_list().unwrap();
    assert_eq!(rows.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Apples", "Bread"]);
    assert_eq!(rows[0].stories, [story.clone()]);
    assert!(rows[0].people.is_empty() && rows[1].stories.is_empty());
    assert_eq!(rows[1].people, [june]);
    assert!(rows.iter().all(|m| m.added.is_some() && m.status == "needs content"));
    assert!(rows[0].added <= rows[1].added);
    let _ = a;
}

#[test]
fn gifs_come_in_as_pictures_and_portraits_turns_and_families_work() {
    let dir = tempfile::tempdir().unwrap();
    let gif = dir.path().join("dancing.gif");
    std::fs::write(&gif, GIF).unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let (r, _) = j.import(&[gif, fixtures().join("walter-camera.jpg")], &JobContext::new()).unwrap();
    assert_eq!(r.imported.len(), 2, "{r:?}");
    let [gif_id, jpg] = <[String; 2]>::try_from(r.imported.clone()).unwrap();
    let (kind, format): (String, String) = j
        .conn()
        .query_row("SELECT c.kind, f.format FROM content c JOIN content_file f ON f.content_id = c.id WHERE c.id = ?1", [&gif_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((kind.as_str(), format.as_str()), ("picture", "image/gif"));

    // Several people into a family in one action.
    let walter = j.add_individual("Person", "Walter").unwrap();
    let june = j.add_individual("Person", "June").unwrap();
    let fam = j.add_family("The Rowans").unwrap();
    assert_eq!(j.add_family_members(&fam, &[walter.clone(), june.clone(), walter.clone()]).unwrap(), 2);
    assert_eq!(j.add_family_members(&fam, &[june.clone()]).unwrap(), 0);
    assert!(j.undo().unwrap());
    assert_eq!(j.family_view(&fam).unwrap().members.len(), 0, "one undo takes both out");
    j.add_family_members(&fam, &[walter.clone(), june.clone()]).unwrap();

    // A portrait chosen from a picture in their memories.
    let m = j.new_memory_with("Walter's camera", &[jpg.clone(), gif_id.clone()], None).unwrap();
    j.link_to_memory(&m, &walter, "in the picture", None, false).unwrap();
    let choices = j.portrait_choices(&walter).unwrap();
    assert_eq!(choices.len(), 2);
    assert!(j.portrait_choices(&june).unwrap().is_empty());
    j.set_portrait(&walter, Some(&jpg)).unwrap();
    assert_eq!(j.portraits().unwrap().get(&walter), Some(&jpg));
    assert!(j.set_portrait(&walter, Some(&walter)).is_err(), "only a picture");
    let line = j.item(&walter).unwrap().unwrap();
    assert_eq!(line["image"], format!("mdpid:{jpg}"));

    // Turn the picture: the original is untouched, the export copy carries the flag.
    let original = j.conn().query_row("SELECT path FROM content_file WHERE content_id = ?1 AND is_copy = 0", [&jpg], |r| r.get::<_, String>(0)).unwrap();
    let before = std::fs::read(j.root().join(&original)).unwrap();
    let camera = orientation(&j.root().join(&original)).unwrap_or(1) as u16;
    assert_eq!(j.turn_picture(&jpg, 1).unwrap(), 1);
    assert_eq!(j.turn_picture(&jpg, -2).unwrap(), 3);
    assert_eq!(j.turn_picture(&jpg, 1).unwrap(), 0);
    j.turn_picture(&jpg, 1).unwrap();
    assert!(j.turn_picture(&gif_id, 1).is_ok());
    assert!(j.turn_picture(&m, 1).is_err(), "only pictures turn");
    assert_eq!(std::fs::read(j.root().join(&original)).unwrap(), before, "the original never changes");
    assert_eq!(j.memory_view(&m).unwrap().items.iter().find(|i| i.id == jpg).unwrap().turn, 1);

    let out = dir.path().join("Export");
    j.export(&out, Privacy::default(), &JobContext::new()).unwrap();
    let exported = jpgs(&out);
    assert_eq!(exported.len(), 1, "{exported:?}");
    assert_eq!(orientation(&exported[0]), Some(turned_orientation(camera, 1) as u32));
    let cat: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(out.join("catalog.json")).unwrap()).unwrap();
    let w = cat["@graph"].as_array().unwrap().iter().find(|o| o["@id"] == format!("mdpid:{walter}")).unwrap();
    assert_eq!(w["image"], format!("mdpid:{jpg}"));
    let page = std::fs::read_to_string(out.join("pages").join("people.html")).unwrap();
    assert!(page.contains("Portrait of Walter"));

    // Deleting the picture clears the portrait instead of failing.
    j.remove_from_memory(&m, &jpg).unwrap();
    j.delete_content(&[jpg.clone()]).unwrap();
    assert!(j.portraits().unwrap().is_empty());
}

#[test]
fn a_picture_without_exif_gets_only_the_turn_flag() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    // A JPEG with no EXIF at all: made from the PNG fixture by FFmpeg when it is there.
    let Some(tool) = mdp_core::convert::find_tool() else { return };
    let src = dir.path().join("plain.jpg");
    let ok = std::process::Command::new(&tool.ffmpeg)
        .args(["-loglevel", "error", "-y", "-i"])
        .arg(fixtures().join("no-exif.png"))
        .args(["-map_metadata", "-1"])
        .arg(&src)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let (r, _) = j.import(&[src], &JobContext::new()).unwrap();
    let pic = r.imported[0].clone();
    j.new_memory_with("Plain", &[pic.clone()], None).unwrap();
    j.turn_picture(&pic, -1).unwrap();
    let out = dir.path().join("Export");
    j.export(&out, Privacy::default(), &JobContext::new()).unwrap();
    let exported = jpgs(&out);
    assert_eq!(orientation(&exported[0]), Some(8));
}

fn png_size(path: &Path) -> (u32, u32) {
    let b = std::fs::read(path).unwrap();
    (u32::from_be_bytes(b[16..20].try_into().unwrap()), u32::from_be_bytes(b[20..24].try_into().unwrap()))
}

fn files_with(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_with(&p, ext));
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
    out
}

#[test]
fn turned_pictures_show_turned_in_the_export_and_the_preview() {
    let Some(tool) = mdp_core::convert::find_tool() else { return };
    let dir = tempfile::tempdir().unwrap();
    let wide = dir.path().join("wide.png");
    let ok = std::process::Command::new(&tool.ffmpeg)
        .args(["-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=40x20", "-frames:v", "1"])
        .arg(&wide)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let mut j = Journey::create(&dir.path().join("J"), "J").unwrap();
    let (r, _) = j.import(&[wide, fixtures().join("walter-camera.jpg")], &JobContext::new()).unwrap();
    let [png, jpg] = <[String; 2]>::try_from(r.imported.clone()).unwrap();
    j.new_memory_with("Turned", &[png.clone(), jpg.clone()], None).unwrap();
    j.turn_picture(&png, 1).unwrap();
    j.turn_picture(&jpg, 1).unwrap();
    let original = j.conn().query_row("SELECT path FROM content_file WHERE content_id = ?1", [&png], |r| r.get::<_, String>(0)).unwrap();
    assert_eq!(png_size(&j.root().join(&original)), (40, 20), "the original never changes");

    // The export's PNG is turned itself, since browsers have no turn flag for PNG.
    let out = dir.path().join("Export");
    j.export(&out, Privacy::default(), &JobContext::new()).unwrap();
    let pngs = files_with(&out, "png");
    assert_eq!(pngs.len(), 1, "{pngs:?}");
    assert_eq!(png_size(&pngs[0]), (20, 40));

    // The preview shows turned copies too, not the untouched originals.
    let (_, report) = j.preview(Privacy::default()).unwrap();
    let pv = Path::new(&report.folder);
    let pngs = files_with(pv, "png");
    assert_eq!(pngs.len(), 1, "{pngs:?}");
    assert_eq!(png_size(&pngs[0]), (20, 40));
    let jpgs = jpgs(pv);
    assert_eq!(jpgs.len(), 1, "{jpgs:?}");
    assert!(orientation(&jpgs[0]).is_some_and(|o| o != 1));
    // Every picture a page shows is inside the preview folder.
    let mut seen = 0;
    for page in files_with(pv, "html") {
        let html = std::fs::read_to_string(&page).unwrap();
        for src in html.split("<img src=\"").skip(1).filter_map(|s| s.split('"').next()) {
            let shown = page.parent().unwrap().join(src).canonicalize().unwrap();
            assert!(shown.starts_with(pv.canonicalize().unwrap()), "{src} in {page:?}");
            seen += 1;
        }
    }
    assert!(seen >= 2);
}
