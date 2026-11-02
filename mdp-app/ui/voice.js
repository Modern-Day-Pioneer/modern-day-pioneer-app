// Voice (sprint 5): the storyteller recording screen, the speech model in
// Settings, transcription progress, and correcting a transcript line by line
//. Uses the helpers in
// app.js, people.js and memories.js (el, $, invoke, toast, modal, ...).
'use strict';

const clock = (s) => {
  const t = Math.max(0, Math.floor(s));
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const sec = String(t % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
};

// ---------- Status of recording, the model and transcription, asked once a second while anything runs ----------

let voice = null;
let voiceTimer = null;
let awake = false;
const voiceWatchers = new Set();

async function refreshVoice() {
  try { voice = await invoke('voice_status'); } catch { return; }
  if ((voice.finished || []).length) {
    toast(voice.finished.length === 1 ? 'A transcript is ready.' : `${voice.finished.length} transcripts are ready.`);
    if (current === 'memory') reload();
    else if (current === 'home') loadHome();
  }
  paintVoiceJob();
  if (voice.offer_model && !modelOffered && !$('#app').hidden) { modelOffered = true; offerModel(voice); }
  for (const w of voiceWatchers) w(voice);
  const busy = voice.recording || voice.transcribing || voice.queued.length || (voice.downloading && voice.downloading.result === null);
  // The phone stops the app's work when its screen goes dark (Jared, 2026-10-20).
  const working = !!(voice.transcribing || voice.queued.length || (voice.downloading && voice.downloading.result === null) || voice.recording);
  if (PHONE && working !== awake) { awake = working; invoke('keep_awake', { on: working }).catch(() => {}); }
  clearTimeout(voiceTimer);
  voiceTimer = setTimeout(refreshVoice, voice.recording ? 250 : busy ? 1000 : 5000);
}

// First run: offer to download the suggested model, which becomes the default
// (transcription speed discussion, 2026-10-17). Asked once, either way.
let modelOffered = false;
async function offerModel(v) {
  const m = v.models.find((x) => x.name === v.suggested) || {};
  const ok = await modal('Download the speech model?', [
    el('p', {}, `The app makes transcripts of recordings on ${HERE} with a speech model. The one suggested for ${HERE} is ${v.suggested}${m.size ? ` (${m.size})` : ''}, a one-time download from the whisper.cpp page on Hugging Face. After that, transcripts need no internet.`),
    PHONE ? el('p', { class: 'hint' }, 'Best done on Wi-Fi, since it is a large download.') : null,
    el('p', { class: 'hint' }, 'You can also do this later, or choose another model, in Settings > Speech to text.'),
  ], { ok: 'Download now', cancel: 'Later', check: async () => {
    await invoke('download_model', { name: v.suggested });
    return true;
  } });
  await invoke('model_offer_answered').catch(() => {});
  if (ok) toast('The speech model is downloading. Settings > Speech to text shows how far along it is.');
  refreshVoice();
}

// How a transcript was made, in words (sprint 5 feedback: one typed or
// corrected by hand must not say it was made automatically).
const MADE = {
  automatic: ['Transcript made automatically, not checked yet', 'needs'],
  corrected: ['Transcript corrected by hand, not checked yet', 'needs'],
  checked: ['Transcript checked by hand', 'add'],
  typed: ['Transcript typed by hand', 'add'],
};
const madeWords = (made) => (MADE[made] || MADE.automatic)[0];

// "Listening: 40%, 1:05 so far" for a transcript being made.
function jobText(t) {
  const m = Math.floor(t.elapsed / 60);
  const sec = String(t.elapsed % 60).padStart(2, '0');
  return `${t.stage}: ${t.percent}%, ${m}:${sec} so far`;
}

// One line on Home while transcripts are being made.
function paintVoiceJob() {
  const p = $('#voice-job');
  if (!p || !voice) return;
  const t = voice.transcribing;
  const more = voice.queued.length;
  if (t) p.textContent = `Making the transcript of ${t.name}. ${jobText(t)}${more ? `, then ${plural(more, 'more recording', 'more recordings')}` : ''}.`;
  else if (voice.last_error) p.textContent = `A transcript could not be made. ${voice.last_error}`;
  else p.textContent = '';
  p.hidden = !p.textContent;
}

// What a recording's card on the memory screen shows about its transcript.
function transcriptBits(it, memoryName, extras = []) {
  if (it.kind !== 'voice recording' && it.kind !== 'video') return null;
  const box = el('div', { class: 'transcript-line' });
  let painted = false;
  const paint = (v) => {
    if (painted && !box.isConnected) { voiceWatchers.delete(paint); return; }
    painted = true;
    const now = v && v.transcribing && v.transcribing.id === it.id;
    const queued = v && v.queued.includes(it.id);
    const failed = v && !now && !queued && !it.transcript && v.error_id === it.id && v.last_error;
    if (now || queued) {
      box.replaceChildren(el('span', { class: 'quiet', role: 'status' }, now ? `Making the transcript. ${jobText(v.transcribing)}` : 'Waiting for its turn to be transcribed'),
        smallBtn('Cancel', `the transcript of ${it.name}`, async () => { await invoke('cancel_transcribe', { id: it.id }).catch(showError); refreshVoice(); }),
        moreMenu(it.name, extras));
    } else if (it.transcript) {
      const [words, cls] = MADE[it.transcript] || MADE.automatic;
      const byHand = it.transcript !== 'automatic';
      box.replaceChildren(chip(words, cls),
        smallBtn(it.transcript === 'checked' || it.transcript === 'typed' ? 'Read or correct the transcript…' : 'Check the transcript…', it.name, () => editTranscript(it.id, it.name)),
        moreMenu(it.name, [['Make the transcript again', () => redoTranscript(it.id, byHand)],
          ['Delete the transcript…', () => deleteTranscript(it.id, it.name)], ...extras]));
    } else if (failed) {
      box.replaceChildren(el('span', { class: 'error', role: 'alert' }, `The transcript could not be made. ${v.last_error.slice(v.last_error.indexOf(': ') + 2)}`),
        smallBtn('Try again', `the transcript of ${it.name}`, () => startTranscribe(it.id)),
        moreMenu(it.name, [['Type it myself…', () => typeTranscript(it.id, it.name)], ...extras]));
    } else {
      box.replaceChildren(el('span', { class: 'quiet' }, 'No transcript yet.'),
        smallBtn('Make a transcript', `of ${it.name}`, () => startTranscribe(it.id)),
        moreMenu(it.name, [['Type it myself…', () => typeTranscript(it.id, it.name)], ...extras]));
    }
  };
  paint(voice);
  voiceWatchers.add(paint);
  return box;
}

// Make the transcript again with the speech model (sprint 5 feedback). The
// current version stays in the history, so Ctrl+Z goes back to it.
async function redoTranscript(id, byHand) {
  if (byHand) {
    const ok = await dialog.ask('Make the transcript again with the speech model?\n\nThe new one replaces the words typed or checked by hand. They are kept in the history, and Ctrl+Z goes back to them.',
      { title: 'Make the transcript again', kind: 'warning', okLabel: 'Make it again', cancelLabel: 'Keep this one' });
    if (!ok) return;
  }
  startTranscribe(id);
}

async function deleteTranscript(id, name) {
  const ok = await dialog.ask(`Delete the transcript of ${name}?\n\nThe recording stays. You can undo this with Ctrl+Z until you close the app.`,
    { title: 'Delete the transcript', kind: 'warning', okLabel: 'Delete', cancelLabel: 'Keep it' });
  if (!ok) return false;
  try {
    await invoke('delete_transcript', { id });
    toast('The transcript is deleted.', { label: 'Undo', run: undoNow });
    if (current === 'memory') reload();
    refreshVoice();
    return true;
  } catch (e) { showError(e); return false; }
}

// A transcript typed by hand, for a recording with none.
async function typeTranscript(id, name) {
  const text = textField('Transcript', '', { multiline: true, hint: 'Leave an empty line between paragraphs.' });
  text.input.rows = 10;
  const ok = await modal(`Type the transcript of ${name}`, [text], { ok: 'Save', wide: true, check: async () => {
    if (!text.input.value.trim()) return 'The transcript is empty.';
    await act(invoke('type_transcript', { id, text: text.input.value }), 'The transcript is saved as typed by hand.');
    return true;
  } });
  if (ok) reload();
}

// A bar at the top of the memory screen while a transcript in it is made (sprint 5 feedback).
function transcriptProgress(items) {
  const ids = new Set(items.filter((it) => it.kind === 'voice recording' || it.kind === 'video').map((it) => it.id));
  const text = el('p', {});
  const bar = el('progress', { max: 100, value: 0, 'aria-label': 'Transcript progress' });
  const box = el('div', { class: 'job', role: 'status', hidden: true }, text, bar);
  if (!ids.size) return box;
  let painted = false;
  const paint = (v) => {
    if (painted && !box.isConnected) { voiceWatchers.delete(paint); return; }
    painted = true;
    const now = v && v.transcribing && ids.has(v.transcribing.id) ? v.transcribing : null;
    const waiting = v ? v.queued.filter((q) => ids.has(q)).length : 0;
    box.hidden = !now && !waiting;
    if (now) {
      text.textContent = `Making the transcript of ${now.name}. ${jobText(now)}${waiting ? `, then ${plural(waiting, 'more recording', 'more recordings')} in this memory` : ''}.`;
      bar.value = now.percent;
      bar.hidden = false;
    } else if (waiting) {
      text.textContent = `${plural(waiting, 'recording in this memory is', 'recordings in this memory are')} waiting for a transcript.`;
      bar.hidden = true;
    }
  };
  paint(voice);
  voiceWatchers.add(paint);
  return box;
}

async function startTranscribe(id) {
  try {
    await invoke('transcribe', { id });
    toast('The transcript is being made. You can carry on working.');
    refreshVoice();
  } catch (e) {
    showError(e);
  }
}

// ---------- The storyteller screen ----------

async function openStoryteller({ memory = null, title, coverId = null } = {}) {
  const madeIds = [];
  let made = 0;
  let changed = false;
  let recording = false;
  const img = el('img', { alt: '', hidden: true });
  if (coverId) {
    invoke('show_content', { id: coverId }).then((s) => {
      if (s.path && s.kind === 'picture') { img.src = fileUrl(s.path); img.alt = `Cover photo of ${title}`; img.hidden = false; }
    }).catch(() => {});
  }
  const timer = el('div', { class: 'timer', role: 'timer', 'aria-label': 'Time recorded' }, '0:00');
  const level = el('div', { class: 'level', 'aria-hidden': 'true' }, el('span'));
  const mic = el('p', { class: 'mic' }, memory ? 'The recording goes into this memory.' : 'The recording waits on the Waiting screen until it goes into a memory.');
  const saved = el('p', { class: 'saved', role: 'status', 'aria-live': 'polite' }, '');
  const rec = el('button', { class: 'rec', type: 'button', 'aria-pressed': 'false' }, 'Record');
  const finish = el('button', { class: 'btn finish', type: 'button' }, 'Finish');
  const d = el('dialog', { class: 'storyteller', 'aria-labelledby': 'st-title' },
    img, el('h2', { id: 'st-title' }, title), timer, level, rec, mic, saved, finish);
  document.body.append(d);

  const watch = (v) => {
    if (!v.recording) return;
    timer.textContent = clock(v.recording.seconds);
    $('span', level).style.width = `${Math.min(100, Math.round(Math.sqrt(v.recording.level) * 100))}%`;
  };
  voiceWatchers.add(watch);

  const stop = async () => {
    rec.disabled = true;
    try {
      madeIds.push(await invoke('record_stop', { title: null }));
      made = madeIds.length;
      changed = true;
      saved.textContent = `Saved: ${plural(made, 'recording', 'recordings')} so far.${voice && voice.model && voice.auto ? ' Transcripts are made in the background.' : ''}`;
    } catch (e) {
      saved.textContent = String(e);
    }
    recording = false;
    rec.textContent = made ? 'Record again' : 'Record';
    rec.setAttribute('aria-pressed', 'false');
    rec.classList.remove('on');
    rec.disabled = false;
    $('span', level).style.width = '0';
    refreshVoice();
  };
  rec.addEventListener('click', async () => {
    if (recording) { await stop(); return; }
    rec.disabled = true;
    try {
      const s = await invoke('record_start', { memory });
      recording = true;
      mic.textContent = `Recording from ${s.mic}.`;
      rec.textContent = 'Stop';
      rec.setAttribute('aria-pressed', 'true');
      rec.classList.add('on');
      timer.textContent = '0:00';
      refreshVoice();
    } catch (e) {
      saved.textContent = String(e);
    }
    rec.disabled = false;
    rec.focus();
  });

  // Finish asks what to do with the recording (sprint 5 feedback; design
  // document 5.2): save it, listen and review, delete it, or go back.
  const choose = () => new Promise((resolve) => {
    const n = made + (recording ? 1 : 0);
    const it = n === 1 ? 'it' : 'them';
    const btn = (label, v, cls = 'btn') => { const b = el('button', { class: cls, type: 'button' }, label); b.addEventListener('click', () => end(v)); return b; };
    const save = btn(`Save ${it}`, 'save', 'btn primary');
    const back = btn('Go back and keep recording', 'back');
    const c = el('dialog', { class: 'confirm', role: 'alertdialog', 'aria-labelledby': 'st-q', 'aria-describedby': 'st-qd' },
      el('b', { id: 'st-q' }, n === 1 ? 'What would you like to do with this recording?' : `What would you like to do with these ${n} recordings?`),
      el('p', { id: 'st-qd' }, recording ? 'Recording stops when you choose.' : n === 1 ? 'It is saved for now.' : 'They are saved for now.'),
      el('div', { class: 'btns' }, save, btn('Listen and review', 'review'), btn(`Delete ${it}`, 'delete'), back));
    const end = (v) => { c.close(); c.remove(); resolve(v); };
    c.addEventListener('cancel', (e) => { e.preventDefault(); end('back'); });
    d.append(c);
    c.showModal();
    save.focus();
  });
  const remove = async (id) => {
    await invoke('delete_recording', { id });
    madeIds.splice(madeIds.indexOf(id), 1);
    made = madeIds.length;
    changed = true;
  };
  // Listen to each recording made here, and delete any not wanted.
  const review = () => new Promise((resolve) => {
    const list = el('ul', { class: 'raw' });
    const status = el('p', { role: 'status', 'aria-live': 'polite' }, '');
    const keep = el('button', { class: 'btn primary', type: 'button' }, 'Save and close');
    const back = el('button', { class: 'btn', type: 'button' }, 'Go back and keep recording');
    const c = el('dialog', { class: 'ask wide', 'aria-labelledby': 'st-r' },
      el('h2', { id: 'st-r' }, made === 1 ? 'Listen to the recording' : 'Listen to the recordings'), list, status, el('div', { class: 'btns' }, keep, back));
    const end = (v) => { c.querySelectorAll('audio').forEach((a) => a.pause()); c.close(); c.remove(); resolve(v); };
    keep.addEventListener('click', () => end('save'));
    back.addEventListener('click', () => end('back'));
    c.addEventListener('cancel', (e) => { e.preventDefault(); end('back'); });
    madeIds.forEach(async (id, i) => {
      const li = el('li', {});
      list.append(li);
      const s = await invoke('show_content', { id }).catch(() => null);
      const name = s ? s.name : `Recording ${i + 1}`;
      const player = s && s.path ? el('audio', { src: fileUrl(s.path), controls: true, preload: 'metadata' })
        : el('span', { class: 'warn' }, 'The file can\'t be played here.');
      li.append(el('span', { class: 't' }, el('b', {}, name)), player,
        smallBtn('Delete this one', name, async () => {
          try {
            player.pause?.();
            await remove(id);
            li.replaceChildren(el('span', { class: 't quiet' }, `${name} is deleted.`));
            status.textContent = made ? `${plural(made, 'recording is', 'recordings are')} kept.` : 'Nothing is kept.';
            if (!made) keep.textContent = 'Close';
            keep.focus();
          } catch (e) { status.textContent = String(e); }
        }));
    });
    d.append(c);
    c.showModal();
    keep.focus();
  });
  const done = () => {
    voiceWatchers.delete(watch);
    d.close();
    d.remove();
    if (changed) reload();
  };
  const close = async () => {
    if (!recording && !made) { done(); return; }
    const choice = await choose();
    if (choice === 'back') { rec.focus(); return; }
    if (recording) await stop();
    if (choice === 'delete') {
      const n = madeIds.length;
      try {
        for (const id of [...madeIds]) await remove(id);
        toast(n === 1 ? 'The recording is deleted.' : `The ${n} recordings are deleted.`, n === 1 ? { label: 'Undo', run: undoNow } : undefined);
      } catch (e) { showError(e); }
    } else if (choice === 'review' && (await review()) === 'back') {
      rec.focus();
      return;
    }
    done();
  };
  finish.addEventListener('click', close);
  d.addEventListener('cancel', (e) => { e.preventDefault(); close(); });
  d.showModal();
  rec.focus();
  refreshVoice();
}

$('#home-record').addEventListener('click', () => openStoryteller({ title: 'New recording' }));

// ---------- Correcting a transcript ----------

async function editTranscript(recordingId, name) {
  let t;
  let shown;
  try {
    [t, shown] = await Promise.all([invoke('transcript', { id: recordingId }), invoke('show_content', { id: recordingId })]);
  } catch (e) { showError(e); return; }
  if (!t) { toast('This recording has no transcript yet.'); return; }
  const player = shown.path
    ? el(shown.kind === 'video' ? 'video' : 'audio', { src: fileUrl(shown.path), controls: true, preload: 'metadata', class: 'player' })
    : el('p', { class: 'warn' }, 'The recording\'s file is missing, so it can\'t be played here.');
  const timed = t.lines.some((l) => l.end > 0);
  const rows = t.lines.map((l, i) => {
    const id = nextId('tl');
    const box = el('textarea', { id, rows: Math.min(4, Math.max(1, Math.ceil(l.text.length / 70))) }, l.text);
    const play = timed && shown.path ? el('button', { class: 'btn', type: 'button', onclick: () => { player.currentTime = l.start; player.play(); } },
      `Play from ${clock(l.start)}`, el('span', { class: 'sr' }, `, line ${i + 1}`)) : null;
    return { l, box, li: el('li', {}, el('label', { for: id, class: 'sr' }, `Line ${i + 1}${timed ? `, at ${clock(l.start)}` : ''}`), play, box) };
  });
  const checked = el('input', { type: 'checkbox', id: nextId('c'), checked: true });
  const by = t.transcribed_by && t.made !== 'typed' ? `Made by ${t.transcribed_by}` : t.transcribed_by ? `By ${t.transcribed_by}` : '';
  const ok = await modal(`Transcript of ${name}`, [
    el('p', { class: 'quiet' }, [madeWords(t.made), by, t.version > 1 ? `version ${t.version}` : ''].filter(Boolean).join(' · ')),
    player,
    el('p', {}, smallBtn('Delete the transcript…', `of ${name}`, async (ev) => {
      if (await deleteTranscript(recordingId, name)) { ev.target.closest('dialog').querySelector('.btns .btn:not(.primary)').click(); reload(); }
    })),
    el('p', { class: 'quiet' }, 'Listen and correct each line where needed. Saving keeps the earlier version too, and Ctrl+Z undoes it.'),
    el('ol', { class: 'tlines' }, ...rows.map((r) => r.li)),
    el('label', { class: 'switch', for: checked.id }, checked, 'Mark the transcript as checked'),
  ], { ok: 'Save', wide: true, check: async () => {
    const lines = rows.map((r) => ({ start: r.l.start, end: r.l.end, text: r.box.value.replace(/\s+/g, ' ').trim() }));
    const same = rows.every((r, i) => lines[i].text === r.l.text.trim());
    if (same && (t.status === 'checked') === checked.checked) return true;
    await act(invoke('correct_transcript', { id: t.id, lines, checked: checked.checked }), checked.checked ? 'The transcript is saved and marked checked.' : 'The transcript is saved.');
    return true;
  } });
  if (player.pause) player.pause();
  if (ok) reload();
}

// The large preview shows a recording's transcript under the player.
async function viewerTranscript(item) {
  if (item.kind !== 'voice recording' && item.kind !== 'video') return null;
  const t = await invoke('transcript', { id: item.id }).catch(() => null);
  if (!t) return el('p', { class: 'quiet' }, 'No transcript yet.');
  const box = el('section', { class: 'vtranscript', 'aria-label': 'Transcript' },
    el('p', { class: 'quiet' }, madeWords(t.made), ' ',
      smallBtn('Delete the transcript…', `of ${item.name}`, async () => {
        if (await deleteTranscript(item.id, item.name)) box.replaceWith(el('p', { class: 'quiet' }, 'No transcript yet.'));
      })),
    el('pre', { class: 'text', tabindex: '0' }, t.text));
  return box;
}

// ---------- Settings > Speech to text (plan steps 4; defaults 4 to 6) ----------

function paintSpeech(v) {
  const box = $('#speech');
  if (!box || current !== 'settings') return;
  const dl = v.downloading;
  const going = dl && dl.result === null;
  const parts = [];
  if (v.model) parts.push(el('p', {}, `Speech model: ${v.model}, on ${HERE}. Transcripts are made without the internet.`));
  else parts.push(el('p', {}, `No speech model on ${HERE} yet. Recordings are kept; their transcripts wait until a model is here.`));
  const name = nextId('m');
  const download = async (m) => {
    await invoke('clear_download').catch(() => {});
    try { await invoke('download_model', { name: m }); refreshVoice(); } catch (e) { showError(e); }
  };
  // A model can be chosen only once it is on this PC (sprint 5 feedback).
  const opts = el('fieldset', { class: 'choice' }, el('legend', {}, 'Model'),
    ...v.models.map((m) => {
      const r = el('input', { type: 'radio', name, value: m.name, id: nextId('m'), checked: v.model === m.name, disabled: !m.present });
      return el('div', { class: 'row' }, el('label', { class: 'switch', for: r.id }, r,
        `${m.name}, ${m.size}${m.suggested ? ` (suggested for ${HERE})` : ''}${m.present ? ', downloaded' : ', not downloaded yet'}`),
        !m.present && !going ? smallBtn('Download', m.name, () => download(m.name)) : null);
    }));
  const chosen = () => $('input:checked', opts)?.value;
  opts.addEventListener('change', async () => {
    const m = v.models.find((x) => x.name === chosen());
    if (m && m.present) { await invoke('choose_model', { name: m.name }).catch(showError); toast(`${m.name} is used from now on.`); refreshVoice(); }
  });
  parts.push(opts);
  if (going) {
    const pct = dl.total ? Math.floor((dl.done / dl.total) * 100) : 0;
    parts.push(el('div', { class: 'job', role: 'status' },
      el('p', {}, `Getting ${dl.name}: ${pct}% (${(dl.done / 1e6).toFixed(0)} of ${(dl.total / 1e6).toFixed(0)} MB)`),
      el('progress', { max: 1, value: dl.total ? dl.done / dl.total : 0, 'aria-label': 'Download progress' }),
      el('button', { class: 'btn', type: 'button', onclick: () => invoke('cancel_download').catch(showError) }, 'Cancel')));
  } else {
    if (dl && dl.result !== null) {
      parts.push(el('p', { class: dl.result ? 'warn' : 'quiet', role: 'status' }, dl.result ? `The model was not saved. ${dl.result}` : `${dl.name} is ready.`));
    }
    parts.push(el('div', { class: 'row' },
      el('button', { class: 'btn', type: 'button', onclick: async () => {
        const path = await dialog.open({ title: 'Choose a whisper.cpp model file', filters: [{ name: 'whisper.cpp model', extensions: ['bin'] }] });
        if (!path) return;
        await invoke('clear_download').catch(() => {});
        try { await invoke('use_model_file', { path }); refreshVoice(); } catch (e) { showError(e); }
      } }, 'Use a model file I already have…'),
      // The phone has no folder to show (Jared, 2026-10-20).
      PHONE ? null : el('button', { class: 'btn', type: 'button', onclick: () => invoke('show_models_folder').catch(showError) }, 'Show the models folder')));
    parts.push(el('p', { class: 'hint' }, `A one-time download from the whisper.cpp page on Hugging Face, checked against the checksum published there. It is kept on ${HERE} for every Journey, not in the Journey or its backups.`));
    if (PHONE) parts.push(el('p', { class: 'hint' }, 'The screen stays on while a model downloads or a transcript is made, since the phone stops the work when it goes dark. Download on Wi-Fi if you can. base.en is faster with more wrong words; if transcripts are too slow on this phone, you can record here and make the transcripts on the PC after moving the Journey.'));
    else parts.push(el('p', { class: 'hint' }, 'The models folder also holds transcription-log.txt, a note of how each transcript went.'));
  }
  const auto = el('input', { type: 'checkbox', id: 's-auto', checked: v.auto });
  auto.addEventListener('change', async () => {
    await invoke('set_auto_transcribe', { on: auto.checked }).catch(showError);
    toast(auto.checked ? 'New recordings are transcribed on their own.' : 'Transcripts are made only when you ask.');
  });
  parts.push(el('label', { class: 'switch', for: 's-auto' }, auto, 'Make a transcript after each recording'));
  // Keep focus where it was when the box is drawn again.
  const focusedText = document.activeElement && box.contains(document.activeElement) ? document.activeElement.textContent : null;
  box.replaceChildren(...parts);
  if (focusedText) [...box.querySelectorAll('button')].find((b) => b.textContent === focusedText)?.focus();
}

let lastSpeech = '';
voiceWatchers.add((v) => {
  // Draw again only when something changed, so ticking a choice isn't undone.
  const key = JSON.stringify([v.model, v.models, v.downloading, v.auto]);
  if (key !== lastSpeech || current !== 'settings') { lastSpeech = current === 'settings' ? key : ''; paintSpeech(v); }
});
extraLoaders.settings = async () => { await loadSettings(); lastSpeech = ''; await refreshVoice(); };

refreshVoice();
