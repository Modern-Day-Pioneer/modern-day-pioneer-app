//! Sprint 5: recordings made in the app, transcripts and corrections.
//! The speech model isn't needed: transcripts are given as lines.

use std::path::{Path, PathBuf};

use mdp_core::voice::{self, Line, WavFile};
use mdp_core::{folder, Journey};

fn count(j: &Journey, sql: &str) -> i64 {
    j.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

/// A WAV of `seconds` of a quiet tone, as the recorder writes it.
fn tone(path: &Path, seconds: f64) -> PathBuf {
    let rate = 48_000;
    let mut w = WavFile::create(path, rate).unwrap();
    let n = (seconds * rate as f64) as usize;
    let chunk: Vec<f32> = (0..n).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.2).collect();
    for c in chunk.chunks(4800) {
        w.write(c).unwrap();
    }
    let s = w.finish().unwrap();
    assert!((s - seconds).abs() < 0.01);
    path.to_path_buf()
}

fn files_of(j: &Journey, id: &str) -> Vec<(String, String, bool)> {
    let mut st = j.conn().prepare("SELECT path, format, is_copy FROM content_file WHERE content_id = ?1 ORDER BY is_copy").unwrap();
    st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(|r| r.unwrap()).collect()
}

fn lines() -> Vec<Line> {
    vec![
        Line { start: 0.0, end: 2.5, text: "We got the camera in 1958.".into() },
        Line { start: 2.6, end: 5.0, text: "Walter carried it everywhere.".into() },
        Line { start: 8.0, end: 10.0, text: "Then June took it over.".into() },
    ]
}

#[test]
fn wav_text_and_captions_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let wav = tone(&dir.path().join("t.wav"), 1.5);
    assert!((voice::wav_seconds(&wav).unwrap() - 1.5).abs() < 0.01);

    // A recording cut off before its sizes were written can be repaired.
    let mut w = WavFile::create(&dir.path().join("cut.wav"), 16_000).unwrap();
    w.write(&vec![0.1; 16_000]).unwrap();
    drop(w);
    let cut = dir.path().join("cut.wav");
    assert_eq!(voice::wav_seconds(&cut).unwrap(), 0.0);
    voice::repair_wav(&cut).unwrap();
    assert!((voice::wav_seconds(&cut).unwrap() - 1.0).abs() < 0.01);

    let text = voice::to_text(&lines());
    // A pause of 2 seconds or more starts a new paragraph.
    assert_eq!(text, "We got the camera in 1958. Walter carried it everywhere.\r\n\r\nThen June took it over.\r\n");
    let vtt = voice::to_vtt(&lines());
    assert!(vtt.starts_with("WEBVTT\r\n\r\n1\r\n00:00:00.000 --> 00:00:02.500\r\n"));
    assert_eq!(voice::parse_vtt(&vtt), lines());
}

#[test]
fn a_recording_is_kept_as_flac_with_an_mp3_copy_in_its_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let m = j.add_memory("Walter's camera", None).unwrap();
    let wav = tone(&dir.path().join("rec.wav"), 2.0);
    let id = j.add_recording(&wav, Some(&m), Some("Grandpa on the camera")).unwrap();
    assert!(!wav.exists(), "the temporary WAV is used up");

    let files = files_of(&j, &id);
    if mdp_core::convert::find_tool().is_some() {
        assert_eq!(files.iter().map(|f| f.1.as_str()).collect::<Vec<_>>(), ["audio/flac", "audio/mpeg"]);
    } else {
        assert_eq!(files[0].1, "audio/wav");
    }
    for (p, _, _) in &files {
        let abs = folder::absolute(j.root(), p);
        assert!(abs.is_file());
        assert!(std::fs::metadata(&abs).unwrap().permissions().readonly());
    }
    let v = j.memory_view(&m).unwrap();
    assert_eq!(v.status, "complete");
    assert_eq!(v.items[0].kind, "voice recording");
    assert_eq!(v.items[0].name, "Grandpa on the camera");
    let dur: String = j.conn().query_row("SELECT duration FROM voice_recording WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
    assert_eq!(dur, "PT2S");

    // Undo takes the recording out and puts its files aside.
    assert!(j.undo().unwrap());
    assert_eq!(count(&j, "SELECT count(*) FROM content"), 0);
    assert_eq!(j.memory_view(&m).unwrap().status, "needs content");
    for (p, _, _) in &files {
        assert!(!folder::absolute(j.root(), p).exists());
    }
}

#[test]
fn a_recording_made_outside_a_memory_waits_with_a_dated_title() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let id = j.add_recording(&tone(&dir.path().join("rec.wav"), 0.5), None, None).unwrap();
    let (state, name): (String, String) =
        j.conn().query_row("SELECT state, name FROM content WHERE id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(state, "waiting");
    assert!(name.starts_with("Recording, "), "{name}");
    assert_eq!(j.waiting().unwrap().len(), 1);
}

#[test]
fn a_transcript_belongs_to_its_recording() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let m = j.add_memory("Walter's camera", None).unwrap();
    let rec = j.add_recording(&tone(&dir.path().join("rec.wav"), 1.0), Some(&m), Some("Camera story")).unwrap();
    assert_eq!(j.untranscribed().unwrap(), vec![rec.clone()]);

    let t = j.save_transcript(&rec, &lines(), "whisper.cpp medium.en").unwrap();
    assert!(j.untranscribed().unwrap().is_empty());
    let view = j.transcript_of(&rec).unwrap().unwrap();
    assert_eq!(view.id, t);
    assert_eq!(view.status, "automatic");
    assert_eq!(view.made, "automatic");
    assert_eq!(view.transcribed_by.as_deref(), Some("whisper.cpp medium.en"));
    assert_eq!(view.lines, lines());
    assert!(view.text.contains("Then June took it over."));
    let files = files_of(&j, &t);
    assert_eq!(files.iter().map(|f| f.1.as_str()).collect::<Vec<_>>(), ["text/plain", "text/vtt"]);
    assert_eq!(count(&j, "SELECT count(*) FROM content_relationship WHERE kind = 'transcript of'"), 1);

    // Not an item of its own: not in the memory and not waiting.
    let mv = j.memory_view(&m).unwrap();
    assert_eq!(mv.items.len(), 1);
    assert_eq!(mv.items[0].transcript.as_deref(), Some("automatic"));
    assert!(j.waiting().unwrap().is_empty());
    assert!(j.raw_content("waiting").unwrap().is_empty());
    assert_eq!(j.home().unwrap().counts.waiting, 0);

    // In the plain-text copy it reads as the recording's transcript.
    let item = j.item(&t).unwrap().unwrap();
    assert_eq!(item["transcriptOf"], format!("mdpid:{rec}"), "{item}");

    // Undo removes the transcript and its files.
    assert!(j.undo().unwrap());
    assert!(j.transcript_of(&rec).unwrap().is_none());
    for (p, _, _) in &files {
        assert!(!folder::absolute(j.root(), p).exists());
    }
}

#[test]
fn a_correction_keeps_the_earlier_version_and_undo_goes_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    j.set_author("Jared");
    let rec = j.add_recording(&tone(&dir.path().join("rec.wav"), 1.0), None, Some("Camera story")).unwrap();
    let t = j.save_transcript(&rec, &lines(), "whisper.cpp medium.en").unwrap();
    let first = files_of(&j, &t);

    let mut fixed = lines();
    fixed[0].text = "We got the camera in 1957.".into();
    j.correct_transcript(&t, &fixed, true).unwrap();
    let view = j.transcript_of(&rec).unwrap().unwrap();
    assert_eq!(view.status, "checked");
    assert_eq!(view.made, "checked");
    assert_eq!(view.lines[0].text, "We got the camera in 1957.");
    assert_eq!(view.transcribed_by.as_deref(), Some("whisper.cpp medium.en (checked by Jared)"));
    assert_eq!(view.version, 2);
    let second = files_of(&j, &t);
    assert_ne!(first[0].0, second[0].0);
    // The earlier files are still there, unchanged, and history names them.
    for (p, _, _) in first.iter().chain(&second) {
        assert!(folder::absolute(j.root(), p).is_file(), "{p}");
    }
    let old = std::fs::read_to_string(folder::absolute(j.root(), &first[0].0)).unwrap();
    assert!(old.contains("1958"));
    let named = count(&j, &format!("SELECT count(*) FROM history WHERE table_name = 'content_file' AND old_value LIKE '%{}%'", first[0].0));
    assert_eq!(named, 1);

    // A second correction gets the next version number.
    fixed[1].text = "Walter took it everywhere.".into();
    j.correct_transcript(&t, &fixed, false).unwrap();
    let third = files_of(&j, &t);
    assert!(third[0].0.contains(&format!("{t}-3.")), "{}", third[0].0);
    let view = j.transcript_of(&rec).unwrap().unwrap();
    assert_eq!(view.status, "automatic");
    // Changed by hand but not marked checked: it no longer says "made automatically".
    assert_eq!(view.made, "corrected");
    assert_eq!(view.transcribed_by.as_deref(), Some("whisper.cpp medium.en (corrected by Jared)"));

    // Undo twice: back to the first words, marked automatic.
    assert!(j.undo().unwrap());
    assert!(j.undo().unwrap());
    let back = j.transcript_of(&rec).unwrap().unwrap();
    assert_eq!(back.lines, lines());
    assert_eq!(back.status, "automatic");
    assert_eq!(files_of(&j, &t), first);
    // Every version's files are kept.
    for (p, _, _) in second.iter().chain(&third) {
        assert!(folder::absolute(j.root(), p).is_file(), "{p}");
    }

    assert!(j.correct_transcript(&t, &[Line { start: 0.0, end: 1.0, text: " ".into() }], true).is_err());
}

#[test]
fn removing_a_waiting_recording_removes_its_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let rec = j.add_recording(&tone(&dir.path().join("rec.wav"), 1.0), None, None).unwrap();
    let t = j.save_transcript(&rec, &lines(), "whisper.cpp small.en").unwrap();
    j.delete_content(&[rec.clone()]).unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM content"), 0);
    // Undo brings both back.
    assert!(j.undo().unwrap());
    assert_eq!(j.transcript_of(&rec).unwrap().unwrap().id, t);
}

#[test]
fn a_transcript_typed_by_hand_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    j.set_author("Jared");
    let m = j.add_memory("Walter's camera", None).unwrap();
    let rec = j.add_recording(&tone(&dir.path().join("rec.wav"), 1.0), Some(&m), None).unwrap();
    assert!(j.type_transcript(&rec, " \n ").is_err());
    let t = j.type_transcript(&rec, "We got the camera in 1958.\r\n\r\nJune took it over.").unwrap();
    let view = j.transcript_of(&rec).unwrap().unwrap();
    assert_eq!(view.id, t);
    assert_eq!((view.made.as_str(), view.status.as_str()), ("typed", "checked"));
    assert_eq!(view.lines.len(), 2);
    assert_eq!(j.memory_view(&m).unwrap().items[0].transcript.as_deref(), Some("typed"));
    // Only one transcript per recording; correcting a typed one keeps "typed".
    assert!(j.type_transcript(&rec, "Again").is_err());
    let mut fixed = view.lines.clone();
    fixed[1].text = "Then June took it over.".into();
    j.correct_transcript(&t, &fixed, false).unwrap();
    assert_eq!(j.transcript_of(&rec).unwrap().unwrap().made, "typed");
}

#[test]
fn a_transcript_can_be_deleted_and_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let m = j.add_memory("Walter's camera", None).unwrap();
    let rec = j.add_recording(&tone(&dir.path().join("rec.wav"), 1.0), Some(&m), None).unwrap();
    let t = j.save_transcript(&rec, &lines(), "whisper.cpp small.en").unwrap();
    let files = files_of(&j, &t);
    j.delete_transcript(&rec).unwrap();
    assert!(j.transcript_of(&rec).unwrap().is_none());
    assert_eq!(j.memory_view(&m).unwrap().items[0].transcript, None);
    assert_eq!(j.untranscribed().unwrap(), vec![rec.clone()]);
    assert!(j.delete_transcript(&rec).is_err());
    // Undo brings it back with its files.
    assert!(j.undo().unwrap());
    assert_eq!(j.transcript_of(&rec).unwrap().unwrap().id, t);
    for (p, _, _) in &files {
        assert!(folder::absolute(j.root(), p).is_file(), "{p}");
    }
    // Making it again (redo) is a new version of the same transcript.
    j.save_transcript(&rec, &lines()[..2], "whisper.cpp small.en").unwrap();
    let view = j.transcript_of(&rec).unwrap().unwrap();
    assert_eq!((view.id.as_str(), view.lines.len()), (t.as_str(), 2));
}

#[test]
fn deleting_a_recording_just_made_takes_it_out_of_its_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journey::create(&dir.path().join("J"), "Rowans").unwrap();
    let m = j.add_memory("Walter's camera", None).unwrap();
    let rec = j.add_recording(&tone(&dir.path().join("rec.wav"), 1.0), Some(&m), None).unwrap();
    j.save_transcript(&rec, &lines(), "whisper.cpp small.en").unwrap();
    j.delete_recording(&rec).unwrap();
    assert_eq!(count(&j, "SELECT count(*) FROM content"), 0);
    let v = j.memory_view(&m).unwrap();
    assert!(v.items.is_empty());
    assert_eq!(v.status, "needs content");
    // One Ctrl+Z brings back the recording, its transcript and its place in the memory.
    assert!(j.undo().unwrap());
    let v = j.memory_view(&m).unwrap();
    assert_eq!(v.items.len(), 1);
    assert_eq!(v.status, "complete");
    assert!(j.transcript_of(&rec).unwrap().is_some());
}

#[test]
fn ffmpeg_reads_a_recording_for_the_speech_model() {
    let Some(tool) = mdp_core::convert::find_tool() else { return };
    let dir = tempfile::tempdir().unwrap();
    let wav = tone(&dir.path().join("t.wav"), 2.0);
    let s = voice::samples_16k(&tool, &wav).unwrap();
    assert!((s.len() as i64 - 32_000).abs() < 400, "{}", s.len());
    assert!(s.iter().any(|x| x.abs() > 0.1));
}
