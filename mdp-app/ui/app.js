// App shell. Plain JavaScript, no build step.
// Every screen reads from the Rust side through `invoke`.
'use strict';

const T = window.__TAURI__;
// Android (sprint 9): the phone layout, and pickers that say "cancelled" by
// failing instead of returning nothing, as they do on Windows.
const PHONE = /Android/i.test(navigator.userAgent);
const HERE = PHONE ? 'this phone' : 'this PC';
// The phone plays videos and recordings, and shows the preview, through a
// small server inside the app that hands files out in pieces as a player
// asks; Android doesn't play videos from the asset protocol (Jared, 2026-10-20).
let MEDIA_BASE = null;
const fileUrl = (path) => MEDIA_BASE
  ? MEDIA_BASE + '/' + path.replace(/^\/+/, '').split('/').map(encodeURIComponent).join('/')
  : T.core.convertFileSrc ? T.core.convertFileSrc(path) : path;
// (T.dialog itself can't be replaced: Tauri defines it read-only.)
if (PHONE) document.documentElement.classList.add('phone');
const quiet = (f) => (o) => f(o).catch((e) => { if (/cancel/i.test(String(e))) return null; throw e; });
const dialog = PHONE ? { ...T.dialog, open: quiet(T.dialog.open), save: quiet(T.dialog.save) } : T.dialog;
// Every error a command returns goes into the app's log too (thumbnails that
// can't be made, and dates checked while they are still being typed, are
// expected and left out).
const logEvent = (area, text) => T.core.invoke('log_event', { area, text: String(text) }).catch(() => {});
const invoke = (cmd, args) => T.core.invoke(cmd, args).catch((e) => {
  if (cmd !== 'thumbnail' && cmd !== 'read_date') logEvent('screen', `ERROR ${cmd}: ${e}`);
  throw e;
});
window.addEventListener('error', (e) => logEvent('screen', `ERROR script: ${e.message} at ${e.filename}:${e.lineno}`));
window.addEventListener('unhandledrejection', (e) => logEvent('screen', `ERROR unhandled: ${e.reason}`));
const $ = (s, root = document) => root.querySelector(s);
const $$ = (s, root = document) => [...root.querySelectorAll(s)];

// Select all rides along in the phone's sticky selection bar.
if (PHONE) $('#sel-bar').append($('.selall'));

function el(tag, attrs = {}, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    if (k === 'class') e.className = v;
    else if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
    else e.setAttribute(k, v === true ? '' : v);
  }
  for (const k of kids.flat()) if (k !== null && k !== undefined) e.append(k);
  return e;
}

let toastTimer;
function toast(text, action) {
  const t = $('#toast');
  t.replaceChildren(el('span', {}, text));
  if (action) t.append(el('button', { class: 'link', onclick: action.run }, action.label));
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), 6000);
}

function showError(e) {
  toast(String(e));
}

// ---------- First run ----------

let chosenFolder = null;

async function updateWhere() {
  const name = $('#journey-name').value.trim();
  let place;
  if (chosenFolder) place = { folder: chosenFolder, warning: await invoke('check_folder', { folder: chosenFolder }) };
  else place = await invoke('suggest_folder', { name: name || 'My Journey' });
  $('#where').textContent = PHONE ? `It will be kept ${place.shown.replace(/^On this phone, in /, 'on this phone, in ')}.` : `It will be kept in ${place.folder}`;
  $('#where-warn').hidden = !place.warning;
  $('#where-warn').textContent = place.warning || '';
}

function showFirstRun(problem) {
  $('#app').hidden = true;
  $('#firstrun').hidden = false;
  $('#create-error').hidden = !problem;
  $('#create-error').textContent = problem || '';
  chosenFolder = null;
  $('#journey-name').value = '';
  updateWhere();
  $('#journey-name').focus();
}

$('#journey-name').addEventListener('input', () => { if (!chosenFolder) updateWhere(); });

$('#create-form').addEventListener('submit', async (ev) => {
  ev.preventDefault();
  const name = $('#journey-name').value.trim();
  if (!name) {
    $('#create-error').textContent = 'Please give the Journey a name.';
    $('#create-error').hidden = false;
    $('#journey-name').focus();
    return;
  }
  try {
    const home = await invoke('create_journey', { name, folder: chosenFolder ? `${chosenFolder}` : null });
    enterApp(home);
  } catch (e) {
    $('#create-error').textContent = String(e);
    $('#create-error').hidden = false;
  }
});

$('#choose-folder').addEventListener('click', async () => {
  const dir = await dialog.open({ directory: true, title: 'Choose an empty folder for the Journey' });
  if (!dir) return;
  const name = $('#journey-name').value.trim() || 'My Journey';
  chosenFolder = `${dir}${dir.includes('\\') ? '\\' : '/'}${name.replace(/[<>:"/\\|?*]/g, '-')}`;
  updateWhere();
});

async function pickAndOpen() {
  const dir = await dialog.open({ directory: true, title: 'Choose the Journey folder (the one with catalog.sqlite in it)' });
  if (!dir) return;
  try { enterApp(await invoke('open_journey', { folder: dir })); } catch (e) {
    if (!$('#firstrun').hidden) { $('#create-error').textContent = String(e); $('#create-error').hidden = false; } else showError(e);
  }
}
$('#open-existing').addEventListener('click', pickAndOpen);
$('#open-sample').addEventListener('click', async () => {
  try { enterApp(await invoke('open_sample')); } catch (e) { $('#create-error').textContent = String(e); $('#create-error').hidden = false; }
});

// ---------- Navigation ----------

let current = 'home';
let currentId = null;
const backStack = [];
// Screens added by later sprints register their loaders here (people.js).
const extraLoaders = {};

function go(pane, { push = true, focus = true, id = null } = {}) {
  if (push && (pane !== current || id !== currentId)) {
    backStack.push({ pane: current, id: currentId });
    if (PHONE) history.pushState({ pane }, ''); // so the phone's Back button comes back here
  }
  if (pane !== 'memory' && typeof memoryNav !== 'undefined') memoryNav = null; // Previous and Next belong to the list just left
  current = pane;
  currentId = id;
  if (typeof typed !== 'undefined') typed = null; // the new page registers its own form (people.js)
  for (const p of $$('.pane')) p.hidden = p.dataset.pane !== pane;
  const section = { person: 'people', family: 'people', places: 'people', place: 'people', event: 'people', story: 'stories', memory: 'stories' }[pane] || pane;
  for (const b of $$('.navbtn')) {
    if (b.dataset.go === section) b.setAttribute('aria-current', 'page');
    else b.removeAttribute('aria-current');
  }
  const loaders = { home: loadHome, people: loadPeople, waiting: loadWaiting, settings: loadSettings, ...extraLoaders };
  const done = loaders[pane] ? loaders[pane](id).catch(showError) : Promise.resolve();
  if (pane !== 'home' && pane !== 'waiting') refreshBadge();
  done.then(() => {
    const h = $(`.pane[data-pane="${pane}"] h1`);
    if (h && focus) { h.tabIndex = -1; h.focus({ preventScroll: true }); }
  });
  $('#main').scrollTop = 0;
  return done;
}

function goBack() {
  if (PHONE && backStack.length) { history.back(); return; } // the popstate below does the rest
  const prev = backStack.pop() || { pane: 'home', id: null };
  go(prev.pane, { push: false, id: prev.id });
}

// The phone's Back button (sprint 9): it closes the open dialog or the
// preview first, then goes back a screen; on the first screen it leaves the
// app as usual. Each screen and dialog adds a step to the web view's history.
let skipPops = 0;
if (PHONE) {
  const showModal = HTMLDialogElement.prototype.showModal;
  HTMLDialogElement.prototype.showModal = function () {
    showModal.call(this);
    history.pushState({ dialog: true }, '');
    this.addEventListener('close', () => {
      // Closed with its own buttons: take its history step back off.
      if (this.byBack) this.byBack = false;
      else { skipPops++; history.back(); }
    }, { once: true });
  };
  window.addEventListener('popstate', () => {
    if (skipPops) { skipPops--; return; }
    if (!$('#phone-preview').hidden) return; // export.js closes it
    const open = $$('dialog[open]').pop();
    if (open) { open.byBack = true; open.close(); return; }
    const prev = backStack.pop();
    if (prev) go(prev.pane, { push: false, id: prev.id });
  });
  // Keep the field being typed in above the on-screen keyboard.
  document.addEventListener('focusin', (e) => {
    if (e.target.matches('input, textarea, select')) setTimeout(() => e.target.scrollIntoView({ block: 'center', behavior: 'smooth' }), 300);
  });
}

document.addEventListener('click', (ev) => {
  const b = ev.target.closest('[data-go]');
  if (b) { go(b.dataset.go); return; }
  const later = ev.target.closest('[data-later]');
  if (later) toast(later.dataset.later);
});
$('#back').addEventListener('click', goBack);

function enterApp(home) {
  $('#firstrun').hidden = true;
  $('#app').hidden = false;
  renderHome(home);
  go('home', { push: false, focus: false });
  invoke('get_settings').then((st) => { suggestOn = st.suggestions; }).catch(() => {});
  if (typeof offerBackupForNewVersion === 'function') offerBackupForNewVersion().catch(() => {});
}

// The sidebar's waiting count (sprint 7: it follows every move, not just Home).
function paintBadge(n) {
  $('#wbadge').hidden = n === 0;
  $('#wbadge').textContent = n;
  $('#wbadge').setAttribute('aria-label', `${n} waiting`);
}
async function refreshBadge() {
  const list = await invoke('raw_content', { kind: 'waiting' }).catch(() => null);
  if (list) paintBadge(list.length);
}

// ---------- Home ----------

// Thumbnail size per screen: small, medium or large, remembered on this device (sprint 8).
function thumbSize(pane) {
  const key = `mdp.thumbs.${pane}`;
  let size = 'm';
  try { size = localStorage.getItem(key) || 'm'; } catch { /* private window */ }
  const box = $(`section.pane[data-pane="${pane}"]`);
  box.dataset.thumbs = size;
  const id = `thumbs-${pane}`;
  const sel = el('select', { id }, ...[['s', 'Small'], ['m', 'Medium'], ['l', 'Large']].map(([v, t]) => el('option', { value: v, selected: v === size }, t)));
  sel.addEventListener('change', () => {
    box.dataset.thumbs = sel.value;
    try { localStorage.setItem(key, sel.value); } catch { /* not kept */ }
  });
  return el('div', { class: 'field thumbsize' }, el('label', { for: id }, 'Picture size'), sel);
}
for (const pane of ['stories', 'waiting', 'people']) $(`section.pane[data-pane="${pane}"] .add-row`)?.append(thumbSize(pane));

// Waiting as a list or as tiles (sprint 12, Jared). Same items and actions either way.
function viewSwitch(pane) {
  const key = `mdp.view.${pane}`;
  let view = 'list';
  try { view = localStorage.getItem(key) || 'list'; } catch { /* private window */ }
  const box = $(`section.pane[data-pane="${pane}"]`);
  box.dataset.view = view;
  const id = `view-${pane}`;
  const sel = el('select', { id }, ...[['list', 'List'], ['tiles', 'Tiles']].map(([v, t]) => el('option', { value: v, selected: v === view }, t)));
  sel.addEventListener('change', () => {
    box.dataset.view = sel.value;
    try { localStorage.setItem(key, sel.value); } catch { /* not kept */ }
  });
  return el('div', { class: 'field viewswitch' }, el('label', { for: id }, 'Show as'), sel);
}
{
  const size = $('section.pane[data-pane="waiting"] .thumbsize');
  if (size) {
    const tools = el('div', { class: 'viewtools' });
    size.before(tools);
    tools.append(viewSwitch('waiting'), size);
  }
}

function plural(n, one, many) { return `${n} ${n === 1 ? one : many}`; }

// One cover picture for each memory (sprint 12): its chosen cover, or else
// its first picture or video; a recording or note shows its kind's letter.
async function memoryCovers() {
  const rows = await invoke('memory_list').catch(() => []);
  return new Map(rows.map((m) => [m.id, { cover_id: m.cover_id, cover_kind: m.cover_kind, words: m.words, who: m.who }]));
}
const withCovers = (list, covers) => list.map((m) => ({ ...m, cover_id: null, cover_kind: null, words: '', ...covers.get(m.id) }));
// Home lists stories and memories together: a box for each, filled once the covers are known.
const coverSlot = (id) => el('span', { class: 'thumb small', 'aria-hidden': 'true', 'data-cover-of': id });
async function fillCoverSlots(root) {
  const covers = await memoryCovers();
  for (const box of root.querySelectorAll('[data-cover-of]')) {
    const c = covers.get(box.dataset.coverOf);
    if (!c) { box.remove(); continue; }
    box.replaceWith(storyThumb({ ...c }, 'small'));
  }
}

function renderHome(h) {
  document.title = `${h.name} - Modern Day Pioneer`;
  $('#journey-title').textContent = h.name;
  $('#journey-intro').textContent = h.introduction || 'Add an introduction later; it opens the exported history.';
  $('#readonly').hidden = !h.read_only;
  const c = h.counts;
  // Each count opens the screen where those are changed (sprint 7).
  $('#counts').replaceChildren(
    ...[[c.stories, 'story', 'stories', 'stories'], [c.memories, 'memory', 'memories', 'stories'], [c.people, 'person', 'people', 'people'],
      [c.pets, 'pet', 'pets', 'people'], [c.families, 'family', 'families', 'people'], [c.places, 'place', 'places', 'places']]
      .map(([n, one, many, to]) => el('button', { class: 'link count', type: 'button', 'data-go': to }, el('b', {}, String(n)), ` ${n === 1 ? one : many}`)));
  $('#wcount').textContent = plural(c.waiting, 'picture or recording', 'pictures and recordings');
  $('#waiting-empty').hidden = c.waiting > 0;
  paintBadge(c.waiting);
  // Memories to finish: placeholders first, then memories that could use
  // more unless suggestions are off.
  const could = suggestOn ? h.could_add : [];
  const finishLine = (m, note, chipEl) => el('li', {}, coverSlot(m.id),
    el('span', { class: 't' }, el('button', { class: 'link', type: 'button', onclick: () => openItem(m.id) }, m.name), note ? el('small', {}, note) : null),
    chipEl);
  $('#to-finish').replaceChildren(
    ...h.to_finish.map((m) => finishLine(m, m.note, el('span', { class: 'chip needs' }, 'Needs a photo, recording or note'))),
    ...could.map((m) => finishLine(m, `Could add ${m.note}`, el('span', { class: 'acts' }, el('span', { class: 'chip add' }, 'Could use more'),
      el('button', { class: 'btn', type: 'button', onclick: () => setFinished(m.id, m.name, true) }, 'Finished', el('span', { class: 'sr' }, ` ${m.name}`))))));
  const more = (h.to_finish_total - h.to_finish.length) + (suggestOn ? h.could_add_total - h.could_add.length : 0);
  $('#to-finish-more').hidden = more <= 0;
  $('#to-finish-more').textContent = `${plural(more, 'more memory', 'more memories')} on the Stories & Memories screen`;
  $('#to-finish').hidden = h.to_finish.length + could.length === 0;
  $('#to-finish-empty').hidden = h.to_finish.length + could.length > 0 || c.memories === 0;
  $('#to-finish-empty').textContent = suggestOn ? 'Every memory has something in it, and a date, place, people, caption and story.' : 'Every memory has something in it.';
  $('#to-finish-none').hidden = c.memories > 0;
  renderShelf(h.shelf);
  const nf = h.featured.length;
  $('#featured').replaceChildren(...h.featured.map((f, i) => el('li', {}, coverSlot(f.id),
    el('span', { class: 't' }, el('button', { class: 'link', onclick: () => openItem(f.id) }, f.name), el('small', {}, f.note)),
    i > 0 ? el('button', { class: 'btn', type: 'button', onclick: () => moveFeatured(f.id, -1) }, 'Up', el('span', { class: 'sr' }, ` ${f.name}`)) : null,
    i < nf - 1 ? el('button', { class: 'btn', type: 'button', onclick: () => moveFeatured(f.id, 1) }, 'Down', el('span', { class: 'sr' }, ` ${f.name}`)) : null)));
  $('#featured-panel').hidden = h.featured.length === 0;
  homeWaiting().catch(() => {});
  if (h.to_finish.length + could.length + nf) fillCoverSlots($('#featured').closest('[data-pane]')).catch(() => {});
}

// The preview card on Home: a small bookshelf drawn from the same stories
// and covers the export's bookshelf uses, redrawn whenever Home is shown, so
// it costs no extra export or screenshot (sprint 7).
let suggestOn = true;
function renderShelf(shelf) {
  $('#shelf-mini').replaceChildren(...shelf.map((b) => {
    const cover = el('span', { class: 'cover', 'data-thumb': b.note || null }, b.note ? '' : b.name[0] || '');
    return el('li', { class: 'book' }, cover, el('span', { class: 'spine' }, b.name));
  }));
  $('#shelf-mini').hidden = shelf.length === 0;
  $('#shelf-empty').hidden = shelf.length > 0;
  for (const b of shelf) if (b.note) loadThumb({ id: b.note, kind: 'picture' });
}

// Home shows the first few waiting items, each openable large or made into a
// memory, with the full Waiting screen a click away (sprint 5 feedback).
const HOME_WAITING = 5;
async function homeWaiting() {
  const list = await invoke('raw_content', { kind: 'waiting' });
  const shown = list.slice(0, HOME_WAITING);
  $('#waiting-short').replaceChildren(...shown.map((it, i) => {
    const img = el('span', { class: 'thumb small', 'data-thumb': it.id, 'aria-hidden': 'true' }, KIND_WORD[it.kind]?.[0] || '?');
    return el('li', {}, img,
      el('span', { class: 't' }, el('button', { class: 'link', type: 'button', onclick: () => openViewer(shown, i, '', waitingActions) }, it.name),
        el('small', {}, `${KIND_WORD[it.kind] || it.kind} · ${it.date || 'date not known'}`)),
      el('button', { class: 'btn', type: 'button', onclick: () => makeMemoryOf(it) }, 'Make a memory…', el('span', { class: 'sr' }, ` from ${it.name}`)));
  }));
  $('#waiting-short').hidden = shown.length === 0;
  $('#waiting-more').hidden = list.length <= HOME_WAITING;
  $('#waiting-more').textContent = `See all ${list.length} on the Waiting screen`;
  for (const it of shown) loadThumb(it);
}

// One waiting item into a new memory (or one already there), from Home or Waiting.
async function makeMemoryOf(it) {
  const made = await putIntoMemory([it.id], it.title || it.name);
  if (!made) return null;
  ticked.delete(it.id);
  refreshAfterWaiting();
  return made;
}

async function loadHome() { renderHome(await invoke('home')); }

// ---------- Lists ----------

function initials(name) {
  return name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0].toUpperCase()).join('');
}

async function loadPeople() {
  const groups = await invoke('people');
  $('#family-list').replaceChildren(...groups.map((g) => el('section', { class: 'fam', 'aria-label': g.name },
    el('h2', { class: 'h3like' }, g.name),
    el('div', { class: 'people' }, ...g.members.map((p) => el('button', { class: 'person', onclick: () => openItem(p.id) },
      el('span', { class: 'avatar', 'aria-hidden': 'true' }, initials(p.name)),
      el('span', {}, p.name, p.note ? el('small', {}, ` ${p.note}`) : null)))))));
  $('#people-empty').hidden = groups.length > 0;
}

// ---------- Waiting: bring in, set aside, delete ----------

let showing = 'waiting';
const KIND_WORD = { picture: 'Picture', 'voice recording': 'Recording', video: 'Video', text: 'Text' };
const thumbs = new Map();

async function loadWaiting() {
  const [list, aside, missing, groups] = await Promise.all([
    invoke('raw_content', { kind: 'waiting' }), invoke('raw_content', { kind: 'set aside' }), invoke('missing_files'),
    invoke('waiting_groups').catch(() => [])]);
  waitingLists = { list, aside, missing, groups };
  paintWaiting();
}

// Draws the lists already fetched; sorting and filtering redraw without asking the core again.
let waitingLists = { list: [], aside: [], missing: [], groups: [] };
function paintWaiting() {
  const { list, aside, missing } = waitingLists;
  const groups = waitingLists.groups.filter((g) => !skippedGroups.has(g.key));
  $('#n-waiting').textContent = `(${list.length})`;
  paintBadge(list.length);
  $('#n-aside').textContent = `(${aside.length})`;
  $('#n-groups').textContent = `(${groups.length})`;
  for (const t of $$('.tab')) t.setAttribute('aria-pressed', String(t.dataset.state === showing));
  const grouped = showing === 'groups';
  for (const id of ['waiting-tools', 'sel-bar', 'waiting-list', 'waiting-list-empty', 'waiting-count']) if (grouped) $(`#${id}`).hidden = true;
  $('.selall').hidden = grouped;
  $('#group-list').hidden = !grouped;
  if (grouped) { paintGroups(groups); return; }
  $('#waiting-tools').hidden = false;
  const all = showing === 'waiting' ? list : aside;
  const items = sortWaiting(all);
  shownItems = items;
  const filtered = items.length !== all.length;
  $('#waiting-count').hidden = !filtered;
  $('#waiting-count').textContent = `${plural(items.length, 'item', 'items')} of ${all.length}`;
  for (const id of [...ticked]) if (!items.some((it) => it.id === id)) ticked.delete(id);
  $('#sel-bar').hidden = showing !== 'waiting' || items.length === 0;
  paintTicked();
  $('#waiting-list').replaceChildren(...items.map(rawRow));
  $('#waiting-list').hidden = items.length === 0;
  $('#waiting-list-empty').hidden = items.length > 0;
  $('#waiting-list-empty').textContent = all.length && !items.length ? 'Nothing matches. Change or clear the choices above.' : showing === 'waiting'
    ? 'Nothing is waiting. Pictures and recordings you bring in wait here until they go into a memory.'
    : 'Nothing is set aside.';
  $('#missing-note').hidden = missing.length === 0;
  $('#missing-note').textContent = missing.length
    ? `${plural(missing.length, 'file is', 'files are')} missing from the Journey folder. If you moved ${missing.length === 1 ? 'it' : 'them'}, put ${missing.length === 1 ? 'it' : 'them'} back anywhere inside the Journey folder and the app will find ${missing.length === 1 ? 'it' : 'them'}.`
    : '';
  for (const it of items) loadThumb(it);
}

// Sort and filter the Waiting list (after sprint 8, like Stories & Memories). The
// core sends items newest added first, so that order is kept for "new" and "old".
function sortWaiting(list) {
  const sort = $('#wf-sort').value;
  const text = $('#wf-text').value.trim().toLowerCase();
  const kind = $('#wf-kind').value;
  const noDate = $('#wf-nodate').checked;
  const rows = list.filter((it) => (!text || it.name.toLowerCase().includes(text))
    && (!kind || it.kind === kind) && (!noDate || !it.date));
  const name = (a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
  if (sort === 'old') rows.reverse();
  else if (sort === 'name') rows.sort(name);
  else if (sort === 'date') rows.sort((a, b) => (a.date_sort ? 0 : 1) - (b.date_sort ? 0 : 1) || (a.date_sort || '').localeCompare(b.date_sort || '') || name(a, b));
  return rows;
}
for (const id of ['wf-sort', 'wf-kind', 'wf-nodate']) $(`#${id}`).addEventListener('change', paintWaiting);
$('#wf-text').addEventListener('input', paintWaiting);
$('#waiting-tools').addEventListener('submit', (e) => e.preventDefault());

// Items selected on the Waiting screen, to put into a memory together (sprint 4).
let shownItems = [];
const ticked = new Set();

function paintTicked() {
  const n = ticked.size;
  $('#sel-count').textContent = n ? `${plural(n, 'item', 'items')} selected.` : PHONE ? 'Select items, or tap one to see it large and act on it.' : 'Select items to put them into a memory together. Click an item to see it large.';
  $('#sel-memory').disabled = n === 0;
  $('#sel-person').disabled = n === 0;
  $('#sel-more').disabled = n === 0;
  // The Select all box at the top of the list (sprint 7): checked when all
  // are selected, half-checked when some are.
  const all = $('#sel-all');
  all.checked = n > 0 && n === shownItems.length;
  all.indeterminate = n > 0 && n < shownItems.length;
}

function tickedInOrder() { return shownItems.filter((it) => ticked.has(it.id)); }

$('#sel-all').addEventListener('change', () => {
  const all = $('#sel-all').checked;
  ticked.clear();
  if (all) for (const it of shownItems) ticked.add(it.id);
  for (const c of $$('#waiting-list input[type=checkbox]')) c.checked = all;
  paintTicked();
});
$('#sel-memory').addEventListener('click', async () => {
  const items = tickedInOrder();
  if (!items.length) return;
  const made = await putIntoMemory(items.map((i) => i.id), items[0].title || items[0].name);
  if (made) { ticked.clear(); loadWaiting(); }
});
$('#sel-person').addEventListener('click', async () => {
  const items = tickedInOrder();
  if (!items.length) return;
  const made = await assignToSomeone(items.map((i) => i.id));
  if (made) { ticked.clear(); loadWaiting(); }
});

// ---------- Waiting in suggested groups (sprint 10) ----------

// A few groups at a time, so a big import turns into a handful of decisions.
const GROUPS_AT_ONCE = 3;
let groupsShown = GROUPS_AT_ONCE;
const skippedGroups = new Set();

function paintGroups(groups) {
  const byId = new Map(waitingLists.list.map((it) => [it.id, it]));
  const shown = groups.slice(0, groupsShown);
  const box = $('#group-list');
  if (!groups.length) {
    box.replaceChildren(el('p', { class: 'empty' }, waitingLists.list.length
      ? 'No groups to suggest. Items taken the same day, in the same place or from the same folder are grouped here.'
      : 'Nothing is waiting.'));
    return;
  }
  box.replaceChildren(
    el('p', { class: 'quiet' }, `${plural(groups.length, 'group', 'groups')} of items that seem to belong together. Each can become one memory in one step; you can change anything later.`),
    ...shown.map((g) => groupCard(g, byId)),
    groups.length > shown.length ? el('button', { class: 'btn', type: 'button', onclick: () => { groupsShown += GROUPS_AT_ONCE; paintWaiting(); } },
      `Show more groups (${groups.length - shown.length} more)`) : '');
  for (const g of shown) for (const id of g.items) { const it = byId.get(id); if (it) loadThumb(it); }
}

function groupCard(g, byId) {
  const items = g.items.map((id) => byId.get(id)).filter(Boolean);
  const hid = nextId('g');
  const title = textField('Title of the memory', g.title);
  const useDate = g.date ? el('input', { type: 'checkbox', id: nextId('gd'), checked: true }) : null;
  const usePlace = g.place_id ? el('input', { type: 'checkbox', id: nextId('gp'), checked: true }) : null;
  const strip = el('ul', { class: 'strip', 'aria-label': 'Items in this group' }, ...items.slice(0, 12).map((it, i) =>
    el('li', {}, el('button', { class: 'link', type: 'button', title: it.name, onclick: () => openViewer(items, i, g.label, waitingActions) },
      el('span', { class: 'thumb', 'data-thumb': it.id }, KIND_WORD[it.kind]?.[0] || '?'), el('span', { class: 'sr' }, it.name)))),
    items.length > 12 ? el('li', { class: 'quiet' }, `and ${items.length - 12} more`) : null);
  const done = () => { skippedGroups.add(g.key); refreshAfterWaiting(); };
  const make = async () => {
    const name = title.input.value.trim();
    if (!name) { toast('Type a title for the memory.'); title.input.focus(); return; }
    try {
      const made = await act(invoke('memory_from_group', { name, content: g.items, date: useDate?.checked ? g.date : null, place: usePlace?.checked ? g.place_id : null }),
        `${name} is made with ${plural(g.items.length, 'item', 'items')}.`);
      done();
      await whatNext(made);
    } catch { /* shown */ }
  };
  return el('section', { class: 'wgroup', 'aria-labelledby': hid },
    el('h2', { id: hid }, g.label),
    el('p', { class: 'why' }, `${g.reason} · ${plural(g.items.length, 'item', 'items')}`),
    strip, title,
    el('div', { class: 'facts' },
      useDate ? el('label', { class: 'switch', for: useDate.id }, useDate, `Date: ${g.date_words} (${g.date_source})`) : null,
      usePlace ? el('label', { class: 'switch', for: usePlace.id }, usePlace, `Place: ${g.place_name}`) : null),
    el('div', { class: 'btns' },
      el('button', { class: 'btn primary', type: 'button', onclick: make }, 'Make a memory', el('span', { class: 'sr' }, ` of ${g.label}`)),
      el('button', { class: 'btn', type: 'button', onclick: async () => {
        try { await act(invoke('memory_each', { content: g.items }), `${plural(g.items.length, 'memory is', 'memories are')} made.`); done(); } catch { /* shown */ }
      } }, 'Each its own memory', el('span', { class: 'sr' }, `: ${g.label}`)),
      el('button', { class: 'btn', type: 'button', onclick: async () => {
        try { await invoke('set_state', { ids: g.items, to: 'set aside' }); toast(`${plural(g.items.length, 'item is', 'items are')} set aside.`, { label: 'Undo', run: undoNow }); done(); } catch (e) { showError(e); }
      } }, 'Set aside', el('span', { class: 'sr' }, `: ${g.label}`)),
      el('button', { class: 'btn', type: 'button', onclick: () => { skippedGroups.add(g.key); paintWaiting(); } }, 'Skip for now', el('span', { class: 'sr' }, `: ${g.label}`))));
}

function rawRow(it, index) {
  const img = el('span', { class: 'thumb', 'data-thumb': it.id, 'aria-hidden': 'true' }, KIND_WORD[it.kind]?.[0] || '?');
  const tick = showing === 'waiting' ? el('input', { type: 'checkbox', checked: ticked.has(it.id), 'aria-label': `Select ${it.name}`,
    onchange: (e) => { if (e.target.checked) ticked.add(it.id); else ticked.delete(it.id); paintTicked(); } }) : null;
  const other = showing === 'waiting'
    ? el('button', { class: 'btn', type: 'button', onclick: () => moveTo(it, 'set aside') }, 'Set aside', el('span', { class: 'sr' }, `: ${it.name}`))
    : el('button', { class: 'btn', type: 'button', onclick: () => moveTo(it, 'waiting') }, 'Back to waiting', el('span', { class: 'sr' }, `: ${it.name}`));
  return el('li', {},
    tick,
    img,
    el('span', { class: 't' }, el('button', { class: 'link', type: 'button', onclick: () => openViewer(shownItems, index, '', waitingActions) }, it.name),
      el('small', {}, `${KIND_WORD[it.kind] || it.kind} · ${it.date || (it.name_date ? `${it.name_date} (from the file name)` : 'date not known')}`)),
    // The phone keeps rows short: the actions are on the selection bar and
    // on the item's own large view (Jared, 2026-10-19).
    PHONE ? null : rowActions(it, other));
}

// Waiting rows (sprint 12, Jared): one split button. A click makes a
// memory; the arrow, a long press or a right-click opens the other actions.
function rowActions(it, other) {
  if (showing !== 'waiting') {
    return el('span', { class: 'rowbtns' }, other,
      el('button', { class: 'btn', type: 'button', onclick: () => removeCopy(it) }, 'Remove from Journey…', el('span', { class: 'sr' }, `: ${it.name}`)));
  }
  const menu = el('div', { class: 'menu right rowmenu', role: 'menu', hidden: true });
  const toggle = el('button', { class: 'btn split-more', type: 'button', 'aria-haspopup': 'menu', 'aria-expanded': 'false', 'aria-label': `More for ${it.name}` }, '▾');
  const setOpen = (open, focusToggle = false) => {
    for (const m of $$('.rowmenu:not([hidden])')) if (m !== menu) { m.hidden = true; m.previousElementSibling?.querySelector('.split-more')?.setAttribute('aria-expanded', 'false'); }
    menu.hidden = !open;
    toggle.setAttribute('aria-expanded', String(open));
    if (open) menu.querySelector('[role=menuitem]')?.focus();
    else if (focusToggle) toggle.focus();
  };
  const items = waitingActions(it).slice(1).map(([label, run]) => el('button', { type: 'button', role: 'menuitem',
    onclick: async () => { setOpen(false); try { await run(); } catch (e) { showError(e); } } }, label));
  menu.replaceChildren(...items);
  menu.addEventListener('keydown', (e) => {
    const list = $$('[role=menuitem]', menu);
    const at = list.indexOf(document.activeElement);
    if (e.key === 'ArrowDown') { e.preventDefault(); list[(at + 1) % list.length].focus(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); list[(at - 1 + list.length) % list.length].focus(); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); setOpen(false, true); }
    else if (e.key === 'Tab') setOpen(false);
  });
  let pressed = null;
  let longDone = false;
  const main = el('button', { class: 'btn split-main', type: 'button',
    onclick: () => { if (longDone) { longDone = false; return; } makeMemoryOf(it); },
    onpointerdown: (e) => { if (e.button !== 0) return; longDone = false; pressed = setTimeout(() => { longDone = true; setOpen(true); }, 550); },
    onpointerup: () => clearTimeout(pressed), onpointerleave: () => clearTimeout(pressed),
    oncontextmenu: (e) => { e.preventDefault(); clearTimeout(pressed); setOpen(true); },
  }, 'Make a memory…', el('span', { class: 'sr' }, ` from ${it.name}`));
  toggle.addEventListener('click', () => setOpen(menu.hidden));
  return el('div', { class: 'menuwrap' }, el('span', { class: 'split', role: 'group', 'aria-label': `Actions for ${it.name}` }, main, toggle), menu);
}

async function loadThumb(it) {
  if (it.kind !== 'picture' && it.kind !== 'video') return;
  let url = thumbs.get(it.id);
  if (url === undefined) {
    url = await invoke('thumbnail', { id: it.id }).catch(() => null);
    thumbs.set(it.id, url);
  }
  // Home and Waiting can both show the item.
  if (url) for (const box of $$(`[data-thumb="${it.id}"]`)) box.replaceChildren(el('img', { src: url, alt: '' }));
}

// The same actions as the Waiting list, on the large preview (sprint 7).
// Each returns true when the item left the list shown.
function waitingActions(it) {
  if (it.state === 'set aside' || (current === 'waiting' && showing === 'set aside')) {
    return [['Back to waiting', () => moveTo(it, 'waiting')], ['Remove from Journey…', () => removeCopy(it)]];
  }
  return [['Make a memory…', async () => !!(await makeMemoryOf(it))],
    ['Assign to an individual…', async () => {
      const made = await assignToSomeone([it.id]);
      if (made) { ticked.delete(it.id); refreshAfterWaiting(); }
      return !!made;
    }],
    ['Set aside', () => moveTo(it, 'set aside')],
    ['Remove from Journey…', () => removeCopy(it)]];
}

function refreshAfterWaiting() {
  if (current === 'waiting') loadWaiting().catch(showError);
  else if (current === 'home') loadHome().catch(showError);
  else refreshBadge();
}

async function moveTo(it, to) {
  try {
    await invoke('set_state', { ids: [it.id], to });
    toast(to === 'set aside' ? `${it.name} is set aside.` : `${it.name} is back in waiting.`, { label: 'Undo', run: undoNow });
    refreshAfterWaiting();
    return true;
  } catch (e) { showError(e); return false; }
}

async function removeCopy(it) {
  const ok = await dialog.ask(
    `Remove ${it.name} from the Journey?\n\nThe Journey's copy is deleted. The original on your device is not touched. You can undo this with Ctrl+Z until you close the app.`,
    { title: 'Remove from Journey', kind: 'warning', okLabel: 'Remove', cancelLabel: 'Keep it' });
  if (!ok) return false;
  try {
    await invoke('delete_raw', { ids: [it.id] });
    toast(`${it.name} is removed from the Journey.`, { label: 'Undo', run: undoNow });
    refreshAfterWaiting();
    return true;
  } catch (e) { showError(e); return false; }
}

for (const t of $$('.tab')) t.addEventListener('click', () => { showing = t.dataset.state; loadWaiting().catch(showError); });

const PICK = [{ name: 'Photos, recordings, videos and text',
  extensions: ['jpg', 'jpeg', 'png', 'gif', 'heic', 'heif', 'avif', 'bmp', 'dng', 'tif', 'tiff', 'webp', 'wav', 'flac', 'mp3', 'm4a', 'opus', 'ogg', 'oga', 'mp4', 'm4v', 'mov', 'txt', 'pdf', 'doc', 'docx', 'odt', 'rtf'] }];

async function addFiles() {
  const picked = await dialog.open({ multiple: true, title: 'Choose photos and recordings to add', filters: PICK });
  if (picked && picked.length) startImport(Array.isArray(picked) ? picked : [picked]);
}
// The phone's own photo picker, for pictures and videos.
async function addMedia() {
  const picked = await dialog.open({ multiple: true, pickerMode: 'media' });
  if (picked && picked.length) startImport(Array.isArray(picked) ? picked : [picked]);
}
async function addFolder() {
  const dir = await dialog.open({ directory: true, title: 'Choose a folder to add (its subfolders are included)' });
  if (dir) startImport([dir]);
}
// One Add button with a small menu: Windows can't pick files and folders in the same dialog.
function closeAddMenu(focusButton) {
  $('#add-menu').hidden = true;
  $('#add-btn').setAttribute('aria-expanded', 'false');
  if (focusButton) $('#add-btn').focus();
}
$('#add-btn').addEventListener('click', () => {
  const open = $('#add-menu').hidden;
  $('#add-menu').hidden = !open;
  $('#add-btn').setAttribute('aria-expanded', String(open));
  if (open) $(PHONE ? '#add-media' : '#add-files').focus();
});
$('#add-menu').addEventListener('keydown', (e) => {
  const items = $$('#add-menu [role="menuitem"]');
  const i = items.indexOf(document.activeElement);
  if (e.key === 'Escape') { e.preventDefault(); closeAddMenu(true); }
  else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    items[(i + (e.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length].focus();
  }
});
document.addEventListener('click', (e) => {
  if (!e.target.closest('.menuwrap')) { closeAddMenu(false); closeSelMenu(false); }
  for (const m of $$('.rowmenu:not([hidden])')) if (!m.parentElement.contains(e.target)) { m.hidden = true; m.parentElement.querySelector('.split-more')?.setAttribute('aria-expanded', 'false'); }
});

// More for the selected items: set aside or remove them all, one undo each.
function closeSelMenu(focusButton) {
  $('#sel-menu').hidden = true;
  $('#sel-more').setAttribute('aria-expanded', 'false');
  if (focusButton) $('#sel-more').focus();
}
$('#sel-more').addEventListener('click', () => {
  const open = $('#sel-menu').hidden;
  $('#sel-menu').hidden = !open;
  $('#sel-more').setAttribute('aria-expanded', String(open));
  if (open) $('#sel-aside').focus();
});
$('#sel-menu').addEventListener('keydown', (e) => {
  const items = $$('#sel-menu [role="menuitem"]');
  const i = items.indexOf(document.activeElement);
  if (e.key === 'Escape') { e.preventDefault(); closeSelMenu(true); }
  else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    items[(i + (e.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length].focus();
  }
});
$('#sel-aside').addEventListener('click', async () => {
  closeSelMenu(false);
  const items = tickedInOrder();
  if (!items.length) return;
  try {
    await invoke('set_state', { ids: items.map((it) => it.id), to: 'set aside' });
    ticked.clear();
    toast(`${plural(items.length, 'item is', 'items are')} set aside.`, { label: 'Undo', run: undoNow });
    refreshAfterWaiting();
  } catch (e) { showError(e); }
});
$('#sel-remove').addEventListener('click', async () => {
  closeSelMenu(false);
  const items = tickedInOrder();
  if (!items.length) return;
  const what = items.length === 1 ? items[0].name : plural(items.length, 'item', 'items');
  const ok = await dialog.ask(
    `Remove ${what} from the Journey?\n\nThe Journey's copies are deleted. The originals on your device are not touched. You can undo this with Ctrl+Z until you close the app.`,
    { title: 'Remove from Journey', kind: 'warning', okLabel: 'Remove', cancelLabel: 'Keep them' });
  if (!ok) return;
  try {
    await invoke('delete_raw', { ids: items.map((it) => it.id) });
    ticked.clear();
    toast(`${what} ${items.length === 1 ? 'is' : 'are'} removed from the Journey.`, { label: 'Undo', run: undoNow });
    refreshAfterWaiting();
  } catch (e) { showError(e); }
});
$('#add-files').addEventListener('click', () => { closeAddMenu(false); addFiles(); });
$('#add-folder').addEventListener('click', () => { closeAddMenu(false); addFolder(); });
$('#add-media').addEventListener('click', () => { closeAddMenu(false); addMedia(); });
$('#home-add').addEventListener('click', () => { go('waiting'); $('#add-btn').click(); });

let polling = null;
async function startImport(paths) {
  try {
    await invoke('start_import', { paths });
    if (current !== 'waiting') go('waiting');
    watchImport();
  } catch (e) { showError(e); }
}

function watchImport() {
  clearInterval(polling);
  const tick = async () => {
    const s = await invoke('import_status').catch(() => null);
    if (!s) { clearInterval(polling); return; }
    $('#job').hidden = !s.running;
    if (s.running) {
      $('#job-msg').textContent = s.message || 'Starting…';
      $('#job-bar').max = Math.max(1, s.total);
      $('#job-bar').value = Math.min(s.done, s.total);
      return;
    }
    clearInterval(polling);
    showResult(s);
    await invoke('clear_import').catch(() => {});
    if (current === 'waiting') loadWaiting().catch(showError);
    else loadHome().catch(showError);
  };
  tick();
  polling = setInterval(tick, 400);
}

function showResult(s) {
  const box = $('#job-done');
  box.hidden = false;
  if (s.error) { $('#job-done-msg').textContent = `The files could not be brought in: ${s.error}`; $('#job-skipped').hidden = true; return; }
  const r = s.summary;
  const parts = [];
  if (r.canceled) parts.push('Canceled. Nothing was added.');
  else {
    if (r.added) parts.push(`Brought the exported history in: ${plural(r.added, 'item', 'items')} added, with ${plural(r.imported, 'file', 'files')}.`);
    else if (r.memories) parts.push(`Added ${plural(r.imported, 'file', 'files')} in ${plural(r.memories, 'new memory', 'new memories')}. Undo takes the whole import back.`);
    else parts.push(r.imported ? `Added ${plural(r.imported, 'item', 'items')}; they are waiting below.` : 'Nothing new was added.');
    if (r.matched) parts.push(`${plural(r.matched, 'person, place or family was', 'people, places and families were')} matched to ones already here.`);
    if (r.duplicates) parts.push(`${plural(r.duplicates, 'item was', 'items were')} already in your Journey, so not added again.`);
    for (const n of r.notes || []) parts.push(n);
    if (r.copies) parts.push(`Made ${plural(r.copies, 'copy', 'copies')} in formats every computer can open.`);
  }
  if (r.note) parts.push(r.note);
  $('#job-done-msg').textContent = parts.join(' ');
  $('#job-skipped').hidden = r.skipped.length === 0;
  $('#job-skipped-sum').textContent = `${plural(r.skipped.length, 'file was', 'files were')} skipped`;
  $('#job-skipped-list').replaceChildren(...r.skipped.map((k) => el('li', {}, el('b', {}, k.item), ` ${k.reason}`)));
}

$('#job-cancel').addEventListener('click', () => { invoke('cancel_import'); $('#job-msg').textContent = 'Canceling…'; });
$('#job-dismiss').addEventListener('click', () => { $('#job-done').hidden = true; $('#add-btn').focus(); });

// Drag and drop onto the window.
T.webview?.getCurrentWebview?.().onDragDropEvent((ev) => {
  const p = ev.payload;
  document.body.classList.toggle('dropping', p.type === 'enter' || p.type === 'over');
  if (p.type === 'drop' && p.paths?.length && !$('#app').hidden) startImport(p.paths);
});

// ---------- One item (from search and lists) ----------

const KIND = { Person: 'Person', Pet: 'Pet', Family: 'Family', Place: 'Place', Event: 'Event', Memory: 'Memory',
  CreativeWork: 'Story', Relationship: 'Relationship', ImageObject: 'Picture', AudioObject: 'Recording', VideoObject: 'Video' };
const LABEL = { description: 'About', birthDateEdtf: 'Born', deathDateEdtf: 'Died', givenName: 'Given name', familyName: 'Family name',
  alternateName: 'Other names', species: 'Kind of animal', breed: 'Breed', joinedDate: 'Joined the family', member: 'Members',
  temporalCoverage: 'Date', dateEdtf: 'Date as known', status: 'Status', contentLocation: 'Place', event: 'Event', people: 'People',
  memories: 'Memories', focus: 'About', startEdtf: 'Starts', endEdtf: 'Ends', location: 'Place', attendee: 'Who took part',
  kind: 'Kind', address: 'Address', geo: 'Coordinates', 'mdp:exact': 'Exact place (hidden in the export)', contents: 'Pictures and recordings' };

const names = new Map();
async function nameOf(ref) {
  const id = String(ref).replace(/^mdpid:/, '');
  if (!names.has(id)) {
    const it = await invoke('item', { id });
    names.set(id, it ? it.name || id : id);
  }
  return names.get(id);
}

async function show(v) {
  if (Array.isArray(v)) return (await Promise.all(v.map(show))).join(', ');
  if (v && typeof v === 'object') {
    if (v.latitude !== undefined) return `${v.latitude}, ${v.longitude}`;
    if (v.individual) return `${await nameOf(v.individual)} (${v.role}${v.certainty === 'probably' ? ', probably' : ''})`;
    return JSON.stringify(v);
  }
  if (typeof v === 'string' && v.startsWith('mdpid:')) return nameOf(v);
  if (typeof v === 'boolean') return v ? 'Yes' : 'No';
  return String(v);
}

// Screens that open one kind of item by its table (people.js fills this).
const openers = {};

async function openItem(id) {
  try {
    const table = await invoke('item_table', { id }).catch(() => null);
    if (table && openers[table]) { openers[table](id); return; }
    const it = await invoke('item', { id });
    if (!it) { toast('That item is no longer in the Journey.'); return; }
    $('#h-item').textContent = it.name || '';
    $('#item-kind').textContent = KIND[it['@type']] || it['@type'];
    const rows = [];
    for (const [k, v] of Object.entries(it)) {
      if (!LABEL[k]) continue;
      rows.push(el('dt', {}, LABEL[k]), el('dd', {}, await show(v)));
    }
    $('#item-fields').replaceChildren(...rows);
    go('item');
  } catch (e) { showError(e); }
}

// ---------- Search ----------

const q = $('#q');
const results = $('#results');
let hits = [];
let active = -1;
let searchSeq = 0;

function closeResults() {
  results.hidden = true;
  q.setAttribute('aria-expanded', 'false');
  q.removeAttribute('aria-activedescendant');
  active = -1;
}

function paintActive() {
  $$('[role=option]', results).forEach((o, i) => o.setAttribute('aria-selected', String(i === active)));
  if (active >= 0) q.setAttribute('aria-activedescendant', `hit-${active}`);
  else q.removeAttribute('aria-activedescendant');
}

q.addEventListener('input', async () => {
  const text = q.value.trim();
  const seq = ++searchSeq;
  if (!text) { closeResults(); return; }
  try {
    const found = await invoke('search', { text });
    // Memories show their cover and date (sprint 13); the covers are read once per search session.
    const covers = found.some((h) => h.kind === 'memory') ? await searchCovers() : null;
    if (seq !== searchSeq) return;
    // Quick-add: a name that isn't there yet can be added from here.
    hits = [...found,
      { create: 'person', text, name: `Add “${text}” as a new person`, kind: '' },
      { create: 'pet', text, name: `Add “${text}” as a new pet`, kind: '' }];
    active = -1;
    results.replaceChildren(...(found.length ? [] : [el('p', {}, `Nothing found for “${text}”.`)]),
      ...hits.map((h, i) => el('div', { role: 'option', id: `hit-${i}`, 'aria-selected': 'false',
        onmousedown: (e) => { e.preventDefault(); pick(i); } },
        h.kind === 'memory' && covers ? storyThumb({ ...covers.get(h.id) }, 'small') : null,
        el('span', {}, h.name), el('small', {}, h.kind === 'memory' && covers && covers.get(h.id)?.date ? `memory · ${covers.get(h.id).date}` : h.kind))));
    results.hidden = false;
    q.setAttribute('aria-expanded', 'true');
  } catch (e) { showError(e); }
});

let coverCache = null;
async function searchCovers() {
  if (!coverCache) {
    coverCache = invoke('memory_list').then((rows) => new Map(rows.map((m) => [m.id, { cover_id: m.cover_id, cover_kind: m.cover_kind, date: m.date }])))
      .catch(() => new Map());
  }
  return coverCache;
}
q.addEventListener('focus', () => { coverCache = null; });

function pick(i) {
  const h = hits[i];
  if (!h) return;
  closeResults();
  q.value = '';
  if (h.create) {
    invoke('add_individual', { kind: h.create, name: h.text })
      .then((id) => { toast(`${h.text} is added.`, { label: 'Undo', run: undoNow }); openItem(id); })
      .catch(showError);
    return;
  }
  openItem(h.id);
}

q.addEventListener('keydown', (e) => {
  if (e.key === 'ArrowDown' && hits.length) { active = (active + 1) % hits.length; paintActive(); e.preventDefault(); }
  else if (e.key === 'ArrowUp' && hits.length) { active = (active - 1 + hits.length) % hits.length; paintActive(); e.preventDefault(); }
  else if (e.key === 'Enter') { pick(active >= 0 ? active : 0); e.preventDefault(); }
  else if (e.key === 'Escape') { if (!results.hidden) e.preventDefault(); closeResults(); }
});
q.addEventListener('blur', () => setTimeout(closeResults, 100));

// ---------- Settings ----------

let settings = null;

async function loadSettings() {
  settings = await invoke('get_settings');
  $('#s-author').value = settings.author;
  $('#s-living').value = settings.living_people;
  $('#s-places').value = settings.exact_places;
  $('#s-front').value = settings.front_door || 'bookshelf';
  $('#s-folder').value = PHONE ? settings.journey_folder_shown : settings.journey_folder;
  $('#s-export').value = PHONE ? settings.export_folder_shown : settings.export_folder;
  $('#s-tips').checked = settings.suggestions;
  suggestOn = settings.suggestions;
  $('#s-folder-warn').hidden = !settings.folder_warning;
  $('#s-folder-warn').textContent = settings.folder_warning || '';
}

$('#settings-form').addEventListener('submit', async (ev) => {
  ev.preventDefault();
  try {
    await invoke('save_settings', { settings: { ...settings,
      author: $('#s-author').value, living_people: $('#s-living').value, exact_places: $('#s-places').value,
      front_door: $('#s-front').value, suggestions: $('#s-tips').checked, export_folder: PHONE ? settings.export_folder : $('#s-export').value } });
    toast('Settings saved.');
    await loadSettings();
  } catch (e) { showError(e); }
});
$('#show-folder').addEventListener('click', () => invoke('show_journey_folder').catch(showError));
$('#s-open').addEventListener('click', pickAndOpen);
$('#s-new').addEventListener('click', () => showFirstRun());
$('#s-export-choose').addEventListener('click', async () => {
  // The phone: exports are made in the app, and a copy goes to the folder picked.
  if (PHONE) {
    try { if (await invoke('pick_export_copy')) { await loadSettings(); toast('Each export will also be copied there.'); } } catch (e) { showError(e); }
    return;
  }
  const dir = await dialog.open({ directory: true, title: 'Choose where exports go' });
  if (dir) $('#s-export').value = dir;
});

// ---------- Keyboard: Ctrl+F searches, Ctrl+Z undoes ----------

async function undoNow() {
  try {
    const undone = await invoke('undo');
    toast(undone ? 'Undone.' : 'Nothing to undo since the Journey was opened.');
    if (undone) go(current, { push: false, focus: false, id: currentId });
  } catch (err) { showError(err); }
}

document.addEventListener('keydown', async (e) => {
  if ($('#app').hidden) return;
  const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName);
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') { e.preventDefault(); q.focus(); q.select(); }
  else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && !typing) {
    e.preventDefault();
    undoNow();
  } else if (e.key === 'Escape' && !e.defaultPrevented) escape(e, typing);
  else if ((e.key === 'ArrowLeft' || e.key === 'ArrowRight') && current === 'memory' && !typing && !e.altKey && !e.ctrlKey && !e.metaKey
    && !/^(AUDIO|VIDEO)$/.test(document.activeElement?.tagName) && !$('dialog[open]')) {
    const b = $(e.key === 'ArrowLeft' ? '#mem-prev' : '#mem-next');
    if (b && !b.disabled) { e.preventDefault(); b.click(); }
  }
});

// Esc (Jared, 2026-10-17): close what is open first; otherwise go back from a
// memory, story, person or other inner screen; otherwise go Home from a main
// screen. Dialogs close themselves (the browser's own Esc), and open lists and
// menus close in their own handlers before this runs.
const MAIN_SCREENS = ['stories', 'people', 'waiting', 'preview', 'settings'];
function escape(e, typing) {
  if ($('dialog[open]')) return;
  const menu = $$('.menu:not([hidden])');
  if (menu.length) { e.preventDefault(); for (const m of menu) m.hidden = true; for (const b of $$('[aria-expanded="true"][aria-haspopup="menu"]')) b.setAttribute('aria-expanded', 'false'); return; }
  // Leaving a box being typed in is the "cancel"; a second Esc moves on.
  if (typing) { e.preventDefault(); document.activeElement.blur(); return; }
  if (current === 'home') return;
  e.preventDefault();
  if (MAIN_SCREENS.includes(current)) go('home');
  else goBack();
}

// ---------- Version (Settings > About, sidebar and window title) ----------

let aboutText = '';
async function showVersion() {
  const a = await invoke('about');
  aboutText = `Modern Day Pioneer ${a.version}, ${a.build}`;
  $('#side-version').textContent = `Version ${a.version}`;
  $('#about-text').textContent = `${aboutText}. Please mention this when you report a problem.`;
  about = a;
  $('#send-feedback').hidden = !a.feedback;
}
let about = null;

// Public test (sprint 12): feedback by email, with the version filled in.
$('#send-feedback').addEventListener('click', () => {
  if (!about?.feedback) return;
  const body = `What happened, or what would you like?\n\n\n---\n${aboutText}\nIf something went wrong, please attach today's log file: Settings > About > Open the log folder.`;
  const url = `mailto:${about.feedback}?subject=${encodeURIComponent(`Modern Day Pioneer feedback (${about.version})`)}&body=${encodeURIComponent(body)}`;
  invoke('open_link', { url }).catch(showError);
});

// "A newer version is available" (sprint 12): asked once a day on opening,
// or with the button. It only links to the download page; nothing installs itself.
function showUpdate(u) {
  $('#update-note').replaceChildren(`Version ${u.version} is available. `,
    el('button', { class: 'link', type: 'button', onclick: () => invoke('open_link', { url: u.page }).catch(showError) }, 'Open the download page'));
}
async function checkForUpdate(daily) {
  const u = await invoke('check_for_update', { daily });
  if (u) {
    showUpdate(u);
    if (daily) toast(`A newer version (${u.version}) is available.`, { label: 'Download', run: () => invoke('open_link', { url: u.page }).catch(showError) });
  } else if (!daily) $('#update-note').textContent = 'This is the newest version.';
}
$('#check-update').addEventListener('click', () => checkForUpdate(false).catch((e) => { $('#update-note').textContent = String(e); }));
if (!PHONE) setTimeout(() => checkForUpdate(true).catch(() => {}), 4000);
$('#open-logs').addEventListener('click', () => invoke('open_log_folder').catch(showError));
$('#copy-version').addEventListener('click', async () => {
  try { await navigator.clipboard.writeText(aboutText); toast('Version copied.'); } catch { toast(aboutText); }
});
showVersion().catch(() => {});

// ---------- Start ----------

(async () => {
  try {
    if (PHONE) MEDIA_BASE = await invoke('media_base').catch(() => null);
    const s = await invoke('startup');
    if (s.journey) enterApp(s.journey);
    else showFirstRun(s.problem);
  } catch (e) { showFirstRun(String(e)); }
})();
