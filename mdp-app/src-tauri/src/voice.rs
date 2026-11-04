//! Sprint 5 screens: recording from the microphone, the speech model and
//! transcription with whisper.cpp. The rules for keeping recordings and transcripts are in
//! `mdp_core::voice`; this file holds what needs the device.
//!
//! None of the long work holds the Journey: recording writes a WAV in the
//! Journey's cache folder as it goes, and transcription reads the recording,
//! runs whisper.cpp, then takes the Journey only to save the result.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use mdp_core::voice::{Line, TranscriptView, WavFile};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, State};

use crate::{err, load_device, save_device, with_journey, Res, Shared};

/// The models offered: (name, file, size shown, bytes for the progress bar).
#[cfg(not(mobile))]
const MODELS: &[(&str, &str, &str, u64)] = &[
    ("medium.en", "ggml-medium.en.bin", "about 1.5 GB", 1_533_763_059),
    ("small.en", "ggml-small.en.bin", "about 470 MB", 487_614_201),
];
/// On the phone, small.en with base.en as the lighter, faster fallback.
#[cfg(mobile)]
const MODELS: &[(&str, &str, &str, u64)] = &[
    ("small.en", "ggml-small.en.bin", "about 470 MB", 487_614_201),
    ("base.en", "ggml-base.en.bin", "about 140 MB", 147_964_211),
];
const MODEL_PAGE: &str = "https://huggingface.co/ggerganov/whisper.cpp";

/// Where recordings are written while they are made (not `managed-content`,
/// whose holding folder is cleared by the file check).
const RECORDING_DIR: &str = "recording";

#[derive(Default)]
pub struct Voice {
    recording: Mutex<Option<Recorder>>,
    download: Mutex<Option<Download>>,
    jobs: Mutex<Jobs>,
}

struct Recorder {
    stop: mpsc::Sender<Msg>,
    done: mpsc::Receiver<Result<PathBuf, String>>,
    started: Instant,
    level: Arc<AtomicU32>,
    mic: String,
    memory: Option<String>,
    root: PathBuf,
}

enum Msg {
    Data(Vec<f32>),
    Stop,
}

#[derive(Clone, Default)]
struct Download {
    name: String,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Result<(), String>>>>,
}

#[derive(Default)]
struct Jobs {
    queue: VecDeque<Job>,
    current: Option<Running>,
    running: bool,
    /// The last transcript that could not be made: (recording id, name, why).
    last_error: Option<(String, String, String)>,
    /// Recordings finished since the screen last asked, so it can refresh.
    finished: Vec<String>,
}

#[derive(Clone)]
struct Job {
    root: PathBuf,
    id: String,
    name: String,
}

#[derive(Clone)]
struct Running {
    job: Job,
    percent: Arc<AtomicI32>,
    cancel: Arc<AtomicBool>,
    /// What it is doing now, for the screens.
    stage: Arc<Mutex<&'static str>>,
    started: Instant,
}

impl Running {
    fn new(job: Job) -> Running {
        Running { job, percent: Arc::default(), cancel: Arc::default(), stage: Arc::new(Mutex::new("Waiting")), started: Instant::now() }
    }

    fn set_stage(&self, stage: &'static str) {
        if let Ok(mut s) = self.stage.lock() {
            *s = stage;
        }
    }
}

const LOG_FILE: &str = "transcription-log.txt";

/// A line in the transcription log, kept beside the speech models, so a slow
/// or failed transcript can be looked into (sprint 5 check on Windows).
fn log(app: &AppHandle, text: &str) {
    let Ok(dir) = models_dir(app) else { return };
    let path = dir.join(LOG_FILE);
    // Keep the log small: start again once it passes 1 MB.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1_000_000) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {text}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

fn models_dir(app: &AppHandle) -> Res<PathBuf> {
    let dir = app.path().app_local_data_dir().map_err(err)?.join("models");
    std::fs::create_dir_all(&dir).map_err(err)?;
    Ok(dir)
}

/// small.en on the phone, or base.en on a phone with under 6 GB of memory,
/// where small.en is slow and can be stopped for using too much.
#[cfg(mobile)]
fn suggested() -> &'static str {
    use sysinfo::{MemoryRefreshKind, RefreshKind, System};
    let sys = System::new_with_specifics(RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()));
    let total = sys.total_memory();
    if total > 0 && total < 5_500_000_000 { "base.en" } else { "small.en" }
}

/// medium.en with 8 GB of memory or more, otherwise small.en.
#[cfg(not(mobile))]
fn suggested() -> &'static str {
    use sysinfo::{MemoryRefreshKind, RefreshKind, System};
    let sys = System::new_with_specifics(RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()));
    // A little under 8 GB, since part of the memory is kept by the system.
    if sys.total_memory() >= 7_500_000_000 { "medium.en" } else { "small.en" }
}

/// The model file to use: the one chosen in Settings if it is there, else
/// the suggested one, else any model in the folder.
fn current_model(app: &AppHandle) -> Option<(String, PathBuf)> {
    let dir = models_dir(app).ok()?;
    let chosen = load_device(app).speech_model;
    let mut names: Vec<String> = chosen.into_iter().collect();
    names.push(suggested().to_string());
    names.extend(MODELS.iter().map(|m| m.0.to_string()));
    for n in names {
        let p = dir.join(format!("ggml-{n}.bin"));
        if p.is_file() {
            return Some((n, p));
        }
    }
    let other = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "bin"))?;
    let name = other.file_stem()?.to_string_lossy().trim_start_matches("ggml-").to_string();
    Some((name, other))
}

#[derive(Serialize)]
pub struct ModelOption {
    name: String,
    size: String,
    present: bool,
    suggested: bool,
}

#[derive(Serialize)]
pub struct RecordingNow {
    seconds: f64,
    /// Loudest sound in the last moment, 0 to 1.
    level: f32,
    mic: String,
    memory: Option<String>,
}

#[derive(Serialize)]
pub struct TranscribingNow {
    id: String,
    name: String,
    percent: i32,
    stage: String,
    /// Seconds since it started.
    elapsed: u64,
}

#[derive(Serialize)]
pub struct VoiceStatus {
    recording: Option<RecordingNow>,
    /// The model in use, if one is on this PC.
    model: Option<String>,
    models: Vec<ModelOption>,
    folder: String,
    downloading: Option<DownloadNow>,
    auto: bool,
    transcribing: Option<TranscribingNow>,
    queued: Vec<String>,
    last_error: Option<String>,
    /// The recording the last error was about.
    error_id: Option<String>,
    /// Recordings whose transcript was saved since the last call.
    finished: Vec<String>,
    /// True on first run, until the offer to download the suggested model is answered.
    offer_model: bool,
    suggested: String,
}

#[derive(Serialize)]
pub struct DownloadNow {
    name: String,
    done: u64,
    total: u64,
    /// Set when the download ended; empty on success.
    result: Option<String>,
}

/// One call for everything the voice screens show; asked about once a second.
/// The model commands run off the window's thread (async), so a slow disk
/// or network while a model downloads never freezes the window.
#[tauri::command(async)]
pub fn voice_status(app: AppHandle, voice: State<Voice>) -> Res<VoiceStatus> {
    let recording = voice.recording.lock().map_err(err)?.as_ref().map(|r| RecordingNow {
        seconds: r.started.elapsed().as_secs_f64(),
        level: f32::from_bits(r.level.swap(0, Ordering::Relaxed)),
        mic: r.mic.clone(),
        memory: r.memory.clone(),
    });
    let dir = models_dir(&app)?;
    let model = current_model(&app).map(|m| m.0);
    let sug = suggested();
    let models = MODELS
        .iter()
        .map(|(n, f, size, _)| ModelOption { name: n.to_string(), size: size.to_string(), present: dir.join(f).is_file(), suggested: *n == sug })
        .collect();
    let downloading = voice.download.lock().map_err(err)?.as_ref().map(|d| DownloadNow {
        name: d.name.clone(),
        done: d.done.load(Ordering::Relaxed),
        total: d.total.load(Ordering::Relaxed),
        result: d.result.lock().ok().and_then(|r| r.clone()).map(|r| r.err().unwrap_or_default()),
    });
    let mut jobs = voice.jobs.lock().map_err(err)?;
    let device = load_device(&app);
    let offer_model = !device.model_offered && model.is_none() && downloading.is_none();
    Ok(VoiceStatus {
        offer_model,
        suggested: sug.to_string(),
        recording,
        model,
        models,
        folder: dir.display().to_string(),
        downloading,
        auto: device.auto_transcribe.unwrap_or(true),
        transcribing: jobs.current.as_ref().map(|c| TranscribingNow {
            id: c.job.id.clone(),
            name: c.job.name.clone(),
            percent: c.percent.load(Ordering::Relaxed),
            stage: c.stage.lock().map(|s| s.to_string()).unwrap_or_default(),
            elapsed: c.started.elapsed().as_secs(),
        }),
        queued: jobs.queue.iter().map(|j| j.id.clone()).collect(),
        last_error: jobs.last_error.as_ref().map(|(_, name, why)| format!("{name}: {why}")),
        error_id: jobs.last_error.as_ref().map(|(id, _, _)| id.clone()),
        finished: std::mem::take(&mut jobs.finished),
    })
}

/// "3:25" or "1:02:05", for the log.
fn clock(seconds: f64) -> String {
    let t = seconds.max(0.0) as u64;
    if t >= 3600 { format!("{}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60) } else { format!("{}:{:02}", t / 60, t % 60) }
}

// ---------- Recording (plan steps 2 and 3) ----------

fn open_mic(tx: mpsc::Sender<Msg>, level: Arc<AtomicU32>) -> Result<(cpal::Stream, u32, String), String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("No microphone was found. Plug one in, or check Windows sound settings.")?;
    #[allow(deprecated)]
    let mic = device.name().unwrap_or_else(|_| "Microphone".into());
    let config = device.default_input_config().map_err(|e| format!("The microphone's settings could not be read: {e}"))?;
    let channels = config.channels() as usize;
    let rate = config.sample_rate().0;
    let push = move |data: Vec<f32>| {
        // Mix every channel down to mono and note the loudest sound.
        let mono: Vec<f32> = data.chunks(channels.max(1)).map(|f| f.iter().sum::<f32>() / f.len() as f32).collect();
        let peak = mono.iter().fold(0f32, |m, s| m.max(s.abs()));
        let _ = level.fetch_max(peak.to_bits(), Ordering::Relaxed);
        let _ = tx.send(Msg::Data(mono));
    };
    let on_err = |e| eprintln!("microphone: {e}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_input_stream(&config.into(), move |d: &[f32], _: &_| push(d.to_vec()), on_err, None)
        }
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |d: &[i16], _: &_| push(d.iter().map(|s| *s as f32 / 32768.0).collect()),
            on_err,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream(
            &config.into(),
            move |d: &[i32], _: &_| push(d.iter().map(|s| *s as f32 / 2_147_483_648.0).collect()),
            on_err,
            None,
        ),
        other => return Err(format!("This microphone's format ({other:?}) isn't supported yet.")),
    }
    .map_err(|e| format!("The microphone could not be opened: {e}"))?;
    Ok((stream, rate, mic))
}

#[derive(Serialize)]
pub struct Started {
    mic: String,
}

/// Start recording into a memory, or into Waiting when `memory` is empty.
#[tauri::command]
pub fn record_start(state: State<Shared>, voice: State<Voice>, memory: Option<String>) -> Res<Started> {
    let mut slot = voice.recording.lock().map_err(err)?;
    if slot.is_some() {
        return Err("A recording is already going.".into());
    }
    let memory = memory.filter(|m| !m.is_empty());
    let root = with_journey(&state, |j| Ok(j.root().to_path_buf()))?;
    let wav = root.join(mdp_core::folder::CACHE_DIR).join(RECORDING_DIR).join(format!("{}.wav", mdp_core::ids::new_id()));
    let (tx, rx) = mpsc::channel::<Msg>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<String, String>>();
    let (done_tx, done_rx) = mpsc::channel();
    let level = Arc::new(AtomicU32::new(0));
    let lv = level.clone();
    let stop = tx.clone();
    // The stream lives on its own thread (it can't be moved between threads on every system).
    std::thread::spawn(move || {
        let (stream, rate, mic) = match open_mic(tx, lv) {
            Ok(v) => v,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        let mut file = match WavFile::create(&wav, rate) {
            Ok(f) => f,
            Err(e) => {
                let _ = ready_tx.send(Err(format!("The recording could not be started: {e}")));
                return;
            }
        };
        if let Err(e) = stream.play() {
            let _ = ready_tx.send(Err(format!("The microphone could not be started: {e}")));
            return;
        }
        let _ = ready_tx.send(Ok(mic));
        let mut failed = None;
        for m in rx.iter() {
            match m {
                Msg::Data(d) => {
                    if failed.is_none() {
                        if let Err(e) = file.write(&d) {
                            failed = Some(e.to_string());
                        }
                    }
                }
                Msg::Stop => break,
            }
        }
        drop(stream);
        let out = match failed {
            Some(e) => Err(format!("The recording could not be written: {e}")),
            None => file.finish().map(|_| wav).map_err(|e| format!("The recording could not be written: {e}")),
        };
        let _ = done_tx.send(out);
    });
    let mic = ready_rx.recv().map_err(|_| "The microphone stopped unexpectedly.".to_string())??;
    *slot = Some(Recorder { stop, done: done_rx, started: Instant::now(), level, mic: mic.clone(), memory, root });
    Ok(Started { mic })
}

/// Stop recording and keep it (FLAC plus MP3), then queue its transcript.
/// Returns the new recording's ID.
#[tauri::command]
pub fn record_stop(app: AppHandle, state: State<Shared>, voice: State<Voice>, title: Option<String>) -> Res<String> {
    let rec = voice.recording.lock().map_err(err)?.take().ok_or("Nothing is being recorded.")?;
    let _ = rec.stop.send(Msg::Stop);
    let wav = rec.done.recv().map_err(|_| "The recording stopped unexpectedly.".to_string())??;
    if rec.started.elapsed().as_secs_f64() < 0.5 {
        let _ = std::fs::remove_file(&wav);
        return Err("That recording was too short to keep.".into());
    }
    let (id, name) = {
        let mut s = state.lock().map_err(err)?;
        let j = s.journey.as_mut().filter(|j| j.root() == rec.root).ok_or("The Journey was closed while recording. The recording is kept in its cache folder.")?;
        let id = j.add_recording(&wav, rec.memory.as_deref(), title.as_deref()).map_err(err)?;
        let name: String = j.conn().query_row("SELECT name FROM content WHERE id = ?1", [&id], |r| r.get(0)).map_err(err)?;
        (id, name)
    };
    if load_device(&app).auto_transcribe.unwrap_or(true) && current_model(&app).is_some() {
        queue(&app, &voice, Job { root: rec.root, id: id.clone(), name })?;
    }
    Ok(id)
}

/// Recordings cut off by a crash are kept: at start, any left in the cache
/// folder are repaired and put in Waiting. Returns how many were saved.
pub fn rescue_recordings(j: &mut mdp_core::Journey) -> usize {
    let dir = j.root().join(mdp_core::folder::CACHE_DIR).join(RECORDING_DIR);
    let Ok(rd) = std::fs::read_dir(&dir) else { return 0 };
    let mut n = 0;
    for p in rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "wav")) {
        if mdp_core::voice::repair_wav(&p).is_ok() && j.add_recording(&p, None, Some("Interrupted recording")).is_ok() {
            n += 1;
        } else {
            let _ = std::fs::remove_file(&p);
        }
    }
    n
}

// ---------- The speech model (plan step 4) ----------

#[tauri::command]
pub fn set_auto_transcribe(app: AppHandle, on: bool) -> Res<()> {
    let mut d = load_device(&app);
    d.auto_transcribe = Some(on);
    save_device(&app, &d)
}

/// Only a model already on this PC can be chosen (sprint 5 feedback).
#[tauri::command]
pub fn choose_model(app: AppHandle, name: String) -> Res<()> {
    let file = MODELS.iter().find(|m| m.0 == name).map(|m| m.1.to_string()).unwrap_or_else(|| format!("ggml-{name}.bin"));
    if !models_dir(&app)?.join(file).is_file() {
        return Err(format!("{name} isn't on this PC yet. Download it first."));
    }
    let mut d = load_device(&app);
    d.speech_model = Some(name);
    save_device(&app, &d)
}

/// The first-run offer to download the suggested model was answered (either way).
#[tauri::command]
pub fn model_offer_answered(app: AppHandle) -> Res<()> {
    let mut d = load_device(&app);
    d.model_offered = true;
    save_device(&app, &d)
}

fn start_download(voice: &Voice, name: &str, work: impl FnOnce(&Download) -> Result<(), String> + Send + 'static) -> Res<()> {
    let mut slot = voice.download.lock().map_err(err)?;
    if slot.as_ref().is_some_and(|d| d.result.lock().map(|r| r.is_none()).unwrap_or(false)) {
        return Err("A model is already being downloaded.".into());
    }
    let d = Download {
        name: name.to_string(),
        done: Arc::default(),
        total: Arc::default(),
        cancel: Arc::default(),
        result: Arc::new(Mutex::new(None)),
    };
    *slot = Some(d.clone());
    std::thread::spawn(move || {
        let out = work(&d);
        if let Ok(mut r) = d.result.lock() {
            *r = Some(out);
        }
    });
    Ok(())
}

/// The size and SHA-256 that Hugging Face publishes for a model file.
fn published(file: &str) -> Result<(u64, String), String> {
    let list = crate::release::web().get("https://huggingface.co/api/models/ggerganov/whisper.cpp/tree/main")
        .call()
        .map_err(|e| format!("The model page could not be reached ({e}). Check the internet connection, or use a model file you already have."))?
        .into_body()
        .into_reader();
    let list: serde_json::Value = serde_json::from_reader(list).map_err(err)?;
    let entry = list
        .as_array()
        .and_then(|a| a.iter().find(|e| e["path"] == file))
        .ok_or_else(|| format!("{file} isn't listed on {MODEL_PAGE}."))?;
    let lfs = &entry["lfs"];
    let size = lfs["size"].as_u64().or(entry["size"].as_u64()).ok_or("The model page gave no size.")?;
    let sha = lfs["oid"].as_str().or(lfs["sha256"].as_str()).ok_or("The model page gave no checksum.")?;
    Ok((size, sha.to_lowercase()))
}

/// Copy `from` into `dest` through a `.partial` file, counting and hashing.
fn copy_counted(mut from: impl Read, dest: &Path, d: &Download) -> Result<String, String> {
    let partial = dest.with_extension("partial");
    let result = (|| {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&partial).map_err(err)?);
        let mut hash = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            if d.cancel.load(Ordering::Relaxed) {
                return Err("Canceled.".to_string());
            }
            let n = from.read(&mut buf).map_err(|e| format!("The download stopped ({e}). Try again."))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| format!("The model could not be saved ({e}). Is the disk full?"))?;
            hash.update(&buf[..n]);
            d.done.fetch_add(n as u64, Ordering::Relaxed);
        }
        out.flush().map_err(err)?;
        Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

/// Like `copy_counted`, but adds to the first `from` bytes already in
/// `partial` (counting and hashing them too), and keeps the partial file
/// when the copy stops, so the next try can carry on.
fn copy_resumed(mut from_reader: impl Read, partial: &Path, from: u64, d: &Download) -> Result<String, String> {
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let file = if from > 0 {
        let mut old = std::fs::File::open(partial).map_err(err)?.take(from);
        loop {
            let n = old.read(&mut buf).map_err(err)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
        let f = std::fs::OpenOptions::new().write(true).open(partial).map_err(err)?;
        f.set_len(from).map_err(err)?;
        let mut f = f;
        std::io::Seek::seek(&mut f, std::io::SeekFrom::End(0)).map_err(err)?;
        f
    } else {
        std::fs::File::create(partial).map_err(err)?
    };
    d.done.store(from, Ordering::Relaxed);
    let mut out = std::io::BufWriter::new(file);
    loop {
        if d.cancel.load(Ordering::Relaxed) {
            return Err("Canceled.".to_string());
        }
        let n = from_reader.read(&mut buf).map_err(|e| format!("The download stopped ({e})."))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| format!("The model could not be saved ({e}). Is the disk full?"))?;
        hash.update(&buf[..n]);
        d.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    out.flush().map_err(err)?;
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Download a model once, checked against the published size and checksum.
#[tauri::command(async)]
pub fn download_model(app: AppHandle, voice: State<Voice>, name: String) -> Res<()> {
    let (_, file, _, approx) = *MODELS.iter().find(|m| m.0 == name).ok_or("Unknown model.")?;
    let dest = models_dir(&app)?.join(file);
    let app2 = app.clone();
    let label = name.clone();
    start_download(&voice, &label, move |d| {
        d.total.store(approx, Ordering::Relaxed);
        let (size, sha) = published(file)?;
        d.total.store(size, Ordering::Relaxed);
        // A dropped connection (a phone that slept, a network change) picks
        // up where it stopped, a few times over (Jared, 2026-10-20).
        let partial = dest.with_extension("partial");
        let mut tries = 0;
        let got = loop {
            let have = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0).min(size);
            let mut req = crate::release::web().get(&format!("{MODEL_PAGE}/resolve/main/{file}"));
            if have > 0 {
                req = req.header("Range", &format!("bytes={have}-"));
            }
            let attempt = req
                .call()
                .map_err(|e| format!("The download could not start ({e})."))
                .and_then(|resp| {
                    let from = if resp.status() == 206 { have } else { 0 };
                    copy_resumed(resp.into_body().into_reader(), &partial, from, d)
                });
            match attempt {
                Ok(sha) => break sha,
                Err(e) if e == "Canceled." || tries >= 5 => {
                    let _ = std::fs::remove_file(&partial);
                    return Err(e);
                }
                Err(e) => {
                    tries += 1;
                    log(&app2, &format!("Model download interrupted ({e}); trying again ({tries} of 5)"));
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
            }
        };
        let len = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
        if len != size || got != sha {
            let _ = std::fs::remove_file(&partial);
            return Err("The downloaded file didn't match the one published, so it was not kept. Please try again.".into());
        }
        std::fs::rename(&partial, &dest).map_err(err)?;
        let mut s = load_device(&app2);
        s.speech_model = Some(name.clone());
        save_device(&app2, &s)
    })
}

/// The download and check above without the app, for the phone's test.
#[cfg(test)]
pub fn fetch_model_for_test(file: &str, dest: &Path) -> Result<(), String> {
    let (size, sha) = published(file)?;
    let resp = crate::release::web().get(&format!("{MODEL_PAGE}/resolve/main/{file}")).call().map_err(err)?;
    let d = Download::default();
    let got = copy_counted(resp.into_body().into_reader(), dest, &d)?;
    let partial = dest.with_extension("partial");
    let len = std::fs::metadata(&partial).map_err(err)?.len();
    if len != size || got != sha {
        return Err(format!("mismatch: {len} bytes (published {size}), sha {got} (published {sha})"));
    }
    std::fs::rename(&partial, dest).map_err(err)
}

/// whisper.cpp model files start with the bytes "lmgg" (its magic number).
fn looks_like_model(path: &Path) -> bool {
    let mut b = [0u8; 4];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut b)).is_ok() && u32::from_le_bytes(b) == 0x6767_6d6c
}

/// "Use a model file I already have…": copy it into the models folder.
#[tauri::command(async)]
pub fn use_model_file(app: AppHandle, voice: State<Voice>, path: String) -> Res<()> {
    // A file picked on the phone is copied in first, keeping its name.
    let src = if crate::mobile::is_picked(&path) {
        crate::mobile::local_file_named(&app, &path)?
    } else {
        PathBuf::from(&path)
    };
    if !looks_like_model(&src) {
        return Err("That file isn't a whisper.cpp model. Model files are named like ggml-medium.en.bin.".into());
    }
    let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let name = stem.trim_start_matches("ggml-").to_string();
    let dest = models_dir(&app)?.join(format!("ggml-{name}.bin"));
    let total = std::fs::metadata(&src).map_err(err)?.len();
    let app2 = app.clone();
    let label = name.clone();
    start_download(&voice, &label, move |d| {
        d.total.store(total, Ordering::Relaxed);
        let f = std::fs::File::open(&src).map_err(err)?;
        copy_counted(f, &dest, d)?;
        std::fs::rename(dest.with_extension("partial"), &dest).map_err(err)?;
        let mut s = load_device(&app2);
        s.speech_model = Some(name.clone());
        save_device(&app2, &s)
    })
}

#[tauri::command]
pub fn cancel_download(voice: State<Voice>) -> Res<()> {
    if let Some(d) = voice.download.lock().map_err(err)?.as_ref() {
        d.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub fn clear_download(voice: State<Voice>) -> Res<()> {
    let mut slot = voice.download.lock().map_err(err)?;
    if slot.as_ref().is_some_and(|d| d.result.lock().map(|r| r.is_some()).unwrap_or(false)) {
        *slot = None;
    }
    Ok(())
}

#[tauri::command]
pub fn show_models_folder(app: AppHandle) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(models_dir(&app)?.display().to_string(), None::<&str>).map_err(err)
}

// ---------- Transcription (plan step 5) ----------

/// "Make a transcript" on a recording (also for imported ones).
#[tauri::command]
pub fn transcribe(app: AppHandle, state: State<Shared>, voice: State<Voice>, id: String) -> Res<()> {
    if current_model(&app).is_none() {
        return Err("Download the speech model first, in Settings > Speech to text.".into());
    }
    let (root, name) = with_journey(&state, |j| {
        j.recording_file(&id)?;
        let name: String = j.conn().query_row("SELECT name FROM content WHERE id = ?1", [&id], |r| r.get(0))?;
        Ok((j.root().to_path_buf(), name))
    })?;
    queue(&app, &voice, Job { root, id, name })
}

#[tauri::command]
pub fn cancel_transcribe(voice: State<Voice>, id: String) -> Res<()> {
    let mut jobs = voice.jobs.lock().map_err(err)?;
    jobs.queue.retain(|j| j.id != id);
    // Its last failure no longer applies once canceled, deleted or typed by hand.
    if jobs.last_error.as_ref().is_some_and(|e| e.0 == id) {
        jobs.last_error = None;
    }
    if let Some(c) = jobs.current.as_ref().filter(|c| c.job.id == id) {
        c.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn queue(app: &AppHandle, voice: &Voice, job: Job) -> Res<()> {
    let mut jobs = voice.jobs.lock().map_err(err)?;
    if jobs.queue.iter().any(|j| j.id == job.id) || jobs.current.as_ref().is_some_and(|c| c.job.id == job.id) {
        return Ok(());
    }
    if jobs.last_error.as_ref().is_some_and(|e| e.0 == job.id) {
        jobs.last_error = None;
    }
    jobs.queue.push_back(job);
    if !jobs.running {
        jobs.running = true;
        let app = app.clone();
        std::thread::spawn(move || worker(app));
    }
    Ok(())
}

/// Find where each new recording's sound starts, a few at a time.
/// The recording is read without holding the Journey, so the app stays quick.
pub fn check_quiet_starts(app: &AppHandle) {
    let state = app.state::<Shared>();
    let (root, todo) = {
        let Ok(s) = state.lock() else { return };
        let Some(j) = s.journey.as_ref() else { return };
        (j.root().to_path_buf(), j.recordings_to_check().unwrap_or_default())
    };
    for (id, file) in todo.into_iter().take(5) {
        let samples = match mdp_core::convert::find_tool() {
            Some(tool) => mdp_core::voice::samples_16k(&tool, &file).map_err(err),
            None if cfg!(mobile) => crate::mobile::samples_16k(&file),
            None => return,
        };
        // A file that can't be read counts as having no quiet start, so it isn't tried forever.
        let at = samples.map(|s| mdp_core::voice::quiet_start(&s, 16_000)).unwrap_or(0.0);
        let Ok(mut s) = state.lock() else { return };
        let Some(j) = s.journey.as_mut().filter(|j| j.root() == root) else { return };
        if let Err(e) = j.set_quiet_start(&id, at) {
            crate::applog::log("error", &format!("The quiet start of a recording could not be saved: {e}"));
        }
    }
}

/// Transcribe one recording at a time until the queue is empty.
fn worker(app: AppHandle) {
    let voice = app.state::<Voice>();
    let mut loaded: Option<(PathBuf, whisper_rs::WhisperContext)> = None;
    loop {
        let running = {
            let Ok(mut jobs) = voice.jobs.lock() else { return };
            let Some(job) = jobs.queue.pop_front() else {
                jobs.running = false;
                jobs.current = None;
                return;
            };
            let r = Running::new(job);
            jobs.current = Some(r.clone());
            r
        };
        log(&app, &format!("Start: \"{}\"", running.job.name));
        // A failure inside whisper.cpp's Rust wrapper must not leave the queue stuck.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_one(&app, &running, &mut loaded)))
            .unwrap_or_else(|_| {
                loaded = None;
                Err("The speech model stopped unexpectedly.".into())
            });
        let took = running.started.elapsed().as_secs();
        match &outcome {
            Ok(()) => log(&app, &format!("Saved: \"{}\" in {took} s", running.job.name)),
            Err(e) => log(&app, &format!("Not saved: \"{}\" after {took} s: {e}", running.job.name)),
        }
        if let Ok(mut jobs) = voice.jobs.lock() {
            jobs.current = None;
            match outcome {
                Ok(()) => {
                    if jobs.last_error.as_ref().is_some_and(|e| e.0 == running.job.id) {
                        jobs.last_error = None;
                    }
                    jobs.finished.push(running.job.id.clone());
                }
                Err(e) if e == "Canceled." => {}
                Err(e) => jobs.last_error = Some((running.job.id.clone(), running.job.name.clone(), e)),
            }
        }
    }
}

fn run_one(app: &AppHandle, r: &Running, loaded: &mut Option<(PathBuf, whisper_rs::WhisperContext)>) -> Result<(), String> {
    let state = app.state::<Shared>();
    r.set_stage("Reading the recording");
    let file = {
        let s = state.lock().map_err(err)?;
        let j = s.journey.as_ref().filter(|j| j.root() == r.job.root).ok_or("The Journey was closed.")?;
        j.recording_file(&r.job.id).map_err(err)?
    };
    let (model_name, model_path) = current_model(app).ok_or("The speech model is missing. Download it again in Settings.")?;
    // No FFmpeg on the phone: recordings are read with the Symphonia library there.
    let samples = match mdp_core::convert::find_tool() {
        Some(tool) => mdp_core::voice::samples_16k(&tool, &file).map_err(err)?,
        None if cfg!(mobile) => crate::mobile::samples_16k(&file)?,
        None => return Err("The tool that reads recordings (FFmpeg) is missing.".into()),
    };
    if samples.len() < 8000 {
        return Err("The recording is too short to transcribe.".into());
    }
    let seconds = samples.len() as f64 / 16_000.0;
    // Phones mix fast and slow cores; whisper.cpp runs best there on the 4 fast ones.
    let most = if cfg!(mobile) { 4 } else { 8 };
    let threads = std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4).clamp(1, most);
    log(app, &format!("  {} of sound, model {model_name}, {threads} threads, flash attention; {}", clock(seconds), whisper_rs::print_system_info().trim()));
    if loaded.as_ref().is_none_or(|(p, _)| *p != model_path) {
        *loaded = None;
        r.set_stage("Loading the speech model");
        let t = Instant::now();
        // Flash attention, as whisper-cli uses (transcription speed discussion, 2026-10-17).
        let mut cp = whisper_rs::WhisperContextParameters::default();
        cp.flash_attn(true);
        let ctx = whisper_rs::WhisperContext::new_with_params(&model_path, cp)
            .map_err(|e| format!("The speech model could not be loaded ({e}). Try downloading it again."))?;
        *loaded = Some((model_path.clone(), ctx));
        log(app, &format!("  Model loaded in {} s", t.elapsed().as_secs()));
    }
    r.set_stage("Listening");
    let ctx = &loaded.as_ref().unwrap().1;
    let mut st = ctx.create_state().map_err(err)?;
    let mut params = whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_n_threads(threads);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // whisper.cpp reports progress once per 30 seconds of sound, so short
    // recordings would sit at 0%; the end of each new line counts too.
    //
    // The log notes where in the sound each new line ends, so the speed can
    // be read off it (sprint 5 feedback):
    //   12:04:10   at 3:25 of 45:15 (8%), line 41, 52 s so far
    let pct = r.percent.clone();
    params.set_progress_callback_safe(move |p: i32| {
        pct.fetch_max(p.min(99), Ordering::Relaxed);
    });
    let pct = r.percent.clone();
    let (log_app, began, total) = (app.clone(), Instant::now(), clock(seconds));
    let mut line = 0;
    params.set_segment_callback_safe_lossy(move |seg: whisper_rs::SegmentCallbackData| {
        let at = seg.end_timestamp as f64 / 100.0;
        let p = (at / seconds * 100.0) as i32;
        pct.fetch_max(p.clamp(0, 99), Ordering::Relaxed);
        line += 1;
        log(&log_app, &format!("  at {} of {total} ({}%), line {line}, {} s so far", clock(at), p.clamp(0, 100), began.elapsed().as_secs()));
    });
    // Cancel. whisper-rs 0.16's set_abort_callback_safe reads its closure
    // as the wrong type, so it answers "stop" at random and whisper.cpp
    // gives up with error -6 (Jared's 0.5.18 log); this reads the flag directly.
    unsafe extern "C" fn stop_asked(flag: *mut std::ffi::c_void) -> bool {
        // SAFETY: `flag` is `r.cancel`, which outlives `st.full` below.
        unsafe { (*(flag as *const AtomicBool)).load(Ordering::Relaxed) }
    }
    // SAFETY: as above; the callback only reads the flag.
    unsafe {
        params.set_abort_callback(Some(stop_asked));
        params.set_abort_callback_user_data(Arc::as_ptr(&r.cancel) as *mut std::ffi::c_void);
    }
    let t = Instant::now();
    let ran = st.full(params, &samples);
    let took = t.elapsed().as_secs_f64();
    log(app, &format!("  whisper.cpp ran for {took:.0} s ({:.2} s per second of sound)", took / seconds.max(0.1)));
    if r.cancel.load(Ordering::Relaxed) {
        return Err("Canceled.".into());
    }
    ran.map_err(|e| format!("Transcription failed ({e})."))?;
    let lines: Vec<Line> = st
        .as_iter()
        .filter_map(|seg| {
            let text = seg.to_str_lossy().ok()?.trim().to_string();
            // whisper.cpp marks silence and music like "[BLANK_AUDIO]".
            if text.is_empty() || (text.starts_with('[') && text.ends_with(']')) {
                return None;
            }
            // Times are in hundredths of a second.
            Some(Line { start: seg.start_timestamp() as f64 / 100.0, end: seg.end_timestamp() as f64 / 100.0, text })
        })
        .collect();
    if lines.is_empty() {
        return Err("No speech was heard in this recording.".into());
    }
    r.set_stage("Saving");
    let mut s = state.lock().map_err(err)?;
    let j = s.journey.as_mut().filter(|j| j.root() == r.job.root).ok_or("The Journey was closed.")?;
    j.save_transcript(&r.job.id, &lines, &format!("whisper.cpp {model_name}")).map_err(err)?;
    Ok(())
}

// ---------- Transcripts (plan step 6) ----------

#[tauri::command]
pub fn transcript(state: State<Shared>, id: String) -> Res<Option<TranscriptView>> {
    with_journey(&state, |j| j.transcript_of(&id))
}

/// "Type it myself…" for a recording with no transcript.
#[tauri::command]
pub fn type_transcript(state: State<Shared>, voice: State<Voice>, id: String, text: String) -> Res<()> {
    cancel_transcribe(voice, id.clone())?;
    with_journey(&state, |j| j.type_transcript(&id, &text).map(|_| ()))
}

#[tauri::command]
pub fn delete_transcript(state: State<Shared>, voice: State<Voice>, id: String) -> Res<()> {
    cancel_transcribe(voice, id.clone())?;
    with_journey(&state, |j| j.delete_transcript(&id))
}

/// "Delete it" when finishing a recording: its transcript stops and goes too.
#[tauri::command]
pub fn delete_recording(state: State<Shared>, voice: State<Voice>, id: String) -> Res<()> {
    cancel_transcribe(voice, id.clone())?;
    with_journey(&state, |j| j.delete_recording(&id))
}

#[tauri::command]
pub fn correct_transcript(state: State<Shared>, id: String, lines: Vec<Line>, checked: bool) -> Res<()> {
    with_journey(&state, |j| j.correct_transcript(&id, &lines, checked))
}
