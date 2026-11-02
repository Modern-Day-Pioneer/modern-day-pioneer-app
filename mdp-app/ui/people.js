// People, pets, families, relationships, places and events.
// Uses the helpers in app.js (el, $, invoke, go, toast, ...).
'use strict';

let CHOICES = null;
async function choices() {
  if (!CHOICES) CHOICES = await invoke('choices');
  return CHOICES;
}

let uid = 0;
const nextId = (p) => `${p}-${++uid}`;

function cap(s) { return s ? s[0].toUpperCase() + s.slice(1) : s; }

// ---------- Small form pieces ----------

function textField(label, value, { wide = false, multiline = false, required = false, hint = '' } = {}) {
  const id = nextId('f');
  const input = multiline
    ? el('textarea', { id, rows: 3 }, value || '')
    : el('input', { id, value: value || '', autocomplete: 'off', required });
  const hintEl = hint ? el('p', { class: 'hint', id: `${id}-h` }, hint) : null;
  if (hintEl) input.setAttribute('aria-describedby', `${id}-h`);
  const box = el('div', { class: `field${wide ? ' wide' : ''}` }, el('label', { for: id }, label), input, hintEl);
  box.input = input;
  box.initial = value || '';
  return box;
}

// A date typed as known, showing back how it was understood.
function dateField(label, date, { hint = 'Write it as you know it: "about 1970", "1970s", "June 1956".' } = {}) {
  const f = textField(label, date ? date.words : '', { hint });
  const h = $('.hint', f);
  let seq = 0;
  f.input.addEventListener('input', async () => {
    const text = f.input.value.trim();
    const mine = ++seq;
    if (!text) { h.textContent = hint; h.classList.remove('bad'); return; }
    try {
      const d = await invoke('read_date', { text });
      if (mine !== seq) return;
      h.textContent = d ? `Understood as ${d.words}.` : hint;
      h.classList.remove('bad');
    } catch (e) {
      if (mine !== seq) return;
      h.textContent = String(e);
      h.classList.add('bad');
    }
  });
  return f;
}

function selectField(label, value, options) {
  const id = nextId('f');
  const sel = el('select', { id }, ...options.map(([v, t]) => el('option', { value: v, selected: v === (value || '') }, t)));
  const box = el('div', { class: 'field' }, el('label', { for: id }, label), sel);
  box.input = sel;
  box.initial = value || '';
  return box;
}

function changed(fields) {
  const out = {};
  for (const [k, f] of Object.entries(fields)) {
    const v = f.input.type === 'checkbox' ? f.input.checked : f.input.value.trim();
    if (v !== f.initial) out[k] = v;
  }
  return out;
}

function section(title, ...kids) {
  return el('section', { class: 'section' }, el('h2', {}, title), ...kids);
}

// Stories about someone or holding one of their memories (sprint 5 feedback round 2).
function storiesOf(v) {
  return section('Stories', lines(v.stories, (s) => linkLine(s), 'Not in a story yet.'));
}

function lines(items, render, empty) {
  if (!items.length) return el('p', { class: 'quiet' }, empty);
  return el('ul', { class: 'lines' }, ...items.map(render));
}

// A memory given its cover (withCovers) shows it, small, before its name.
function linkLine(entry, ...buttons) {
  return el('li', {}, 'cover_kind' in entry ? storyThumb(entry, 'small') : null, el('span', { class: 't' },
    el('button', { class: 'link', type: 'button', onclick: () => (entry.open ? entry.open() : openItem(entry.id)) }, entry.name),
    entry.note ? el('small', {}, entry.note) : null,
    entry.words ? el('small', { class: 'words' }, entry.words) : null), ...buttons);
}

async function act(promise, message) {
  try {
    const out = await promise;
    if (message) toast(message, { label: 'Undo', run: undoNow });
    return out;
  } catch (e) { showError(e); throw e; }
}

// Pick a person, pet, family or place by name, or add a new one (quick-add).
// kinds: search kinds to offer; create: which new kinds can be added from here.
// A name box with its list (sprint 5 feedback round 2): clicking it shows
// every name of these kinds, A to Z, and typing narrows the list. With
// `keep`, the chosen name stays in the box, shown like a name already
// linked, until the person types again (then onPick(null) clears it).
function picker(label, { kinds, create = [], exclude = [], onPick, keep = false }) {
  const id = nextId('p');
  const input = el('input', { id, role: 'combobox', 'aria-expanded': 'false', 'aria-autocomplete': 'list',
    'aria-controls': `${id}-list`, autocomplete: 'off', placeholder: 'Choose or type a name' });
  const list = el('div', { class: 'results', id: `${id}-list`, role: 'listbox', 'aria-label': label, hidden: true });
  let all = null;
  let options = [];
  let active = -1;
  let chosen = null;
  const close = () => { list.hidden = true; input.setAttribute('aria-expanded', 'false'); input.removeAttribute('aria-activedescendant'); active = -1; };
  const paint = () => {
    $$('[role=option]', list).forEach((o, i) => o.setAttribute('aria-selected', String(i === active)));
    if (active >= 0) {
      input.setAttribute('aria-activedescendant', `${id}-o${active}`);
      $(`#${id}-o${active}`)?.scrollIntoView({ block: 'nearest' });
    }
  };
  const choose = async (i) => {
    const o = options[i];
    if (!o) return;
    close();
    try {
      let pickedId = o.id;
      const name = o.create ? o.text : o.name;
      if (o.create) {
        const cmd = { person: 'add_individual', pet: 'add_individual', family: 'add_family', place: 'add_place', story: 'add_story' }[o.create];
        const args = o.create === 'person' || o.create === 'pet' ? { kind: o.create, name: o.text } : { name: o.text };
        pickedId = await invoke(cmd, args);
        all = null;
      }
      if (keep) { chosen = name; input.value = name; input.classList.add('picked'); } else input.value = '';
      await onPick(pickedId, name);
    } catch (e) { showError(e); }
  };
  const show = async () => {
    const text = input.value.trim();
    if (!all) all = (await invoke('browse', { kinds }).catch(() => [])).filter((h) => !exclude.includes(h.id));
    const words = text.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean);
    const found = (chosen !== null && text === chosen) || !words.length ? all
      : all.filter((h) => { const w = h.name.toLowerCase().split(/[^\p{L}\p{N}]+/u); return words.every((x) => w.some((y) => y.startsWith(x))); });
    const adds = text && !(chosen !== null && text === chosen) && !found.some((h) => h.name.toLowerCase() === text.toLowerCase())
      ? create.map((k) => ({ create: k, text, name: `Add “${text}” as a new ${k}`, kind: '' })) : [];
    options = [...found, ...adds];
    active = -1;
    list.replaceChildren(...(options.length ? options.map((o, i) => el('div', { role: 'option', id: `${id}-o${i}`, 'aria-selected': 'false',
      onmousedown: (e) => { e.preventDefault(); choose(i); } }, el('span', {}, o.name), el('small', {}, o.kind)))
      : [el('p', {}, text ? `Nothing found for “${text}”.` : 'Nothing here yet. Type a name to add one.')]));
    list.hidden = false;
    input.setAttribute('aria-expanded', 'true');
  };
  input.addEventListener('focus', () => { show(); });
  input.addEventListener('click', () => { if (list.hidden) show(); });
  input.addEventListener('input', () => {
    if (chosen !== null && input.value !== chosen) { chosen = null; input.classList.remove('picked'); onPick(null, ''); }
    show();
  });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); if (list.hidden) { show(); return; } if (options.length) { active = (active + 1) % options.length; paint(); } }
    else if (e.key === 'ArrowUp' && options.length) { active = (active - 1 + options.length) % options.length; paint(); e.preventDefault(); }
    else if (e.key === 'Enter') {
      // Enter picks the highlighted (or first) match; with a name already chosen it submits the form.
      const typed = input.value.trim() && input.value !== chosen;
      if (!list.hidden && options.length && (active >= 0 || typed)) { e.preventDefault(); choose(active >= 0 ? active : 0); }
      else if (chosen === null) e.preventDefault();
    }
    else if (e.key === 'Escape' && !list.hidden) { e.preventDefault(); e.stopPropagation(); close(); }
  });
  input.addEventListener('blur', () => setTimeout(close, 120));
  return el('div', { class: 'picker' }, el('label', { for: id }, label), input, list);
}

// Remove with the warning of what will be unlinked.
async function removeWithWarning(id, name, back = true) {
  try {
    const lines = await invoke('removal_warning', { id });
    const what = lines.length ? `\n\nThis also unlinks:\n• ${lines.join('\n• ')}` : '';
    const ok = await dialog.ask(`Remove ${name} from the Journey?${what}\n\nYou can undo this with Ctrl+Z until you close the app.`,
      { title: 'Remove from Journey', kind: 'warning', okLabel: 'Remove', cancelLabel: 'Keep' });
    if (!ok) return;
    await act(invoke('remove_item', { id }), `${name} is removed.`);
    if (back) goBack();
  } catch (e) { showError(e); }
}

function header(hId, title, kindLine, id) {
  return el('header', {},
    el('div', {}, el('button', { class: 'link', type: 'button', onclick: goBack }, '← Back'),
      el('h1', { id: hId }, title), el('p', { class: 'lede' }, kindLine)),
    el('button', { class: 'btn danger', type: 'button', onclick: () => removeWithWarning(id, title) }, 'Remove from Journey…'));
}

function saveButton(label = 'Save changes') {
  return el('div', { class: 'btns wide' }, el('button', { class: 'btn primary', type: 'submit' }, label));
}

// Typed but unsaved changes on the open page. Adding a relationship, family,
// event, picture or anything else reloads the page, so those changes are
// saved first instead of being lost (sprint 8 fix).
let typed = null;
function keepTyped(fields, save) { typed = { fields, save }; }
function settle(fields) {
  for (const f of Object.values(fields)) f.initial = f.input.type === 'checkbox' ? f.input.checked : f.input.value.trim();
}
async function saveTyped() {
  const t = typed;
  if (!t) return;
  const ch = changed(t.fields);
  if (!Object.keys(ch).length) return;
  try { await t.save(ch); settle(t.fields); } catch (e) { toast(`Your typed changes could not be saved: ${e}`); }
}

const reload = async () => { await saveTyped(); go(current, { push: false, focus: false, id: currentId }); };

// ---------- People list ----------

extraLoaders.people = async () => {
  const [groups, faces] = await Promise.all([invoke('people'), invoke('portraits')]);
  chosen.clear();
  $('#family-list').replaceChildren(...groups.map((g) => el('section', { class: 'fam', 'aria-label': g.name, 'data-family': g.id || null },
    el('h2', { class: 'h3like' }, g.id ? el('button', { class: 'link', type: 'button', 'aria-controls': 'family-summary', onclick: () => showFamilySummary(g.id) }, g.name) : g.name),
    g.members.length
      ? el('div', { class: 'people' }, ...g.members.map((p) => personCard(p, faces[p.id])))
      : el('p', { class: 'quiet' }, g.id ? 'No one in this family yet. Drag people here.' : 'No one in this family yet.'))));
  $('#people-empty').hidden = groups.length > 0;
  fillChoice($('#choose-family'), groups.filter((g) => g.id).map((g) => [g.id, g.name]));
  showChosen();
  if (summaryFamily && groups.some((g) => g.id === summaryFamily)) await showFamilySummary(summaryFamily, false);
  else closeFamilySummary();
};

// A family's summary beside the list (sprint 12): its people, the stories
// and memories about the family or any of them, each one click away.
let summaryFamily = null;
async function showFamilySummary(fid, focus = true) {
  const [v, rows, stories] = await Promise.all([invoke('family_view', { id: fid }), invoke('memory_list'), invoke('stories')]);
  summaryFamily = fid;
  const members = new Set(v.members.map((m) => m.id));
  const linked = new Set(v.memories.map((m) => m.id));
  const mems = rows.filter((m) => linked.has(m.id) || m.people.some((p) => members.has(p)))
    .sort((a, b) => (a.date_sort || '9999').localeCompare(b.date_sort || '9999') || a.name.localeCompare(b.name));
  const storyIds = new Set([...v.stories.map((s) => s.id), ...mems.flatMap((m) => m.stories)]);
  const storyList = stories.filter((s) => storyIds.has(s.id));
  const name = v.name;
  $('#family-summary').replaceChildren(...[
    el('div', { class: 'row' }, el('h2', { id: 'h-famsum', tabindex: '-1' }, name),
      el('button', { class: 'btn', type: 'button', onclick: () => { closeFamilySummary(); } }, 'Close', el('span', { class: 'sr' }, ` the summary of ${name}`))),
    v.description ? el('p', {}, v.description) : null,
    el('p', { class: 'quiet' }, [plural(v.members.length, 'person', 'people'), plural(storyList.length, 'story', 'stories'), plural(mems.length, 'memory', 'memories')].join(', ')),
    section('People', lines(v.members, (m) => linkLine(m), 'No one in this family yet.')),
    section('Stories', lines(storyList, (s) => linkLine(s), 'No stories about this family yet.')),
    section('Memories', lines(mems, (m) => linkLine({ id: m.id, name: m.name, note: [m.date, m.who].filter(Boolean).join(' · '), words: m.words, cover_id: m.cover_id, cover_kind: m.cover_kind,
      open: () => openInList(mems, m.id, name) }), 'No memories with this family or its people yet.')),
    el('div', { class: 'btns' }, el('button', { class: 'btn primary', type: 'button', onclick: () => go('family', { id: fid }) },
      'Open the family', el('span', { class: 'sr' }, ` ${name}`), ' to change it'))].filter((x) => x != null));
  $('#family-summary').hidden = false;
  $('#famsplit').classList.add('open');
  for (const s of $$('#family-list section.fam')) s.classList.toggle('picked', s.dataset.family === fid);
  if (focus) $('#h-famsum').focus();
}
function closeFamilySummary() {
  summaryFamily = null;
  $('#family-summary').hidden = true;
  $('#famsplit').classList.remove('open');
  for (const s of $$('#family-list section.fam.picked')) s.classList.remove('picked');
}

// Sprint 8: tick several people and add them to a family, or drag them onto
// one. The drag is our own (pointer events), because a file drop onto the
// window blocks the web view's own drag and drop on Windows.
const chosen = new Map();
function personCard(p, face) {
  const avatar = el('span', { class: 'avatar', 'aria-hidden': 'true' }, initials(p.name));
  if (face) thumbInto(avatar, { id: face, kind: 'picture' });
  const box = el('input', { type: 'checkbox', id: nextId('c'), 'aria-label': `Choose ${p.name}` });
  box.checked = chosen.has(p.id);
  box.addEventListener('change', () => { if (box.checked) chosen.set(p.id, p.name); else chosen.delete(p.id); showChosen(); });
  const btn = el('button', { class: 'person', type: 'button', onclick: (e) => { if (dragged) { e.preventDefault(); return; } go('person', { id: p.id }); } },
    avatar, el('span', {}, p.name, p.note ? el('small', {}, ` ${p.note}`) : null));
  btn.addEventListener('pointerdown', (e) => startDrag(e, p));
  return el('span', { class: 'personcard', 'data-id': p.id }, box, btn);
}

function showChosen() {
  const n = chosen.size;
  $('#chosen-bar').hidden = n === 0;
  $('#chosen-count').textContent = `${plural(n, 'person', 'people')} chosen`;
  for (const card of $$('.personcard')) $('input', card).checked = chosen.has(card.dataset.id);
}

async function addChosen(family, familyName, ids) {
  if (!family) { toast('Choose a family first.'); return; }
  try {
    const added = await act(invoke('add_family_members', { family, individuals: ids }), null);
    if (added) toast(`${plural(added, 'person', 'people')} added to ${familyName}.`, { label: 'Undo', run: undoNow });
    else toast(`They are already in ${familyName}.`);
    chosen.clear();
    reload();
  } catch { /* shown */ }
}

let dragged = false;
function startDrag(e, p) {
  if (e.button !== 0) return;
  const x0 = e.clientX, y0 = e.clientY;
  let ghost = null, over = null;
  dragged = false;
  const ids = chosen.has(p.id) ? [...chosen.keys()] : [p.id];
  const move = (ev) => {
    if (!ghost && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
    if (!ghost) {
      dragged = true;
      ghost = el('div', { class: 'dragghost', 'aria-hidden': 'true' }, ids.length > 1 ? `${ids.length} people` : p.name);
      document.body.append(ghost);
    }
    ghost.style.left = `${ev.clientX + 12}px`;
    ghost.style.top = `${ev.clientY + 12}px`;
    const target = document.elementFromPoint(ev.clientX, ev.clientY)?.closest('section.fam[data-family]');
    if (target !== over) { over?.classList.remove('dropover'); over = target; over?.classList.add('dropover'); }
  };
  const up = () => {
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    ghost?.remove();
    over?.classList.remove('dropover');
    if (over) addChosen(over.dataset.family, over.getAttribute('aria-label'), ids);
    setTimeout(() => { dragged = false; }, 0);
  };
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', up);
}

$('#add-chosen').addEventListener('click', () => {
  const sel = $('#choose-family');
  addChosen(sel.value, sel.selectedOptions[0]?.textContent, [...chosen.keys()]);
});
$('#clear-chosen').addEventListener('click', () => { chosen.clear(); showChosen(); });

// A portrait for each person and pet, chosen from pictures in their memories (sprint 8).
async function portraitSection(id, name) {
  const [choices, all] = await Promise.all([invoke('portrait_choices', { id }), invoke('portraits')]);
  const current = all[id] || null;
  const face = el('span', { class: 'avatar big', 'aria-hidden': 'true' }, initials(name));
  if (current) thumbInto(face, { id: current, kind: 'picture' });
  const pick = (pid) => async () => { await act(invoke('set_portrait', { id, picture: pid }), pid ? 'Portrait chosen.' : 'Portrait removed.'); reload(); };
  const list = choices.length
    ? el('ul', { class: 'portraits' }, ...choices.map((c) => {
      const label = `Use ${c.name} from ${c.note} as the portrait`;
      const b = el('button', { class: 'thumb', type: 'button', 'aria-pressed': c.id === current ? 'true' : 'false', onclick: pick(c.id) }, el('span', { class: 'sr' }, label));
      thumbInto(b, { id: c.id, kind: 'picture' }).then(() => b.append(el('span', { class: 'sr' }, label)));
      return el('li', {}, b);
    }))
    : el('p', { class: 'quiet' }, `Link ${name} to a memory with a picture to choose a portrait from it.`);
  return section('Portrait',
    el('div', { class: 'row' }, face, el('p', { class: 'quiet' }, current
      ? 'Shown in the People list, on their page and in the export.'
      : 'No portrait yet. Choose one of the pictures from their memories.')),
    list, current ? el('div', { class: 'btns' }, el('button', { class: 'btn', type: 'button', onclick: pick(null) }, 'No portrait')) : null);
}

// Add an individual (a person or pet) or a family: the button opens a small box that asks for the name.
let addingFamily = false;
function openAdd(family) {
  addingFamily = family;
  $('#add-dialog-h').textContent = family ? 'New family' : 'Add an individual';
  $('#add-name-label').textContent = family ? 'Family name, for example "The Rowans" or "Mom\'s side"' : 'Name, for example June Rowan';
  $('#add-name-hint').textContent = family ? 'You can add people to the family on the next screen.'
    : 'You can add dates, family and everything else on the next screen.';
  $('#add-kind').hidden = family;
  $('#add-error').hidden = true;
  $('#add-name').value = '';
  $('#add-dialog').showModal();
  $('#add-name').focus();
}
$('#add-person-btn').addEventListener('click', () => openAdd(false));
$('#new-family').addEventListener('click', () => openAdd(true));
$('#add-cancel').addEventListener('click', () => $('#add-dialog').close());
$('#add-dialog-form').addEventListener('submit', async (ev) => {
  ev.preventDefault();
  const name = $('#add-name').value.trim();
  if (!name) {
    $('#add-error').textContent = addingFamily ? 'Type the family\'s name.' : 'Type their name.';
    $('#add-error').hidden = false;
    $('#add-name').focus();
    return;
  }
  try {
    if (addingFamily) {
      const id = await act(invoke('add_family', { name }), `${name} is added.`);
      $('#add-dialog').close();
      go('family', { id });
    } else {
      const kind = $('input[name="add-kind"]:checked').value;
      const id = await act(invoke('add_individual', { kind, name }), `${name} is added.`);
      $('#add-dialog').close();
      go('person', { id });
    }
  } catch (e) { $('#add-error').textContent = String(e); $('#add-error').hidden = false; }
});

// ---------- Person and pet ----------

extraLoaders.person = async (id) => {
  const [v, c, suggested] = await Promise.all([invoke('individual_view', { id }), choices(),
    suggestOn ? invoke('suggested_relationships', { id }).catch(() => []) : []]);
  const pet = v.kind === 'Pet';
  const f = {
    name: textField('Name shown', v.name, { required: true }),
  };
  if (!pet) {
    f.given_name = textField('First name', v.given_name);
    f.family_name = textField('Surname', v.family_name);
    f.other_names = textField('Other names (nicknames, earlier surnames; one per line)', v.other_names, { wide: true, multiline: true });
  } else {
    f.species = textField('Kind of animal', v.species);
    f.breed = textField('Breed', v.breed);
  }
  f.born = dateField('Born', v.born);
  f.died = dateField('Died', v.died);
  if (pet) f.joined = dateField('Joined the family', v.joined);
  if (!pet) {
    f.gender = selectField('Gender (used only to word relationships)', v.gender, [['', 'Not given'], ...c.genders.map((g) => [g, cap(g)])]);
    f.living_override = selectField('Living, for privacy in the export', v.living_override,
      [['', `Work it out (now: ${v.living ? 'living' : 'not living'})`], ['living', 'Treat as living'], ['not living', 'Treat as not living']]);
  }
  f.description = textField('About (shown on their page)', v.description, { wide: true, multiline: true });

  const birthLine = v.birth_event
    ? el('p', { class: 'quiet wide' }, 'Place of birth: ', v.birthplace ? v.birthplace.name : 'not given yet', ' (it comes from the ',
      el('button', { class: 'link', type: 'button', onclick: () => go('event', { id: v.birth_event }) }, 'birth event'), ').')
    : el('p', { class: 'quiet wide' }, 'Place of birth comes from the birth event, which is made when you give a birth date.');

  const form = el('form', { class: 'form', novalidate: true }, ...Object.values(f).slice(0, pet ? 3 : 4), f.born, f.died,
    ...(pet ? [f.joined] : [f.gender, f.living_override]), birthLine, f.description, saveButton());
  keepTyped(f, (ch) => invoke('update_individual', { id, changes: ch }));
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const ch = changed(f);
    if (!Object.keys(ch).length) { toast('Nothing has changed.'); return; }
    try { await act(invoke('update_individual', { id, changes: ch }), 'Saved.'); settle(f); reload(); } catch { /* shown */ }
  });

  // Families
  const families = section('Families',
    lines(v.families, (fam) => linkLine(fam, el('button', { class: 'btn', type: 'button',
      onclick: async () => { await act(invoke('remove_family_member', { family: fam.id, individual: id }), `Taken out of ${fam.name}.`); reload(); } },
    'Take out', el('span', { class: 'sr' }, ` of ${fam.name}`))), 'Not in a family yet.'),
    el('div', { class: 'addline' }, picker('Add to a family', { kinds: ['family'], create: ['family'], exclude: v.families.map((x) => x.id),
      onPick: async (fid, fname) => { await act(invoke('add_family_member', { family: fid, individual: id }), `Added to ${fname}.`); reload(); } })));

  // Relationships (stored once, worded from this side)
  const relList = lines(v.relationships, (r) => {
    const li = linkLine({ id: r.other_id, name: r.words, note: r.source_note ? `Source: ${r.source_note}` : '' },
      el('button', { class: 'btn', type: 'button', onclick: () => editRel(li, r) }, 'Change…', el('span', { class: 'sr' }, ` ${r.words}`)),
      el('button', { class: 'btn', type: 'button', onclick: async () => {
        await act(invoke('remove_relationship', { id: r.id }), 'Relationship removed.'); reload(); } }, 'Remove', el('span', { class: 'sr' }, ` ${r.words}`)));
    return li;
  }, 'No relationships yet.');
  // Suggested from the data (sprint 10): accepted or turned down, never added on their own.
  const answer = async (sug, yes) => {
    try { await act(invoke('answer_suggestion', { suggestion: sug, accept: yes }), yes ? 'Relationship added.' : 'It won\'t be suggested again.'); reload(); } catch { /* shown */ }
  };
  const sugList = suggested.length ? el('div', {}, el('h3', { class: 'h3like' }, 'Suggested from what you entered'),
    el('ul', { class: 'lines' }, ...suggested.map((sug) => el('li', {}, el('span', { class: 't' }, sug.words, el('small', {}, sug.because)),
      el('button', { class: 'btn', type: 'button', onclick: () => answer(sug, true) }, 'Yes, add it', el('span', { class: 'sr' }, `: ${sug.words}`)),
      el('button', { class: 'btn', type: 'button', onclick: () => answer(sug, false) }, 'Not right', el('span', { class: 'sr' }, `: ${sug.words}`)))))) : null;
  const relationships = section('Relationships', relList, sugList, addRelationship(v, c));

  const merge = section(`Same ${pet ? 'pet' : 'person'} entered twice?`,
    el('p', { class: 'quiet' }, `Choose the other entry to merge it into ${v.name}. Its memories, events, families and relationships move here, details missing here are taken from it, and it is removed. Links to it in older copies still find ${v.name}.`),
    el('div', { class: 'addline' }, picker(`Merge another ${pet ? 'pet' : 'person'} into this one`, { kinds: [pet ? 'pet' : 'person'], exclude: [id],
      onPick: async (oid, oname) => {
        if (!oid) return;
        const ok = await dialog.ask(`Merge ${oname} into ${v.name}?\n\nEverything linked to ${oname} moves to ${v.name}, and ${oname} is removed. You can undo this with Ctrl+Z.`,
          { title: `Merge ${pet ? 'pets' : 'people'}`, kind: 'warning', okLabel: 'Merge', cancelLabel: 'Cancel' });
        if (!ok) return;
        try { await act(invoke('merge_individuals', { keep: id, other: oid }), `${oname} is merged into ${v.name}.`); reload(); } catch { /* shown */ }
      } })));

  const events = section('Events',
    lines(v.events, (e) => linkLine(e), 'No events yet.'),
    addEventLine([id], `Add an event with ${v.name}`));

  const memories = section('Memories',
    lines(withCovers(v.memories, await memoryCovers()), (m) => linkLine({ ...m, note: m.note ? `Role: ${m.note}` : '' }), 'Not in any memory yet.'),
    el('div', { class: 'btns' },
      el('button', { class: 'btn primary', type: 'button', onclick: () => assignFromWaiting(id, v.name) }, 'Add from waiting…'),
      el('button', { class: 'btn', type: 'button', 'data-go': 'waiting' }, 'Go to Waiting'),
      el('button', { class: 'btn', type: 'button', 'data-go': 'stories' }, 'Go to Stories')),
    linkMemoryForm(id));

  const portrait = await portraitSection(id, v.name);
  const tributes = await tributeSection(v);
  $('#person-body').replaceChildren(header('h-person', v.name, pet ? `Pet${v.species ? `, ${v.species}` : ''}` : 'Person', id),
    portrait, form, families, relationships, events, storiesOf(v), tributes, memories, merge);
};

// "In memory of" pages about this person or pet (sprint 13): chosen
// memories with a note on why each matters. A celebration works for the living too.
async function tributeSection(v) {
  const list = await invoke('tributes_about', { about: v.id }).catch(() => []);
  const first = (v.given_name || v.name).trim();
  const start = async () => {
    const id = await askTitle('New In memory of page', 'Title', 'For example "In memory of Grandpa Walter" or "Celebrating Ellen".',
      (name) => act(invoke('add_tribute', { about: v.id, name }), `${name} is started.`), v.living ? `Celebrating ${first}` : `In memory of ${first}`);
    if (id) go('story', { id });
  };
  return section('In memory of',
    el('p', { class: 'quiet' }, `A page about ${v.name}: the pictures, stories and recordings you choose, each with a few words on why it matters. It gets its own page in the export.`),
    lines(list.map((t) => ({ ...t, open: () => go('story', { id: t.id }) })), (t) => linkLine(t), 'No In memory of page yet.'),
    el('div', { class: 'btns' }, el('button', { class: 'btn', type: 'button', onclick: start }, list.length ? 'Start another…' : 'Start an In memory of page…')));
}

function editRel(li, r) {
  if ($('.edit', li)) { $('.edit', li).remove(); return; }
  const parent = r.kind === 'parent';
  const q = parent ? selectField('Kind of parent', r.qualifier, [['', 'Not given'], ...CHOICES.parent_qualifiers.map((x) => [x, cap(x)])]) : null;
  const probably = el('input', { type: 'checkbox', id: nextId('c'), checked: r.certainty === 'probably' });
  const note = textField('Where the family knows this from', r.source_note);
  const box = el('form', { class: 'edit' }, q, el('label', { class: 'switch', for: probably.id }, probably, 'Probably'), note,
    el('button', { class: 'btn primary', type: 'submit' }, 'Save'));
  box.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const changes = { certainty: probably.checked ? 'probably' : 'certain', source_note: note.input.value.trim() };
    if (q) changes.qualifier = q.input.value;
    try { await act(invoke('update_relationship', { id: r.id, changes }), 'Saved.'); reload(); } catch { /* shown */ }
  });
  li.append(box);
  (q ? q.input : probably).focus();
}

function addRelationship(v, c) {
  const kindId = nextId('k');
  const kind = el('select', { id: kindId },
    ...c.relationship_kinds.map((k) => el('option', { value: k }, cap(k))), el('option', { value: '' }, 'Their own word…'));
  const own = textField('Their own word', '');
  own.hidden = true;
  const q = selectField('Kind of parent', '', [['', 'Not given'], ...c.parent_qualifiers.map((x) => [x, cap(x)])]);
  const probably = el('input', { type: 'checkbox', id: nextId('c') });
  const note = textField('Where the family knows this from (optional)', '');
  const sync = () => {
    own.hidden = kind.value !== '';
    q.hidden = !(kind.value === 'parent' || kind.value === 'child');
  };
  kind.addEventListener('change', sync);
  sync();
  let otherId = null;
  const pick = picker('Who', { kinds: ['person', 'pet'], create: ['person', 'pet'], exclude: [v.id], keep: true,
    onPick: (oid) => { otherId = oid; } });
  const form = el('form', { class: 'addline', 'aria-label': 'Add a relationship' },
    el('span', {}, `${v.name} is the`),
    el('div', { class: 'field' }, el('label', { for: kindId }, 'Relationship'), kind), own,
    el('span', {}, 'of'), pick, q,
    el('label', { class: 'switch', for: probably.id }, probably, 'Probably'), note,
    el('button', { class: 'btn primary', type: 'submit' }, 'Add relationship'));
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const k = kind.value || own.input.value.trim();
    if (!otherId) { toast('Choose who the relationship is with.'); return; }
    if (!k) { toast('Type the family\'s own word for it.'); own.input.focus(); return; }
    try {
      await act(invoke('add_relationship', { from: v.id, kind: k, to: otherId, qualifier: q.hidden ? null : (q.input.value || null),
        certainty: probably.checked ? 'probably' : 'certain', sourceNote: note.input.value.trim() || null }), 'Relationship added.');
      reload();
    } catch { /* shown */ }
  });
  return form;
}

// Add an event with these people already in it, then open it.
function addEventLine(people, label) {
  const kindId = nextId('k');
  const kind = el('select', { id: kindId }, ...(CHOICES?.event_kinds || []).map((k) => el('option', { value: k }, cap(k))));
  const when = dateField('When', null, { hint: 'Optional. "about 1970", "June 1956".' });
  const form = el('form', { class: 'addline', 'aria-label': label },
    el('div', { class: 'field' }, el('label', { for: kindId }, 'Kind of event'), kind), when,
    el('button', { class: 'btn primary', type: 'submit' }, 'Add event'));
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    try {
      const id = await act(invoke('add_event', { kind: kind.value, name: null, people, when: when.input.value, place: null }), 'Event added.');
      await saveTyped();
      go('event', { id });
    } catch { /* shown */ }
  });
  return form;
}

// ---------- Family ----------

extraLoaders.family = async (id) => {
  const v = await invoke('family_view', { id });
  const f = { name: textField('Name', v.name, { required: true, hint: '"The Rowans", "Mom\'s side"' }),
    description: textField('About', v.description, { wide: true, multiline: true }) };
  const form = el('form', { class: 'form', novalidate: true }, f.name, f.description, saveButton());
  keepTyped(f, (ch) => invoke('update_family', { id, changes: ch }));
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const ch = changed(f);
    if (!Object.keys(ch).length) { toast('Nothing has changed.'); return; }
    try { await act(invoke('update_family', { id, changes: ch }), 'Saved.'); settle(f); reload(); } catch { /* shown */ }
  });
  const members = section('Members',
    lines(v.members, (m) => linkLine(m, el('button', { class: 'btn', type: 'button',
      onclick: async () => { await act(invoke('remove_family_member', { family: id, individual: m.id }), `${m.name} is taken out of ${v.name}.`); reload(); } },
    'Take out', el('span', { class: 'sr' }, ` ${m.name}`))), 'No one in this family yet.'),
    el('div', { class: 'addline' }, picker('Add an individual', { kinds: ['person', 'pet'], create: ['person', 'pet'], exclude: v.members.map((m) => m.id),
      onPick: async (pid, pname) => { await act(invoke('add_family_member', { family: id, individual: pid }), `${pname} is added to ${v.name}.`); reload(); } })));
  const related = section('Related families',
    lines(v.related, (r) => linkLine(r, el('button', { class: 'btn', type: 'button',
      onclick: async () => { await act(invoke('unrelate_families', { a: id, b: r.id }), 'Link removed.'); reload(); } },
    'Remove link', el('span', { class: 'sr' }, ` to ${r.name}`))), 'No related families.'),
    el('div', { class: 'addline' }, picker('Link a related family', { kinds: ['family'], create: ['family'], exclude: [id, ...v.related.map((r) => r.id)],
      onPick: async (fid, fname) => { await act(invoke('relate_families', { a: id, b: fid }), `Linked to ${fname}.`); reload(); } })));
  await choices();
  const events = section('Events', lines(v.events, (e) => linkLine(e), 'No events yet.'), addEventLine([id], `Add an event with ${v.name}`));
  const memories = section('Memories', lines(withCovers(v.memories, await memoryCovers()), (m) => linkLine({ ...m, note: m.note ? `Role: ${m.note}` : '' }), 'Not in any memory yet.'),
    linkMemoryForm(id));
  $('#family-body').replaceChildren(header('h-family', v.name, 'Family', id), form, members, related, events, storiesOf(v), memories);
};

// ---------- Places and events list ----------

extraLoaders.places = async () => {
  const [v] = await Promise.all([invoke('places_and_events'), choices()]);
  const placeName = el('input', { id: 'new-place', autocomplete: 'off', placeholder: 'For example Ogden, Utah' });
  const addPlace = el('form', { class: 'addline', 'aria-label': 'Add a place' },
    el('div', { class: 'field' }, el('label', { for: 'new-place' }, 'New place'), placeName),
    el('button', { class: 'btn primary', type: 'submit' }, 'Add place'));
  addPlace.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    try { const id = await act(invoke('add_place', { name: placeName.value }), 'Place added.'); go('place', { id }); } catch { /* shown */ }
  });
  $('#places-body').replaceChildren(
    el('header', {}, el('div', {}, el('button', { class: 'link', type: 'button', onclick: goBack }, '← Back'),
      el('h1', { id: 'h-places' }, 'Places and events'),
      el('p', { class: 'lede' }, 'Places are linked from memories and events. A person\'s birthplace comes from their birth event.'))),
    el('div', { class: 'two' },
      section('Places', lines(v.places, (p) => linkLine(p), 'No places yet.'), addPlace),
      section('Events', lines(v.events, (e) => linkLine(e), 'No events yet.'), addEventLine([], 'Add an event'))));
};

// ---------- One place ----------

extraLoaders.place = async (id) => {
  const v = await invoke('place_view', { id });
  const coords = v.latitude != null ? `${v.latitude}, ${v.longitude}` : '';
  const exact = el('input', { type: 'checkbox', id: nextId('c'), checked: v.exact });
  const f = {
    name: textField('Name', v.name, { required: true }),
    address: textField('Address (optional)', v.address),
    coordinates: textField('Coordinates (optional)', coords, { hint: 'Latitude, then longitude, for example 41.223, -111.974. You can paste them from a map.' }),
    description: textField('About', v.description, { wide: true, multiline: true }),
    exact: { input: exact, initial: v.exact },
  };
  const form = el('form', { class: 'form', novalidate: true }, f.name, f.address, f.coordinates,
    el('label', { class: 'switch wide', for: exact.id }, exact, 'Exact place, such as the family home: the export can hide it (privacy setting)'),
    f.description, saveButton());
  keepTyped(f, (ch) => invoke('update_place', { id, changes: ch }));
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const ch = changed(f);
    if (!Object.keys(ch).length) { toast('Nothing has changed.'); return; }
    try { await act(invoke('update_place', { id, changes: ch }), 'Saved.'); settle(f); reload(); } catch { /* shown */ }
  });
  const merge = section('Same place entered twice?',
    el('p', { class: 'quiet' }, `Choose the other entry to merge it into ${v.name}. Its memories and events move here, and it is removed.`),
    el('div', { class: 'addline' }, picker('Merge another place into this one', { kinds: ['place'], exclude: [id],
      onPick: async (oid, oname) => {
        const ok = await dialog.ask(`Merge ${oname} into ${v.name}?\n\nEvery memory and event at ${oname} moves to ${v.name}, and ${oname} is removed. You can undo this with Ctrl+Z.`,
          { title: 'Merge places', kind: 'warning', okLabel: 'Merge', cancelLabel: 'Cancel' });
        if (!ok) return;
        await act(invoke('merge_places', { keep: id, other: oid }), `${oname} is merged into ${v.name}.`);
        reload();
      } })));
  $('#place-body').replaceChildren(header('h-place', v.name, 'Place', id), form,
    section('Events here', lines(v.events, (e) => linkLine(e), 'No events here.')),
    section('Memories here', lines(withCovers(v.memories, await memoryCovers()), (m) => linkLine(m), 'No memories here yet.'),
      el('div', { class: 'btns' }, el('button', { class: 'btn', type: 'button', onclick: () => addMemoriesTo('place_id', id, v.name) }, 'Add memories already in the Journey…'))), merge);
};

// ---------- One event ----------

extraLoaders.event = async (id) => {
  const [v, c] = await Promise.all([invoke('event_view', { id }), choices()]);
  const f = {
    kind: selectField('Kind of event', v.kind, c.event_kinds.map((k) => [k, cap(k)])),
    name: textField('Name', v.name, { required: true }),
    start: dateField(v.end ? 'Starts' : 'When', v.start),
    end: dateField('Ends (optional)', v.end, { hint: 'For a trip or anything that lasted a while.' }),
    description: textField('About', v.description, { wide: true, multiline: true }),
  };
  const form = el('form', { class: 'form', novalidate: true }, f.kind, f.name, f.start, f.end, f.description, saveButton());
  keepTyped(f, (ch) => invoke('update_event', { id, changes: ch }));
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const ch = changed(f);
    if (!Object.keys(ch).length) { toast('Nothing has changed.'); return; }
    try { await act(invoke('update_event', { id, changes: ch }), 'Saved.'); settle(f); reload(); } catch { /* shown */ }
  });

  const place = section('Place',
    v.place
      ? el('ul', { class: 'lines' }, linkLine(v.place, el('button', { class: 'btn', type: 'button',
        onclick: async () => { await act(invoke('update_event', { id, changes: { place_id: '' } }), 'Place removed from the event.'); reload(); } }, 'No place')))
      : el('p', { class: 'quiet' }, 'No place yet.'),
    el('div', { class: 'addline' }, picker(v.place ? 'Change the place' : 'Choose a place', { kinds: ['place'], create: ['place'],
      onPick: async (pid, pname) => { await act(invoke('update_event', { id, changes: { place_id: pid } }), `Place set to ${pname}.`); reload(); } })));

  const ids = v.people.map((p) => p.id);
  const setPeople = async (list, msg) => { await act(invoke('set_event_people', { id, people: list }), msg); reload(); };
  const move = (i, d) => { const l = [...ids]; [l[i], l[i + d]] = [l[i + d], l[i]]; setPeople(l, 'Order changed.'); };
  const firstNote = v.kind === 'birth' ? 'The person born is listed first.' : v.kind === 'death' ? 'The person who died is listed first.' : '';
  const people = section('Who took part',
    firstNote ? el('p', { class: 'quiet' }, firstNote) : null,
    lines(v.people, (p, i) => linkLine(p,
      i > 0 ? el('button', { class: 'btn', type: 'button', onclick: () => move(i, -1) }, 'Up', el('span', { class: 'sr' }, ` ${p.name}`)) : null,
      i < ids.length - 1 ? el('button', { class: 'btn', type: 'button', onclick: () => move(i, 1) }, 'Down', el('span', { class: 'sr' }, ` ${p.name}`)) : null,
      el('button', { class: 'btn', type: 'button', onclick: () => setPeople(ids.filter((x) => x !== p.id), `${p.name} is taken out of the event.`) },
        'Take out', el('span', { class: 'sr' }, ` ${p.name}`))), 'No one yet.'),
    el('div', { class: 'addline' }, picker('Add a person, pet or family', { kinds: ['person', 'pet', 'family'], create: ['person', 'pet'], exclude: ids,
      onPick: (pid, pname) => setPeople([...ids, pid], `${pname} is added to the event.`) })));

  const memories = section('Memories', lines(withCovers(v.memories, await memoryCovers()), (m) => linkLine(m), 'No memories linked to this event yet.'),
    el('div', { class: 'btns' }, el('button', { class: 'btn', type: 'button', onclick: () => addMemoriesTo('event_id', id, v.name) }, 'Add memories already in the Journey…')));
  $('#event-body').replaceChildren(header('h-event', v.name, `Event: ${v.kind}`, id), form, place, people, memories);
};


// ---------- Opening items from search and lists ----------

openers.individual = (id) => go('person', { id });
openers.family = (id) => go('family', { id });
openers.place = (id) => go('place', { id });
openers.event = (id) => go('event', { id });

// Home: "Add family members" opens People and asks for the name.
$('#home-people').addEventListener('click', async () => {
  await go('people', { focus: false });
  openAdd(false);
});
