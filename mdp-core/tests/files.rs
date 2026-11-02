use std::path::{Path, PathBuf};

use mdp_core::folder::absolute;
use mdp_core::import::checksum;
use mdp_core::jobs::JobContext;
use mdp_core::Journey;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn with_two(dir: &Path) -> (Journey, Vec<String>) {
    let mut j = Journey::create(&dir.join("J"), "Rowans").unwrap();
    let (r, _) = j.import(&[fixtures().join("walter-camera.jpg"), fixtures().join("hello.wav")], &JobContext::new()).unwrap();
    (j, r.imported)
}

fn path_of(j: &Journey, id: &str) -> String {
    j.conn().query_row("SELECT path FROM content_file WHERE content_id = ?1", [id], |r| r.get(0)).unwrap()
}

fn state_of(j: &Journey, id: &str) -> String {
    j.conn().query_row("SELECT state FROM content WHERE id = ?1", [id], |r| r.get(0)).unwrap()
}

fn unlock(p: &Path) {
    let mut perm = std::fs::metadata(p).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(p, perm).unwrap();
}

#[test]
fn a_moved_or_renamed_file_is_found_again_by_checksum() {
    let dir = tempfile::tempdir().unwrap();
    let (j, ids) = with_two(dir.path());
    let folder = j.root().to_path_buf();
    let rel = path_of(&j, &ids[0]);
    let at = absolute(&folder, &rel);
    let sum = checksum(&at).unwrap();
    drop(j);
    // Someone tidies up in File Explorer: renames the picture and moves it up a level.
    let elsewhere = folder.join("managed-content").join("tidy").join("nice name.jpg");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    unlock(&at);
    std::fs::rename(&at, &elsewhere).unwrap();
    // And deletes the recording.
    let wav = absolute(&folder, &path_of(&Journey::open(&folder).unwrap(), &ids[1]));
    unlock(&wav);
    std::fs::remove_file(&wav).unwrap();

    let mut j = Journey::open(&folder).unwrap();
    let check = j.check_files(&JobContext::new()).unwrap();
    assert_eq!(check.relinked, vec![rel.clone()]);
    assert_eq!(check.missing.len(), 1);
    assert_eq!(checksum(&at).unwrap(), sum);
    assert!(std::fs::metadata(&at).unwrap().permissions().readonly());
    assert!(!elsewhere.exists());
    // Nothing to do the second time.
    let again = j.check_files(&JobContext::new()).unwrap();
    assert!(again.relinked.is_empty());
}

#[test]
fn set_aside_and_back_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, ids) = with_two(dir.path());
    j.set_content_state(&ids, "set aside").unwrap();
    assert_eq!(state_of(&j, &ids[0]), "set aside");
    assert_eq!(state_of(&j, &ids[1]), "set aside");
    j.set_content_state(&ids[..1], "waiting").unwrap();
    assert_eq!(state_of(&j, &ids[0]), "waiting");
    j.undo().unwrap();
    assert_eq!(state_of(&j, &ids[0]), "set aside");
    assert!(j.set_content_state(&ids, "in a memory").is_err());
}

#[test]
fn deleting_the_copy_never_touches_the_original_and_undo_brings_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.jpg");
    std::fs::copy(fixtures().join("walter-camera.jpg"), &original).unwrap();
    let before = (checksum(&original).unwrap(), std::fs::metadata(&original).unwrap().modified().unwrap());
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let (r, _) = j.import(&[original.clone()], &JobContext::new()).unwrap();
    let id = r.imported[0].clone();
    let rel = path_of(&j, &id);
    let copy = absolute(j.root(), &rel);

    j.delete_content(&[id.clone()]).unwrap();
    let n: i64 = j.conn().query_row("SELECT count(*) FROM content", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    assert!(!copy.exists(), "the Journey's copy is removed");
    assert_eq!((checksum(&original).unwrap(), std::fs::metadata(&original).unwrap().modified().unwrap()), before);

    j.undo().unwrap();
    assert_eq!(state_of(&j, &id), "waiting");
    assert!(copy.is_file(), "undo brings the copy back");
    assert!(std::fs::metadata(&copy).unwrap().permissions().readonly());

    // Delete again and reopen: the removed copy is gone for good, the original stays.
    j.delete_content(&[id]).unwrap();
    let folder = j.root().to_path_buf();
    drop(j);
    let _j = Journey::open(&folder).unwrap();
    assert!(!folder.join("managed-content").join("removed").exists());
    assert!(original.is_file());
}

#[test]
fn undoing_an_import_puts_the_copies_aside() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, ids) = with_two(dir.path());
    let copies: Vec<PathBuf> = ids.iter().map(|id| absolute(j.root(), &path_of(&j, id))).collect();
    j.undo().unwrap();
    let n: i64 = j.conn().query_row("SELECT count(*) FROM content", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    assert!(copies.iter().all(|c| !c.exists()));
}

#[test]
fn content_in_a_memory_cannot_be_deleted_here() {
    let dir = tempfile::tempdir().unwrap();
    let (mut j, ids) = with_two(dir.path());
    let id = ids[0].clone();
    j.act(|a| {
        let m = a.create("memory", serde_json::json!({"name": "Trip"}).as_object().unwrap().clone())?;
        a.update("content", mdp_core::id_key(&id), "state", "in a memory".into())?;
        a.insert("memory_content", serde_json::json!({"memory_id": m, "content_id": id, "position": 0}).as_object().unwrap().clone())
    })
    .unwrap();
    assert!(j.delete_content(&ids[..1]).is_err());
    assert!(j.set_content_state(&ids[..1], "set aside").is_err());
}
