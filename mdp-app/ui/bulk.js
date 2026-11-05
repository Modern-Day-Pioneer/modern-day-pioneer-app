// Sprint 11 (release 2): bring in a folder with a preview, an import plan
// (CSV), or a history this app exported. The guesses are made by
// mdp_core::bulk and the rebuild by mdp_core::rebuild; this screen shows
// them, lets the person fix them, and starts an ordinary import.

const bulk = { plan: null, folder: null, shown: 200, fromCsv: false, look: null };
const PAGE = 200;

extraLoaders.bulk = async () => {
  const s = await invoke('scan_status').catch(() => null);
  if (s && s.running) watchScan();
};

$('#add-bulk').addEventListener('click', () => { closeAddMenu(false); go('bulk'); });

function bulkShow(which) {
  $('#bulk-plan').hidden = which !== 'plan';
  $('#bulk-export').hidden = which !== 'export';
}

async function bulkPick() {
  const dir = await dialog.open({ directory: true, title: 'Choose a folder (it can hold Google Takeout zips), or a history this app exported' });
  if (!dir) return;
  bulk.folder = dir;
  bulk.zips = null;
  try {
    if ((await invoke('folder_kind', { folder: dir })) === 'export') return showExport(dir);
    await startScan();
  } catch (e) { showError(e); }
}

// Zip files picked one by one, such as all the parts of a Google Takeout.
async function bulkPickZips() {
  let picked = await dialog.open({ multiple: true, title: 'Choose zip files (for a Google Takeout, choose all its parts)', filters: [{ name: 'Zip files', extensions: ['zip'] }] });
  if (!picked) return;
  if (!Array.isArray(picked)) picked = [picked];
  if (!picked.length) return;
  bulk.zips = picked;
  bulk.folder = null;
  try { await startScan(); } catch (e) { showError(e); }
}

async function startScan() {
  bulkShow(null);
  await invoke('start_scan', { folder: bulk.folder || '', zips: bulk.zips || null, options: { photo_dates_first: $('#bulk-photo').checked, story_per_folder: $('#bulk-story').checked } });
  watchScan();
}

let scanPolling = null;
// Another Journey: the last folder's list belongs to the one before (issue #74).
journeyResets.push(() => {
  clearInterval(scanPolling);
  Object.assign(bulk, { plan: null, folder: null, zips: null, shown: PAGE, fromCsv: false });
  bulkShow(null);
  $('#bulk-job').hidden = true;
});
function watchScan() {
  clearInterval(scanPolling);
  const tick = async () => {
    const s = await invoke('scan_status').catch(() => null);
    if (!s) { clearInterval(scanPolling); $('#bulk-job').hidden = true; return; }
    $('#bulk-job').hidden = !s.running;
    if (s.running) {
      $('#bulk-job-msg').textContent = s.message || 'Looking through the folder…';
      $('#bulk-bar').max = Math.max(1, s.total);
      $('#bulk-bar').value = Math.min(s.done, s.total);
      return;
    }
    clearInterval(scanPolling);
    if (s.error) { showError(s.error); return; }
    bulk.fromCsv = false;
    showPlan(s.plan, s.skipped || []);
  };
  tick();
  scanPolling = setInterval(tick, 300);
}

function showPlan(plan, skipped = []) {
  bulk.plan = plan;
  bulk.editing = new Set();
  bulk.placeEditing = new Set();
  bulk.shown = PAGE;
  $('#bulk-from').textContent = bulk.fromCsv ? `From the import plan in ${plan.base}` : `From ${plan.base}`;
  $('#bulk-photo-wrap').hidden = bulk.fromCsv;
  $('#bulk-story').checked = plan.rows.some((r) => r.story);
  $('#bulk-skipped').hidden = skipped.length === 0;
  $('#bulk-skipped-sum').textContent = `${plural(skipped.length, 'file was', 'files were')} left out`;
  $('#bulk-skipped-list').replaceChildren(...skipped.map((k) => el('li', {}, el('b', {}, k.item), ` ${k.reason}`)));
  bulkShow('plan');
  drawRows();
  checkNames().catch(showError);
}

// ---------- Names found (sprint 12) ----------
// Every name in People once, matched to someone in the Journey, added as a
// new person, or left out, for every file that has it. The rows' notes are
// worked out again after each choice, so a note goes once its name is matched.

async function checkNames() {
  if (!bulk.plan) return;
  const out = await invoke('check_plan_names', { plan: bulk.plan, towns: $('#bulk-towns').checked });
  bulk.plan.rows.forEach((r, i) => {
    const o = out.plan.rows[i] || {};
    for (const k of ['people_note', 'place', 'place_from_location', 'place_note']) r[k] = o[k] ?? r[k];
  });
  bulk.found = out.names;
  bulk.places = out.places;
  drawNames();
  drawPlaces();
  paintNotes();
}

// The notes in the rows shown, changed in place so typing isn't interrupted.
function paintNotes() {
  for (const tr of $$('#bulk-rows tr')) {
    const r = bulk.plan.rows[Number(tr.dataset.i)];
    const cell = tr.querySelector('td.file');
    if (!r || !cell) continue;
    const text = [r.people_note, r.place_note, r.note].filter(Boolean).join(' ');
    const place = tr.querySelector('input[aria-label^="Place for"]');
    if (place && document.activeElement !== place && place.value !== r.place) place.value = r.place || '';
    let span = cell.querySelector('.note');
    if (!text) { span?.remove(); continue; }
    if (!span) { span = el('span', { class: 'note' }); cell.append(span); }
    span.textContent = text;
  }
}

function setChoice(name, choice) {
  bulk.editing?.delete(name);
  const names = (bulk.plan.names || []).filter((c) => c.name.toLowerCase() !== name.toLowerCase());
  if (choice) names.push({ name, remember: false, id: '', ...choice });
  bulk.plan.names = names;
  return checkNames().catch(showError);
}

function choiceOf(name) {
  return (bulk.plan.names || []).find((c) => c.name.toLowerCase() === name.toLowerCase());
}

function nameLine(f) {
  const c = choiceOf(f.name);
  const files = plural(f.files, 'file', 'files');
  const sr = (t) => el('span', { class: 'sr' }, t);
  let said;
  if (c && c.action === 'person') said = `Is ${c.who || (f.matches.find((m) => m[0] === c.id) || [])[1] || 'the person chosen'}`;
  else if (c && c.action === 'new') said = 'Will be added as a new person';
  else if (c && c.action === 'leave') said = 'Left out';
  else if (f.status === 'known') said = `Is ${f.matches[0][1]}, already in this Journey`;
  else if (f.status === 'several') said = `More than one person is called this: ${f.matches.map((m) => m[1]).join(', ')}`;
  else said = 'Not in this Journey yet';
  const li = el('li', { 'data-name': f.name },
    el('span', { class: 't' }, el('b', {}, f.name), el('small', {}, `${files} · ${said}`)));
  if (c) {
    if (c.action === 'person') {
      const box = el('input', { type: 'checkbox', checked: c.remember, onchange: (e) => { c.remember = e.target.checked; } });
      li.append(el('label', { class: 'switch' }, box, `Keep "${f.name}" as another name for them`));
    }
    li.append(el('button', { class: 'btn', type: 'button', onclick: () => setChoice(f.name, null) }, 'Change', sr(` what ${f.name} is`)));
    return li;
  }
  // A match can always be changed, however sure the app is.
  const editing = (bulk.editing ||= new Set()).has(f.name);
  if (f.status === 'known' && !editing) {
    li.append(el('button', { class: 'btn', type: 'button', onclick: () => { bulk.editing.add(f.name); drawNames(); } }, 'Change', sr(` who ${f.name} is`)));
    return li;
  }
  const who = picker(`Who is ${f.name}?`, { kinds: ['person', 'pet'],
    onPick: (id, nm) => setChoice(f.name, { action: 'person', id, who: nm, remember: nm.toLowerCase() !== f.name.toLowerCase() }) });
  li.append(el('div', { class: 'addline' }, who,
    el('button', { class: 'btn', type: 'button', onclick: () => setChoice(f.name, { action: 'new' }) }, 'Add as a new person', sr(` named ${f.name}`)),
    el('button', { class: 'btn', type: 'button', onclick: () => setChoice(f.name, { action: 'leave' }) }, 'Leave out', sr(` ${f.name}`)),
    editing ? el('button', { class: 'btn', type: 'button', onclick: () => { bulk.editing.delete(f.name); drawNames(); } }, `Keep ${f.matches[0][1]}`) : null));
  return li;
}

// ---------- Places found (sprint 12) ----------
// Google's locations grouped by place: a place in the Journey nearby, or the
// nearest town from the list built into the app. A choice fills every file there.

function setPlace(key, choice) {
  bulk.placeEditing?.delete(key);
  bulk.plan.places = (bulk.plan.places || []).filter((c) => c.key !== key);
  if (choice) bulk.plan.places.push({ key, name: '', ...choice });
  return checkNames().catch(showError);
}

function placeLine(f) {
  const c = (bulk.plan.places || []).find((x) => x.key === f.key);
  const sr = (t) => el('span', { class: 'sr' }, t);
  const title = f.status === 'none' ? 'No town nearby' : f.suggestion;
  const where = `${f.latitude.toFixed(4)}, ${f.longitude.toFixed(4)}`;
  let said;
  if (c && c.action === 'leave') said = 'Left out';
  else if (c && c.action === 'new') said = `Will be added as a new place: ${c.name}`;
  else if (c) said = `Is ${c.name}`;
  else if (f.status === 'known') said = 'Already in this Journey';
  else if (f.status === 'town') said = `Nearest town, ${f.km < 1 ? 'under 1' : Math.round(f.km)} km away`;
  else said = 'No place in this Journey or town within 30 km';
  const li = el('li', { 'data-key': f.key },
    el('span', { class: 't' }, el('b', {}, title), el('small', {}, `${plural(f.files, 'file', 'files')} · ${said} · ${where}`)));
  if (c) {
    li.append(el('button', { class: 'btn', type: 'button', onclick: () => setPlace(f.key, null) }, 'Change', sr(` the place for ${title}`)));
    return li;
  }
  const editing = bulk.placeEditing.has(f.key);
  if (f.status === 'known' && !editing) {
    li.append(el('button', { class: 'btn', type: 'button', onclick: () => { bulk.placeEditing.add(f.key); drawPlaces(); } }, 'Change', sr(` the place for ${title}`)));
    return li;
  }
  const which = picker('A place already in this Journey', { kinds: ['place'], onPick: (id, nm) => setPlace(f.key, { action: 'place', name: nm }) });
  const name = el('input', { type: 'text', value: f.status === 'town' ? f.suggestion : '', 'aria-label': `Name for the new place at ${where}`, placeholder: 'Name for a new place' });
  li.append(el('div', { class: 'addline' }, which, name,
    el('button', { class: 'btn', type: 'button', onclick: () => {
      if (!name.value.trim()) { toast('Type a name for the new place first.'); name.focus(); return; }
      setPlace(f.key, { action: 'new', name: name.value.trim() });
    } }, 'Add as a new place'),
    el('button', { class: 'btn', type: 'button', onclick: () => setPlace(f.key, { action: 'leave' }) }, 'Leave out', sr(` the place for ${title}`)),
    editing ? el('button', { class: 'btn', type: 'button', onclick: () => { bulk.placeEditing.delete(f.key); drawPlaces(); } }, `Keep ${f.suggestion}`) : null));
  return li;
}

function drawPlaces() {
  const found = bulk.places || [];
  const chosen = (f) => (bulk.plan.places || []).some((c) => c.key === f.key);
  const todo = found.filter((f) => (f.status !== 'known' || bulk.placeEditing.has(f.key)) && !chosen(f));
  const done = found.filter((f) => !todo.includes(f));
  $('#bulk-places').hidden = found.length === 0;
  $('#bulk-places-todo').replaceChildren(...todo.map(placeLine));
  $('#bulk-places-todo').hidden = todo.length === 0;
  $('#bulk-places-done').replaceChildren(...done.map(placeLine));
  $('#bulk-places-done-wrap').hidden = done.length === 0;
  $('#bulk-places-done-sum').textContent = `${plural(done.length, 'place is', 'places are')} matched or chosen`;
  if (!todo.length && done.length) $('#bulk-places-done-wrap').open = true;
}
$('#bulk-towns').addEventListener('change', () => checkNames().catch(showError));

function drawNames() {
  const found = bulk.found || [];
  const todo = found.filter((f) => (f.status !== 'known' || bulk.editing?.has(f.name)) && !choiceOf(f.name));
  const done = found.filter((f) => !todo.includes(f));
  $('#bulk-names').hidden = found.length === 0;
  $('#bulk-names-todo').replaceChildren(...todo.map(nameLine));
  $('#bulk-names-todo').hidden = todo.length === 0;
  $('#bulk-names-done').replaceChildren(...done.map(nameLine));
  $('#bulk-names-done-wrap').hidden = done.length === 0;
  $('#bulk-names-done-sum').textContent = `${plural(done.length, 'name is', 'names are')} matched or chosen`;
  if (!todo.length && done.length) $('#bulk-names-done-wrap').open = true;
}

function folderOf(file) {
  const parts = file.split(/[\\/]/);
  return parts.length > 1 ? parts[parts.length - 2] : '';
}

function drawRows() {
  const rows = bulk.plan.rows;
  const body = $('#bulk-rows');
  body.replaceChildren();
  rows.slice(0, bulk.shown).forEach((r, i) => {
    const text = (field, label, attrs = {}) => el('input', {
      type: 'text', value: r[field] || '', 'aria-label': `${label} for ${r.file}`, ...attrs,
      oninput: (e) => { r[field] = e.target.value; if (field === 'date') src.textContent = e.target.value ? 'typed' : ''; },
    });
    const src = el('span', { class: 'src' }, r.date_source ? `from the ${r.date_source}` : '');
    const tick = el('input', { type: 'checkbox', checked: r.include, 'aria-label': `Bring in ${r.file}`,
      onchange: (e) => { r.include = e.target.checked; tr.classList.toggle('off', !r.include); count(); } });
    const tr = el('tr', { class: r.include ? null : 'off' },
      el('td', {}, tick),
      el('td', { class: 'file' }, r.file, r.kind ? el('span', { class: 'src' }, r.kind) : null,
        r.people_note || r.place_note || r.note ? el('span', { class: 'note' }, [r.people_note, r.place_note, r.note].filter(Boolean).join(' ')) : null),
      el('td', {}, text('title', 'Title')),
      el('td', {}, text('date', 'Date'), src),
      el('td', {}, text('people', 'People', { onchange: () => checkNames().catch(showError) })),
      el('td', {}, text('place', 'Place', { onchange: () => { r.place_from_location = false; checkNames().catch(showError); } })),
      el('td', {}, text('memory', 'Memory', { placeholder: 'Its own memory' }), r.group && !r.memory ? el('span', { class: 'src' }, `Suggested: ${r.group}`) : null));
    tr.dataset.i = i;
    body.append(tr);
  });
  const left = rows.length - bulk.shown;
  $('#bulk-more').hidden = left <= 0;
  $('#bulk-more').textContent = `Show ${Math.min(left, PAGE)} more of ${left}`;
  count();
}

function count() {
  const rows = bulk.plan.rows;
  const on = rows.filter((r) => r.include);
  const memories = new Set(on.map((r, i) => (r.memory.trim() ? `m:${r.memory.trim().toLowerCase()}` : `r:${i}`))).size;
  $('#bulk-count').textContent = `${plural(on.length, 'file', 'files')} ticked, making ${plural(memories, 'memory', 'memories')}.`;
  $('#bulk-go').textContent = `Bring in ${plural(on.length, 'file', 'files')}`;
  $('#bulk-go').disabled = on.length === 0;
  $('#bulk-all').checked = on.length === rows.length;
  const groups = rows.filter((r) => r.group).length;
  $('#bulk-groups').disabled = groups === 0;
}

$('#bulk-more').addEventListener('click', () => { bulk.shown += PAGE; drawRows(); });
$('#bulk-all').addEventListener('change', (e) => { for (const r of bulk.plan.rows) r.include = e.target.checked; drawRows(); });
$('#bulk-groups').addEventListener('click', () => { for (const r of bulk.plan.rows) if (r.group) r.memory = r.group; drawRows(); });
$('#bulk-by-folder').addEventListener('click', () => {
  for (const r of bulk.plan.rows) { const f = folderOf(r.file); r.memory = f || r.title; }
  drawRows();
});
$('#bulk-each').addEventListener('click', () => { for (const r of bulk.plan.rows) r.memory = ''; drawRows(); });
$('#bulk-story').addEventListener('change', (e) => { for (const r of bulk.plan.rows) r.story = e.target.checked ? folderOf(r.file) : ''; });
$('#bulk-photo').addEventListener('change', () => { if ((bulk.folder || bulk.zips) && !bulk.fromCsv) startScan().catch(showError); });
$('#bulk-pick').addEventListener('click', bulkPick);
$('#bulk-zips').addEventListener('click', bulkPickZips);
$('#add-zips').addEventListener('click', () => { closeAddMenu(false); $('#takeout-dialog').showModal(); });
$('#takeout-open').addEventListener('click', () => invoke('open_link', { url: 'https://takeout.google.com/settings/takeout/custom/photos' }).catch(showError));
$('#takeout-choose').addEventListener('click', () => { $('#takeout-dialog').close(); go('bulk'); bulkPickZips(); });
$('#bulk-cancel').addEventListener('click', () => { invoke('cancel_scan'); $('#bulk-job').hidden = true; clearInterval(scanPolling); });

const CSV = [{ name: 'Import plan (CSV)', extensions: ['csv'] }];
$('#bulk-open').addEventListener('click', async () => {
  const path = await dialog.open({ title: 'Open an import plan', filters: CSV });
  if (!path) return;
  try {
    bulk.fromCsv = true;
    bulk.folder = null;
    bulk.zips = null;
    showPlan(await invoke('open_plan', { path }));
  } catch (e) { showError(e); }
});
$('#bulk-save').addEventListener('click', async () => {
  const path = await dialog.save({ title: 'Save as import plan', defaultPath: 'import-plan.csv', filters: CSV });
  if (!path) return;
  try { await invoke('save_plan', { path, plan: bulk.plan }); toast('The import plan is saved. A spreadsheet or script can change it; open it here again to bring the files in.'); } catch (e) { showError(e); }
});
$('#bulk-sample').addEventListener('click', async () => {
  const path = await dialog.save({ title: 'Save a blank sample plan', defaultPath: 'import-plan.csv', filters: CSV });
  if (!path) return;
  try { const readme = await invoke('save_sample_plan', { path }); toast(`Saved the sample plan, with a read-me beside it: ${readme}`); } catch (e) { showError(e); }
});

$('#bulk-go').addEventListener('click', async () => {
  try {
    await invoke('start_plan_import', { plan: bulk.plan, addMissing: $('#bulk-add').checked });
    bulkShow(null);
    go('waiting');
    watchImport();
  } catch (e) { showError(e); }
});

// ---------- An exported history: a full rebuild, which also merges two Journeys ----------

const KINDS = [['person', 'People and pets'], ['pet', null], ['family', 'Families'], ['place', 'Places'], ['event', 'Events'], ['story', 'Stories'], ['memory', 'Memories']];

async function showExport(dir) {
  const look = await invoke('look_at_export', { folder: dir });
  bulk.look = look;
  const parts = [plural(look.people, 'person or pet', 'people and pets'), plural(look.families, 'family', 'families'),
    plural(look.places, 'place', 'places'), plural(look.events, 'event', 'events'), plural(look.stories, 'story', 'stories'),
    plural(look.memories, 'memory', 'memories'), plural(look.files, 'file', 'files')];
  let text = `This is a history exported by the app: “${look.name}”, with ${parts.join(', ')}.`;
  if (look.already) text += ` ${plural(look.already, 'item is', 'items are')} already in this Journey and will be left as they are.`;
  $('#bulk-ex-sum').textContent = text;
  const groups = $('#bulk-ex-groups');
  groups.replaceChildren();
  KINDS.filter(([, label]) => label).forEach(([kind, label]) => {
    const items = look.items.map((it, i) => ({ ...it, i })).filter((it) => it.kind === kind || (kind === 'person' && it.kind === 'pet'));
    if (!items.length) return;
    const sum = el('summary');
    const all = el('input', { type: 'checkbox', checked: true, 'aria-label': `Bring in all ${label.toLowerCase()}` });
    const rows = items.map((it) => {
      const box = el('input', { type: 'checkbox', id: `bulk-x-${it.i}`, checked: true, 'data-i': it.i, class: 'pick' });
      const use = it.journey_id ? el('select', { 'aria-label': `What to do with ${it.name}`, 'data-i': it.i, class: 'use' },
        el('option', { value: 'match' }, 'Use the one already here'), el('option', { value: 'new' }, 'Bring in as a new one')) : null;
      box.addEventListener('change', () => { if (use) use.disabled = !box.checked; tally(); });
      return el('li', {}, el('label', { class: 'switch', for: box.id }, box, el('span', {}, it.name, it.detail ? el('span', { class: 'src' }, it.detail) : null)), use);
    });
    const tally = () => {
      const on = rows.filter((r) => r.querySelector('.pick').checked).length;
      sum.textContent = `${label}: ${on} of ${rows.length} ticked`;
      all.checked = on === rows.length;
      bulkExCount();
    };
    all.addEventListener('change', () => { for (const r of rows) { r.querySelector('.pick').checked = all.checked; const u = r.querySelector('.use'); if (u) u.disabled = !all.checked; } tally(); });
    groups.append(el('details', { class: 'bulk-matches', open: items.length <= 25 }, sum,
      el('label', { class: 'switch' }, all, 'All'), el('ul', {}, rows)));
    tally();
  });
  bulkShow('export');
}

function bulkExCount() {
  const left = $$('#bulk-ex-groups .pick').filter((b) => !b.checked).length;
  $('#bulk-ex-go').textContent = left ? `Bring it in, leaving out ${plural(left, 'item', 'items')}` : 'Bring it all in';
}

$('#bulk-ex-go').addEventListener('click', async () => {
  const merge = {};
  const leaveOut = [];
  for (const box of $$('#bulk-ex-groups .pick')) {
    const it = bulk.look.items[Number(box.dataset.i)];
    if (!box.checked) { leaveOut.push(it.export_id); continue; }
    const use = $(`#bulk-ex-groups .use[data-i="${box.dataset.i}"]`);
    if (use && use.value === 'match') merge[it.export_id] = it.journey_id;
  }
  try {
    await invoke('start_export_import', { folder: bulk.folder, merge, leaveOut });
    bulkShow(null);
    go('waiting');
    watchImport();
  } catch (e) { showError(e); }
});
