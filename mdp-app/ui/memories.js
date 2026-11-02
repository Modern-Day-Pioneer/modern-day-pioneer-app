// Memories, stories, the Journey's title and featured items, and the Waiting
// screen's "put into a memory", "assign to a person" and large preview
//. Uses the helpers in app.js
// and people.js (el, $, invoke, go, toast, textField, dateField, picker, ...).
'use strict';

const KIND_NAME = { picture: 'Picture', 'voice recording': 'Recording', video: 'Video', text: 'Typed note or text' };

// ---------- A small box that asks something (built here, so index.html stays short) ----------

// Opens a dialog with these fields; resolves with true when the person confirms
// (after check() passes), or false when they cancel.
function modal(title, fields, { ok = 'OK', cancel: cancelLabel = 'Cancel', check = async () => true, wide = false } = {}) {
  return new Promise((resolve) => {
    const hId = nextId('dh');
    const error = el('p', { class: 'error', role: 'alert', hidden: true });
    const cancel = el('button', { class: 'btn', type: 'button' }, cancelLabel);
    const form = el('form', { method: 'dialog', novalidate: true }, el('h2', { id: hId }, title), ...fields, error,
      el('div', { class: 'btns' }, el('button', { class: 'btn primary', type: 'submit' }, ok), cancel));
    const d = el('dialog', { class: `ask${wide ? ' wide' : ''}`, 'aria-labelledby': hId }, form);
    let done = false;
    const finish = (v) => { if (done) return; done = true; d.close(); d.remove(); resolve(v); };
    cancel.addEventListener('click', () => finish(false));
    d.addEventListener('cancel', (e) => { e.preventDefault(); finish(false); });
    form.addEventListener('submit', async (ev) => {
      ev.preventDefault();
      try {
        const okay = await check();
        if (okay === true) finish(true);
        else if (typeof okay === 'string') { error.textContent = okay; error.hidden = false; }
      } catch (e) { error.textContent = String(e); error.hidden = false; }
    });
    document.body.append(d);
    d.showModal();
    const first = $('input:not([type=checkbox]):not([type=radio]), textarea, select', form);
    if (first) first.focus();
  });
}

// Ask for a title, then run make(title). Returns make's result, or null when canceled.
async function askTitle(title, label, hint, make, initial = '') {
  const f = textField(label, initial, { hint });
  let out = null;
  const ok = await modal(title, [f], { ok: 'Add', check: async () => {
    const name = f.input.value.trim();
    if (!name) return 'Type a title.';
    out = await make(name);
    return true;
  } });
  return ok ? out : null;
}

// A "More ▾" button holding the less used actions of one item (sprint 5
// feedback round 2). actions: [label, run] pairs; null entries are skipped.
function moreMenu(sr, actions) {
  actions = actions.filter(Boolean);
  if (!actions.length) return null;
  const mid = nextId('mm');
  const btn = el('button', { class: 'btn', type: 'button', 'aria-haspopup': 'menu', 'aria-expanded': 'false', 'aria-controls': mid },
    'More ▾', el('span', { class: 'sr' }, ` for ${sr}`));
  const menu = el('div', { class: 'menu right', id: mid, role: 'menu', hidden: true });
  const close = (focus) => { menu.hidden = true; btn.setAttribute('aria-expanded', 'false'); if (focus) btn.focus(); };
  for (const [label, run] of actions) {
    menu.append(el('button', { type: 'button', role: 'menuitem', onclick: () => { close(false); run(); } }, label));
  }
  btn.addEventListener('click', () => {
    const open = menu.hidden;
    for (const m of $$('.menu.right:not([hidden])')) m.hidden = true;
    menu.hidden = !open;
    btn.setAttribute('aria-expanded', String(open));
    if (open) $('[role=menuitem]', menu).focus();
  });
  menu.addEventListener('keydown', (e) => {
    const items = $$('[role=menuitem]', menu);
    const i = items.indexOf(document.activeElement);
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(true); }
    else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      items[(i + (e.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length].focus();
    }
  });
  const wrap = el('div', { class: 'menuwrap' }, btn, menu);
  document.addEventListener('click', (e) => { if (!wrap.contains(e.target)) close(false); });
  return wrap;
}

function chip(text, cls = 'needs') { return el('span', { class: `chip ${cls}` }, text); }

function smallBtn(label, sr, onclick, extra = {}) {
  return el('button', { class: 'btn', type: 'button', onclick, ...extra }, label, sr ? el('span', { class: 'sr' }, ` ${sr}`) : null);
}

async function thumbInto(box, item) {
  if (item.kind !== 'picture' && item.kind !== 'video') return;
  const url = await invoke('thumbnail', { id: item.id }).catch(() => null);
  if (url) box.replaceChildren(el('img', { src: url, alt: '' }));
}

function featuredSwitch(id, on, what) {
  const box = el('input', { type: 'checkbox', id: nextId('c'), checked: on });
  box.addEventListener('change', async () => {
    try {
      await act(invoke('set_featured', { id, on: box.checked }), box.checked ? `${what} is featured on the home page.` : `${what} is off the home page.`);
    } catch { box.checked = !box.checked; }
  });
  return el('label', { class: 'switch', for: box.id }, box, 'Feature on the home page (up to six stories or memories)');
}

// ---------- Stories & Memories screen ----------
// Each story opens to show its memories in story order; the memories in no
// story follow under "Assorted memories", the export's own heading. Stories
// start open when there are five or fewer, closed when there are more.

let memoryRows = [];
extraLoaders.stories = async () => {
  const [stories, memories, rows, people] = await Promise.all([invoke('stories'), invoke('memories'), invoke('memory_list'),
    invoke('browse', { kinds: ['person', 'pet'] })]);
  memoryRows = rows;
  const prev = new Map(rows.map((m) => [m.id, { words: m.words, who: m.who }]));
  const views = (await Promise.all(stories.map((s) => invoke('story_view', { id: s.id }))))
    .map((v) => ({ ...v, memories: v.memories.map((m) => ({ ...m, ...prev.get(m.id) })) }));
  const open = stories.length <= 5;
  $('#story-list').replaceChildren(...views.map((v) => el('details', { class: 'storygroup', open },
    el('summary', {}, el('span', { class: 'name' }, v.name), el('small', {}, `${v.genre === 'tribute' ? 'In memory of page · ' : ''}${plural(v.memories.length, 'memory', 'memories')}`)),
    el('div', { class: 'groupbody' },
      v.memories.length ? el('ul', { class: 'todo' }, ...v.memories.map((m) => memoryLine(m, v.memories, v.name))) : el('p', { class: 'quiet' }, 'No memories in this story yet.'),
      el('div', { class: 'row' },
        el('button', { class: 'btn', type: 'button', onclick: () => go('story', { id: v.id }) }, 'Open the story', el('span', { class: 'sr' }, ` ${v.name}`)),
        el('button', { class: 'btn', type: 'button', onclick: () => newMemory(v.id) }, 'New memory in this story…', el('span', { class: 'sr' }, ` (${v.name})`)))))));
  $('#stories-empty').hidden = stories.length > 0;
  const covers = new Map(rows.map((m) => [m.id, { cover_id: m.cover_id, cover_kind: m.cover_kind }]));
  const loose = withCovers(memories.filter((m) => m.note.startsWith('Not in a story yet')), covers);
  $('#loose-memories').replaceChildren(lines(loose, (m) => linkLine({ ...m, note: m.note.replace(/^Not in a story yet( · )?/, ''),
    open: () => openInList(loose, m.id, 'Assorted memories') }),
    'Every memory is in a story.'));
  $('#all-memories-count').textContent = plural(memories.length, 'memory', 'memories');
  fillChoice($('#mf-person'), people.map((p) => [p.id, p.name]));
  fillChoice($('#mf-story'), stories.map((s) => [s.id, s.name]), 2);
  showMemories();
};

const memoryLine = (m, list, from) => el('li', {}, storyThumb(m),
  el('span', { class: 't' }, el('button', { class: 'link', type: 'button', onclick: () => openInList(list, m.id, from) }, m.name),
    el('small', {}, [m.date || 'No date yet', m.who].filter(Boolean).join(' · ')),
    m.words ? el('small', { class: 'words' }, m.words) : null),
  m.status === 'needs content' ? chip('Needs a photo, recording or note') : null);

// Keep a filter's choice when its options are filled again; the first `keep` options are fixed.
function fillChoice(sel, options, keep = 1) {
  const was = sel.value;
  while (sel.options.length > keep) sel.remove(keep);
  for (const [v, t] of options) sel.append(el('option', { value: v }, t));
  sel.value = [...sel.options].some((o) => o.value === was) ? was : '';
}

// Sort and filter memories (sprint 8). "By story" with no filter shows the stories as before.
function showMemories() {
  const sort = $('#mf-sort').value;
  const text = $('#mf-text').value.trim().toLowerCase();
  const who = $('#mf-person').value;
  const story = $('#mf-story').value;
  const needs = $('#mf-needs').checked;
  const flat = sort !== 'story' || text || who || story || needs;
  $('#memory-results').hidden = !flat;
  $('#story-list').hidden = !!flat;
  $('#stories-empty').hidden = flat || $('#story-list').children.length > 0;
  $('#h-loose').closest('section').hidden = !!flat;
  if (!flat) return;
  const rows = memoryRows.filter((m) => (!text || m.name.toLowerCase().includes(text))
    && (!who || m.people.includes(who))
    && (!story || (story === '-' ? !m.stories.length : m.stories.includes(story)))
    && (!needs || m.status === 'needs content'));
  const by = {
    name: (a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }),
    date: (a, b) => (a.date_sort ? 0 : 1) - (b.date_sort ? 0 : 1) || (a.date_sort || '').localeCompare(b.date_sort || '') || by.name(a, b),
    added: (a, b) => (b.added || '').localeCompare(a.added || '') || by.name(a, b),
  };
  rows.sort(by[sort] || by.name);
  $('#results-count').textContent = `${plural(rows.length, 'memory', 'memories')} of ${memoryRows.length}`;
  $('#results-list').replaceChildren(...rows.map((m) => memoryLine(m, rows, 'the list shown')));
}
for (const id of ['mf-sort', 'mf-person', 'mf-story', 'mf-needs']) $(`#${id}`).addEventListener('change', showMemories);
$('#mf-text').addEventListener('input', showMemories);
$('#memory-tools').addEventListener('submit', (e) => e.preventDefault());

async function newStory() {
  const id = await askTitle('New story', 'Title, for example "The Kitchen Table"', 'You can choose who it is about and add memories on the next screen.',
    (name) => act(invoke('add_story', { name }), `${name} is added.`));
  if (id) go('story', { id });
}

async function newMemory(story = null) {
  const id = await askTitle('New memory', 'Title, for example "Walter and June\'s wedding day"', 'A memory with nothing in it yet waits as "needs content" and is left out of the export.',
    (name) => act(invoke('add_memory', { name, story }), `${name} is added.`));
  if (id) go('memory', { id });
}

$('#new-story').addEventListener('click', newStory);
$('#new-memory').addEventListener('click', () => newMemory());
$('#home-story').addEventListener('click', newStory);

// ---------- One story ----------

async function focusOptions() {
  const groups = await invoke('people');
  const seen = new Set();
  const opts = [['', 'No one in particular']];
  for (const g of groups) if (g.id) { opts.push([g.id, `${g.name} (family)`]); seen.add(g.id); }
  for (const g of groups) for (const m of g.members) if (!seen.has(m.id)) { opts.push([m.id, m.name]); seen.add(m.id); }
  return opts;
}

extraLoaders.story = async (id) => {
  const [v, focus] = await Promise.all([invoke('story_view', { id }), focusOptions()]);
  v.memories = await withPreviews(v.memories);
  const tribute = v.genre === 'tribute';
  const f = {
    name: textField('Title', v.name, { required: true }),
    focus_id: selectField(tribute ? 'Who it is about' : 'Who it is about (optional)', v.focus ? v.focus.id : '',
      tribute ? focus.filter(([fid, label]) => !fid || !label.endsWith('(family)')) : focus),
    introduction: textField('Introduction', v.introduction, { wide: true, multiline: true,
      hint: tribute ? 'Opens the page in the exported history: who they are and why this page exists.' : 'Opens the story in the exported history.' }),
  };
  const form = el('form', { class: 'form', novalidate: true }, f.name, f.focus_id, f.introduction, saveButton());
  keepTyped(f, (ch) => invoke('update_story', { id, changes: ch }));
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const ch = changed(f);
    if (!Object.keys(ch).length) { toast('Nothing has changed.'); return; }
    try { await act(invoke('update_story', { id, changes: ch }), 'Saved.'); settle(f); reload(); } catch { /* shown */ }
  });

  const n = v.memories.length;
  const toFinish = v.memories.filter((m) => m.status === 'needs content').length;
  const move = async (m, d) => { await act(invoke('move_in_story', { story: id, memory: m.id, delta: d }), 'Order changed.'); reload(); };
  const list = lines(v.memories, (m, i) => el('li', {},
    el('span', { class: 'pos', 'aria-hidden': 'true' }, String(i + 1)),
    storyThumb(m),
    el('span', { class: 't' }, el('button', { class: 'link', type: 'button', onclick: () => openInList(v.memories, m.id, v.name) }, m.name),
      el('small', {}, [m.date || 'No date yet', m.cover_kind ? KIND_NAME[m.cover_kind] : null, m.who].filter(Boolean).join(' · ')),
      m.words ? el('small', { class: 'words' }, m.words) : null,
      tribute ? el('small', { class: m.note ? 'why' : 'quiet' }, m.note ? `Why it matters: ${m.note}` : 'No note yet on why it matters.') : null),
    tribute ? smallBtn(m.note ? 'Change why it matters' : 'Why it matters…', m.name, () => whyItMatters(id, m)) : null,
    m.status === 'needs content' ? chip('Needs a photo, recording or note') : chip('Complete', 'add'),
    i > 0 ? smallBtn('Move up', m.name, () => move(m, -1)) : null,
    i < n - 1 ? smallBtn('Move down', m.name, () => move(m, 1)) : null,
    smallBtn(tribute ? 'Take off this page' : 'Take out of story', m.name, async () => {
      await act(invoke('remove_memory_from_story', { story: id, memory: m.id }), `${m.name} is taken ${tribute ? 'off the page' : 'out of the story'}. It stays in the Journey.`); reload();
    })), tribute ? 'No memories on this page yet. Add the ones that say most about them.' : 'No memories in this story yet.');
  const memories = section('Memories, in order',
    el('p', { class: 'quiet' }, n ? `${plural(n, 'memory', 'memories')}${toFinish ? `, ${toFinish} to finish` : ''}. ${tribute
      ? 'The export shows them in this order on one page, each with its note.' : 'The export shows them in this order, one page each.'}` : ''),
    list,
    el('div', { class: 'addline' },
      el('button', { class: 'btn', type: 'button', onclick: async () => {
        const picked = await chooseMemories(tribute ? 'Add memories to this page' : 'Add memories to this story', { exclude: v.memories.map((m) => m.id),
          hint: 'Tick the ones to add. They go at the end, in the order shown here.' });
        if (!picked.length) return;
        await act(invoke('add_memories_to_story', { story: id, memories: picked.map((m) => m.id) }),
          `${picked.length === 1 ? picked[0].name : plural(picked.length, 'memory', 'memories')} added.`);
        reload();
      } }, 'Add memories already in the Journey…'),
      el('button', { class: 'btn primary', type: 'button', onclick: () => newMemory(id) }, tribute ? 'New memory on this page…' : 'New memory in this story…'),
      el('button', { class: 'btn', type: 'button', onclick: () => storyFromWaiting(id, v.name) }, 'Add from waiting…')));

  const cover = tribute
    ? el('p', { class: 'quiet' }, 'In the export this page is linked from their own page and from the front door, with their portrait when they have one.')
    : v.cover_id
    ? el('p', { class: 'quiet' }, 'Cover: the first picture in the story\'s first memory that has one. In the export this story is one book on the shelf.')
    : el('p', { class: 'quiet' }, 'No cover yet: it will be the first picture in the story\'s first memory that has one.');
  const coverBox = el('span', { class: 'thumb big', 'aria-hidden': 'true' }, '');
  if (v.cover_id) thumbInto(coverBox, { id: v.cover_id, kind: 'picture' });

  $('#story-body').replaceChildren(header('h-story', v.name, `${tribute ? 'In memory of page' : 'Story'}${v.focus ? `, about ${v.focus.name}` : ''}`, id),
    ...[form, el('div', { class: 'row' }, v.cover_id && !tribute ? coverBox : null, cover), tribute ? null : featuredSwitch(id, v.featured, v.name), memories].filter(Boolean));
};

// The note on an "In memory of" page saying why a memory matters (sprint 13).
async function whyItMatters(story, m) {
  const f = textField('Why it matters', m.note || '', { wide: true, multiline: true, hint: 'A few words in your own voice. Leave it empty to show the memory without a note.' });
  const ok = await modal(`Why ${m.name} matters`, [f], { ok: 'Save', check: async () => {
    await invoke('set_memory_note', { story, memory: m.id, note: f.input.value });
    return true;
  } });
  if (ok) { toast('Saved.'); reload(); }
}

// The memory's picture (or video still), small, so a picture put into it shows here.
function storyThumb(m, size) {
  const box = el('span', { class: size ? `thumb ${size}` : 'thumb', 'aria-hidden': 'true' }, (KIND_NAME[m.cover_kind] || '·')[0]);
  if (m.cover_id) thumbInto(box, { id: m.cover_id, kind: 'picture' });
  return box;
}

// ---------- One memory ----------

// Previous and Next on an open memory (sprint 12): through the list it was
// opened from, in the order shown there. The left and right arrow keys do the same.
let memoryNav = null;
function openInList(list, id, from) {
  memoryNav = { ids: list.map((m) => m.id), from };
  go('memory', { id });
}
function memoryNavBar(id) {
  const ids = memoryNav ? memoryNav.ids : [];
  const i = ids.indexOf(id);
  if (i < 0 || ids.length < 2) return null;
  const step = async (d) => { await saveTyped(); go('memory', { id: ids[i + d], push: false }); };
  return el('nav', { class: 'row memnav', 'aria-label': 'Previous and next memory' },
    el('button', { class: 'btn', type: 'button', id: 'mem-prev', disabled: i === 0, onclick: () => step(-1) }, '← Previous'),
    el('span', { class: 'quiet' }, `${i + 1} of ${ids.length} in ${memoryNav.from}`),
    el('button', { class: 'btn', type: 'button', id: 'mem-next', disabled: i === ids.length - 1, onclick: () => step(1) }, 'Next →'));
}

extraLoaders.memory = async (id) => {
  const [v, pe, proposed] = await Promise.all([invoke('memory_view', { id }), invoke('places_and_events'),
    suggestOn ? invoke('memory_proposals', { id }).catch(() => []) : []]);
  const inStories = v.stories.map((s) => s.name).join(', ');

  // Photos, recordings and notes, in order, with the cover first unless one was chosen.
  const n = v.items.length;
  const move = async (it, d) => { await act(invoke('move_in_memory', { memory: id, content: it.id, delta: d }), 'Order changed.'); reload(); };
  // Turning keeps the original as it is: the app stores the turn, and the export copy carries it.
  const turn = async (it, q) => { await act(invoke('turn_picture', { id: it.id, quarters: q }), `${it.name} is turned ${q > 0 ? 'right' : 'left'}. The original file is not changed.`); reload(); };
  const cards = v.items.map((it, i) => {
    const thumb = el('button', { class: 'thumb big', type: 'button', onclick: () => openViewer(v.items, i, v.name) },
      el('span', { 'aria-hidden': 'true' }, (KIND_NAME[it.kind] || '?')[0]), el('span', { class: 'sr' }, `Show ${it.name} large`));
    thumbInto(thumb, it).then(() => thumb.append(el('span', { class: 'sr' }, `Show ${it.name} large`)));
    const extras = [
      it.cover || (it.kind !== 'picture' && it.kind !== 'video') ? null : ['Make this the cover', async () => {
        await act(invoke('update_memory', { id, changes: { cover_id: it.id } }), 'Cover changed.'); reload(); }],
      it.kind === 'picture' ? ['Turn right', () => turn(it, 1)] : null,
      it.kind === 'picture' ? ['Turn left', () => turn(it, -1)] : null,
      i > 0 ? ['Move earlier', () => move(it, -1)] : null,
      i < n - 1 ? ['Move later', () => move(it, 1)] : null,
      ['Take out of this memory', async () => {
        await act(invoke('remove_from_memory', { memory: id, content: it.id }), `${it.name} is taken out. It waits on the Waiting screen.`); reload(); }],
    ];
    // Recordings and videos lead with the transcript; its More menu holds the rest.
    const talk = transcriptBits(it, v.name, extras);
    return el('li', { class: 'item' }, thumb,
      el('div', { class: 't' }, el('b', {}, it.name), el('small', {}, `${KIND_NAME[it.kind] || it.kind} · ${it.date || 'date not known'}`),
        it.cover ? chip('Cover', 'add') : null, talk),
      talk ? null : el('div', { class: 'btns' }, smallBtn('Show large', it.name, () => openViewer(v.items, i, v.name)), moreMenu(it.name, extras)));
  });
  const items = section(`Photos, recordings and notes${n ? `, ${n} in order` : ''}`,
    v.status === 'needs content' ? el('p', { class: 'warn' }, 'This memory needs a photo, recording or note before it shows in the export.') : null,
    n ? el('div', { class: 'row' }, thumbSize('memory')) : null,
    n ? el('ul', { class: 'items' }, ...cards) : null,
    el('div', { class: 'slots' },
      el('button', { class: 'slot', type: 'button', onclick: () => addFromWaiting(id, v.name) }, el('b', {}, 'Add from waiting…'),
        el('small', {}, 'Pictures and recordings already copied into the Journey')),
      el('button', { class: 'slot', type: 'button', onclick: () => typeNote(id) }, el('b', {}, 'Type a note…'),
        el('small', {}, 'A typed note counts as the memory\'s text')),
      el('button', { class: 'slot', type: 'button', onclick: () => openStoryteller({ memory: id, title: v.name,
        coverId: (v.items.find((it) => it.cover && it.kind === 'picture') || v.items.find((it) => it.kind === 'picture') || {}).id }) },
        el('b', {}, 'Record someone telling it'), el('small', {}, 'Opens the large recording screen for the storyteller'))),
    v.cover_chosen ? el('p', { class: 'quiet' }, 'The cover was chosen by hand.') : n ? el('p', { class: 'quiet' }, 'The first picture (or video) is the cover unless you choose another. Recordings and notes can\'t be the cover.') : null);

  // Details
  const f = {
    name: textField('Title', v.name, { required: true }),
    date: dateField('Date', v.date),
    event_id: selectField('Event', v.event ? v.event.id : '', [['', 'None'], ...pe.events.map((e) => [e.id, e.note ? `${e.name} (${e.note})` : e.name])]),
    caption: textField('Caption', v.caption, { wide: true, multiline: true, hint: 'A sentence or two. Longer writing can become a typed note.' }),
  };
  // Place (sprint 12, Jared): suggested as you type; a new name becomes a new place at once. Saved when picked.
  const placePick = picker('Place', { kinds: ['place'], create: ['place'], keep: true,
    onPick: async (pid, pname) => {
      if (!pid) return;
      try { await act(invoke('update_memory', { id, changes: { place_id: pid } }), `The place is ${pname}.`); reload(); } catch { /* shown */ }
    } });
  const placeInput = $('input', placePick);
  if (v.place) { placeInput.value = v.place.name; placeInput.classList.add('picked'); }
  const placeBox = el('div', { class: 'field' }, placePick,
    v.place ? el('button', { class: 'link', type: 'button', onclick: async () => {
      try { await act(invoke('update_memory', { id, changes: { place_id: '' } }), 'The place is cleared.'); reload(); } catch { /* shown */ }
    } }, 'No place', el('span', { class: 'sr' }, ` for ${v.name}`)) : null);
  const form = el('form', { class: 'form', novalidate: true }, f.name, f.date, placeBox, f.event_id, f.caption,
    v.long_caption ? el('p', { class: 'warn wide' }, 'This caption is long. ',
      el('button', { class: 'link', type: 'button', onclick: async () => {
        await act(invoke('add_note', { memory: id, title: '', text: v.caption, fromCaption: true }), 'The caption is now a typed note in this memory.'); reload();
      } }, 'Make it a typed note'), ' so it is kept as text in the export.') : null,
    saveButton());
  keepTyped(f, (ch) => invoke('update_memory', { id, changes: ch }));
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const ch = changed(f);
    if (!Object.keys(ch).length) { toast('Nothing has changed.'); return; }
    try { await act(invoke('update_memory', { id, changes: ch }), 'Saved.'); settle(f); reload(); } catch { /* shown */ }
  });

  // People, pets and families with a role
  const roleWords = (p) => `${p.role === 'other' ? p.role_note : p.role}${p.probably ? ' (probably)' : ''}`;
  const people = section('People, pets and families in this memory',
    lines(v.people, (p) => {
      const li = el('li', {}, el('span', { class: 'avatar', 'aria-hidden': 'true' }, initials(p.name)),
        el('span', { class: 't' }, el('button', { class: 'link', type: 'button', onclick: () => openItem(p.id) }, p.name), el('small', {}, roleWords(p))),
        smallBtn('Change…', `${p.name}'s role`, () => editRole(li, v, p)),
        smallBtn('Remove', `${p.name} (${roleWords(p)})`, async () => {
          await act(invoke('unlink_from_memory', { memory: id, who: p.id, role: p.role }), `${p.name} is no longer linked to this memory.`); reload(); }));
      return li;
    }, 'No one linked yet.'),
    addRole(v));

  const stories = section('In stories',
    lines(v.stories, (s) => linkLine(s, smallBtn('Take out of story', s.name, async () => {
      await act(invoke('remove_memory_from_story', { story: s.id, memory: id }), `Taken out of ${s.name}.`); reload(); })), 'Not in a story yet. In the export it shows under "Assorted memories".'),
    el('div', { class: 'addline' }, picker('Add to a story', { kinds: ['story'], create: ['story'], exclude: v.stories.map((s) => s.id),
      onPick: async (sid, sname) => { await act(invoke('add_memories_to_story', { story: sid, memories: [id] }), `Added to ${sname}.`); reload(); } })));

  // "Could add": each word jumps to its box. Never required.
  const jump = { 'a date': () => $('input', f.date), 'a place': () => placeInput, people: () => $('input', people),
    'a caption': () => $('textarea, input', f.caption), 'a story': () => $('input', stories) };
  const couldAdd = suggestOn && v.could_add.length ? el('p', { class: 'could' }, 'This memory could use ',
    ...v.could_add.flatMap((w, i) => [i ? (i === v.could_add.length - 1 ? ' or ' : ', ') : '',
      el('button', { class: 'link', type: 'button', onclick: () => { const box = jump[w]?.(); box?.scrollIntoView({ block: 'center' }); box?.focus(); } }, w)]),
    '. Only if you know it; nothing here is required. ',
    smallBtn('Mark as finished', v.name, () => setFinished(id, v.name, true))) : null;
  // Idea 55: a finished memory stops asking for more until it is unmarked.
  const finished = v.finished ? el('p', { class: 'could' }, 'Finished. The app won\'t suggest more for this memory. ',
    smallBtn('Not finished after all', v.name, () => setFinished(id, v.name, false))) : null;

  // Proposed values (sprint 10): each is one click to use, never applied on its own.
  const what = { name: 'Title', date: 'Date', place_id: 'Place', person: 'Person' };
  const proposals = proposed.length ? section('Suggestions',
    el('ul', { class: 'proposals' }, ...proposed.map((p) => el('li', {},
      el('span', {}, `${what[p.field]}: `, el('b', {}, p.words), ' ', el('small', {}, `(${p.source})`)),
      smallBtn('Use it', `${what[p.field]} ${p.words}`, async () => {
        try {
          if (p.field === 'person') await act(invoke('link_to_memory', { memory: id, who: p.value, role: p.role, roleNote: null, probably: false }), `${p.words} is linked to this memory.`);
          else await act(invoke('update_memory', { id, changes: { [p.field]: p.value } }), 'Saved.');
          reload();
        } catch { /* shown */ }
      }))))) : null;

  // Parts with nothing to show are null; replaceChildren would print them as "null".
  $('#memory-body').replaceChildren(...[header('h-memory', v.name, `Memory${inStories ? ` in ${inStories}` : ''}`, id), memoryNavBar(id),
    couldAdd, proposals, finished, transcriptProgress(v.items), items, form, people, stories, featuredSwitch(id, v.featured, v.name)].filter((x) => x != null));
};

function roleFields(v, current = null) {
  const roleId = nextId('r');
  const sel = el('select', { id: roleId }, ...v.roles.map((r) => el('option', { value: r, selected: current && current.role === r }, r === 'other' ? 'Other…' : cap(r))));
  const note = textField('What "other" means here', current ? current.role_note : '', { hint: 'For example "groom" or "set up the camera".' });
  const probably = el('input', { type: 'checkbox', id: nextId('c'), checked: current ? current.probably : false });
  const sync = () => { note.hidden = sel.value !== 'other'; };
  sel.addEventListener('change', sync);
  sync();
  return { sel, note, probably, box: [el('div', { class: 'field' }, el('label', { for: roleId }, 'Role'), sel), note,
    el('label', { class: 'switch', for: probably.id }, probably, 'Probably')] };
}

function editRole(li, v, p) {
  if ($('.edit', li)) { $('.edit', li).remove(); return; }
  const r = roleFields(v, p);
  const box = el('form', { class: 'edit' }, ...r.box, el('button', { class: 'btn primary', type: 'submit' }, 'Save'));
  box.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    try {
      await act(invoke('update_memory_link', { memory: v.id, who: p.id, oldRole: p.role, role: r.sel.value,
        roleNote: r.note.input.value.trim() || null, probably: r.probably.checked }), 'Saved.');
      reload();
    } catch { /* shown */ }
  });
  li.append(box);
  r.sel.focus();
}

function addRole(v) {
  const r = roleFields(v);
  // Several at once, separated by commas, as on Waiting (sprint 12, Jared).
  const box = namesBox('Who');
  const form = el('form', { class: 'addline', 'aria-label': 'Add people, pets or families' }, el('div', { class: 'field' }, ...box.nodes), ...r.box,
    el('button', { class: 'btn primary', type: 'submit' }, 'Add'));
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    if (!box.parts().length) { toast('Type who to add first.'); return; }
    const unclear = box.unclear();
    if (unclear.length) { toast(`More than one person is called ${unclear.join(', ')}. Choose the right one under the box.`); return; }
    try {
      const out = await act(invoke('link_names_to_memory', { memory: v.id, names: box.input.value, role: r.sel.value,
        roleNote: r.note.input.value.trim() || null, probably: r.probably.checked }));
      const notes = ['Linked to this memory.'];
      if (out.added.length) notes.push(`Added as new: ${out.added.join(', ')}.`);
      if (out.unsure.length) notes.push(`Left out (more than one): ${out.unsure.join(', ')}.`);
      toast(notes.join(' '), { label: 'Undo', run: undoNow });
      reload();
    } catch { /* shown */ }
  });
  return form;
}

async function typeNote(memory) {
  const title = textField('Title (optional)', '', { hint: 'Left empty, the first words are used.' });
  const text = textField('Note', '', { multiline: true });
  text.input.rows = 8;
  const ok = await modal('Type a note', [title, text], { ok: 'Add note', wide: true, check: async () => {
    if (!text.input.value.trim()) return 'The note is empty.';
    await act(invoke('add_note', { memory, title: title.input.value, text: text.input.value, fromCaption: false }), 'The note is added.');
    return true;
  } });
  if (ok) reload();
}

// Select waiting items, then run use(ids). Resolves true when something was done.
// Memories already in the Journey as tiles with a preview: the cover, the
// date, who is in it and its first words; a box filters them (sprint 13).
// multi: tick several, else choose one. Returns { box, chosen() }.
function memoryChooser(rows, { multi = true } = {}) {
  const group = nextId('cm');
  const items = rows.map((m) => {
    const c = el('input', { type: multi ? 'checkbox' : 'radio', name: group, id: nextId('cm'), value: m.id });
    const li = el('li', {}, c, storyThumb(m), el('label', { for: c.id, class: 't' }, m.name,
      el('small', {}, [m.date || 'No date yet', m.who].filter(Boolean).join(' · ')),
      m.words ? el('small', { class: 'words' }, m.words) : null));
    return { m, c, li, text: `${m.name} ${m.date} ${m.who} ${m.words}`.toLowerCase() };
  });
  const find = textField('Find a memory', '', { hint: 'Part of a title, a name, a date or a word from the caption.' });
  const count = el('p', { class: 'quiet', role: 'status' });
  const list = el('ul', { class: 'raw pick memories' }, ...items.map((i) => i.li));
  const none = el('p', { class: 'quiet', hidden: true }, 'No memory matches.');
  const chosen = () => items.filter((i) => i.c.checked).map((i) => i.m);
  find.input.addEventListener('input', () => {
    const words = find.input.value.toLowerCase().split(/\s+/).filter(Boolean);
    let shown = 0;
    for (const i of items) { i.li.hidden = !words.every((w) => i.text.includes(w)) && !i.c.checked; if (!i.li.hidden) shown += 1; }
    none.hidden = shown > 0;
  });
  list.addEventListener('change', () => { if (multi) { const n = chosen().length; count.textContent = n ? `${plural(n, 'memory', 'memories')} chosen.` : ''; } });
  // Enter in the box doesn't submit the dialog before anything is chosen.
  find.input.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !chosen().length) e.preventDefault(); });
  return { box: [find, list, none, count], chosen };
}

// Choose memories already in the Journey in a dialog. Resolves with the
// chosen memory rows (from memory_list), or [] when canceled.
async function chooseMemories(title, { exclude = [], multi = true, okLabel = 'Add', hint = '' } = {}) {
  const rows = (await invoke('memory_list')).filter((m) => !exclude.includes(m.id));
  if (!rows.length) { toast('There are no other memories in the Journey yet.'); return []; }
  const ch = memoryChooser(rows, { multi });
  let out = [];
  const ok = await modal(title, [hint ? el('p', { class: 'quiet' }, hint) : null, ...ch.box].filter(Boolean), { ok: okLabel, wide: true, check: async () => {
    out = ch.chosen();
    return out.length ? true : (multi ? 'Tick at least one memory.' : 'Choose a memory.');
  } });
  return ok ? out : [];
}

// Merge each memory's preview (cover, first words, people) into rows that only have IDs.
async function withPreviews(list) {
  const covers = await memoryCovers();
  return list.map((m) => ({ ...m, ...covers.get(m.id) }));
}

async function pickWaiting(title, hint, okLabel, use) {
  const items = await invoke('raw_content', { kind: 'waiting' });
  if (!items.length) { toast('Nothing is waiting. Bring pictures and recordings in on the Waiting screen first.'); return false; }
  const boxes = items.map((it) => {
    const c = el('input', { type: 'checkbox', id: nextId('w'), value: it.id });
    const thumb = el('span', { class: 'thumb', 'aria-hidden': 'true' }, (KIND_NAME[it.kind] || '?')[0]);
    thumbInto(thumb, it);
    return { c, li: el('li', {}, c, thumb, el('label', { for: c.id, class: 't' }, it.name, el('small', {}, `${KIND_NAME[it.kind] || it.kind} · ${it.date || 'date not known'}`))) };
  });
  const list = el('ul', { class: 'raw pick' }, ...boxes.map((b) => b.li));
  return modal(title, [el('p', { class: 'quiet' }, hint), list],
    { ok: okLabel, wide: true, check: async () => {
      const ids = boxes.filter((b) => b.c.checked).map((b) => b.c.value);
      if (!ids.length) return 'Select at least one.';
      await use(ids);
      return true;
    } });
}

// Choose waiting items to put into this memory.
async function addFromWaiting(memory, name) {
  const ok = await pickWaiting(`Add to ${name}`, 'Select the pictures and recordings to add. They go in at the end, in this order.', 'Add',
    (ids) => act(invoke('add_to_memory', { memory, content: ids }), `${plural(ids.length, 'item is', 'items are')} added.`));
  if (ok) reload();
}

// From a person's or pet's page: waiting items into a new memory about them
// (the same as "Assign to an individual" on the Waiting screen).
async function assignFromWaiting(who, name) {
  let made = null;
  const ok = await pickWaiting(`Add from waiting to ${name}`, `This makes a memory about ${name} holding the ticked items, with a role you can change later.`, 'Done',
    async (ids) => { made = await act(invoke('assign_to_individual', { who, content: ids }), `A memory about ${name} is made.`); });
  if (!ok) return;
  reload();
  await whatNext(made);
}

// From a story's page: waiting items into a new memory at the end of the
// story (as "Add from waiting" does on a person's page). The memory is
// named after the first item ticked, and can be renamed after.
async function storyFromWaiting(story, name) {
  let made = null;
  const ok = await pickWaiting(`Add from waiting to ${name}`, `This makes a new memory in ${name} holding the ticked items.`, 'Done',
    async (ids) => {
      const items = await invoke('raw_content', { kind: 'waiting' });
      const first = items.find((it) => it.id === ids[0]) || {};
      const title = first.title || (first.name || 'New memory').replace(/\.[a-z0-9]{2,4}$/i, '');
      made = await act(invoke('new_memory_with', { name: title, content: ids, story }), `A memory is made in ${name}.`);
    });
  if (!ok) return;
  reload();
  await whatNext(made);
}

// From a person's, pet's or family's page: link a memory already in the Journey, with a role.
function linkMemoryForm(who) {
  const start = async () => {
    const linked = (await invoke('memory_list')).filter((m) => m.people.includes(who)).map((m) => m.id);
    const picked = await chooseMemories('Link memories', { exclude: linked, okLabel: 'Next',
      hint: 'Tick the memories this person, pet or family belongs in. You choose their role next.' });
    if (!picked.length) return;
    const r = roleFields(await invoke('memory_view', { id: picked[0].id }));
    const ok = await modal(`Role in ${picked.length === 1 ? picked[0].name : plural(picked.length, 'memory', 'memories')}`, r.box, { ok: 'Link', check: async () => {
      for (const m of picked) {
        await invoke('link_to_memory', { memory: m.id, who, role: r.sel.value, roleNote: r.note.input.value.trim() || null, probably: r.probably.checked });
      }
      return true;
    } });
    if (ok) { toast(`Linked to ${picked.length === 1 ? picked[0].name : plural(picked.length, 'memory', 'memories')}.`); reload(); }
  };
  return el('div', { class: 'btns' }, el('button', { class: 'btn', type: 'button', onclick: start }, 'Link memories already in the Journey…'));
}

// Place and event pages: bring memories here. A memory has one place and one
// event, so one that already has another is moved, after a warning.
async function addMemoriesTo(field, id, name) {
  const word = field === 'place_id' ? 'place' : 'event';
  const picked = await chooseMemories(`Add memories to ${name}`, { exclude: [],
    hint: `Tick the memories that belong ${field === 'place_id' ? 'at' : 'to'} ${name}.` });
  const moving = picked.filter((m) => m[field] && m[field] !== id);
  const fresh = picked.filter((m) => m[field] !== id);
  if (!fresh.length) { if (picked.length) toast(`Those are already at ${name}.`); return; }
  if (moving.length) {
    const ok = await dialog.ask(`${moving.map((m) => m.name).join(', ')} already ${moving.length === 1 ? 'has' : 'have'} another ${word}.\n\nA memory has one ${word}, so ${moving.length === 1 ? 'it moves' : 'they move'} to ${name}.`,
      { title: `Change the ${word}`, kind: 'warning', okLabel: 'Move them here', cancelLabel: 'Cancel' });
    if (!ok) return;
  }
  for (const m of fresh) await invoke('update_memory', { id: m.id, changes: { [field]: id } });
  toast(`${fresh.length === 1 ? fresh[0].name : plural(fresh.length, 'memory', 'memories')} added to ${name}.`);
  reload();
}

openers.story = (id) => go('story', { id });
openers.memory = (id) => go('memory', { id });

// ---------- Waiting: put selected items into a memory, or assign them to someone ----------

async function putIntoMemory(ids, firstName) {
  const [memories, stories] = await Promise.all([invoke('memories'), invoke('stories')]);
  const newId = nextId('m');
  const oldId = nextId('m');
  const fresh = el('input', { type: 'radio', name: 'where', id: newId, checked: true });
  const existing = el('input', { type: 'radio', name: 'where', id: oldId, disabled: memories.length === 0 });
  const title = textField('Title of the new memory', firstName.replace(/\.[a-z0-9]{2,4}$/i, ''));
  const rows = memories.length ? await invoke('memory_list') : [];
  const chooser = memoryChooser(rows, { multi: false });
  const which = el('div', { class: 'field' }, ...chooser.box);
  const story = selectField('In a story (optional)', '', [['', 'No story yet (shows under "Assorted memories")'], ...stories.map((s) => [s.id, s.name])]);
  const sync = () => { title.hidden = !fresh.checked; story.hidden = !fresh.checked; which.hidden = fresh.checked; };
  fresh.addEventListener('change', sync);
  existing.addEventListener('change', sync);
  sync();
  let made = null;
  const ok = await modal(`Put ${plural(ids.length, 'item', 'items')} into a memory`, [
    el('fieldset', { class: 'choice' }, el('legend', {}, 'Into'),
      el('label', { class: 'switch', for: newId }, fresh, 'A new memory'),
      el('label', { class: 'switch', for: oldId }, existing, 'A memory already in the Journey')),
    title, story, which], { ok: 'Put it in', wide: true, check: async () => {
    if (fresh.checked) {
      const name = title.input.value.trim();
      if (!name) return 'Type a title for the new memory.';
      const s = story.input.value || null;
      made = await act(invoke('new_memory_with', { name, content: ids, story: s }),
        `${name} is made with ${plural(ids.length, 'item', 'items')}${s ? ` in ${stories.find((x) => x.id === s).name}` : ''}.`);
    } else {
      made = chooser.chosen()[0]?.id;
      if (!made) return 'Choose the memory.';
      await act(invoke('add_to_memory', { memory: made, content: ids }), `${plural(ids.length, 'item is', 'items are')} added to the memory.`);
    }
    return true;
  } });
  if (!ok) return null;
  await whatNext(made);
  return made;
}

// After a memory is made or filled from Home or Waiting: open it, or carry on.
async function whatNext(memory) {
  const back = { home: 'Back to Home', waiting: 'Back to Waiting' }[current] || 'Stay on this page';
  const open = await modal('The memory is ready', [el('p', {}, 'Open it to add a story, people, a date or a place, or carry on where you were.')],
    { ok: 'Open this memory', cancel: back });
  if (open) go('memory', { id: memory });
}

// A names box (sprint 12, Jared): several names separated by commas, each
// matched to a person, pet or family; a name not in the Journey becomes a
// new person; same-named people show their years to pick from.
function namesBox(labelText) {
  let all = [];
  const ready = invoke('name_hints').then((x) => { all = x; paint(); }).catch(() => {});
  const id = nextId('names');
  const input = el('input', { id, autocomplete: 'off', placeholder: 'For example: June Rowan, Walter, Cousin Pat', 'aria-describedby': `${id}-hint` });
  const hint = el('p', { class: 'quiet', id: `${id}-hint`, 'aria-live': 'polite' });
  const suggest = el('div', { class: 'chips' });
  const segs = () => input.value.split(/[,;]/);
  const parts = () => segs().map((n) => n.trim().replace(/\s+/g, ' ')).filter(Boolean);
  const lower = (s) => s.toLowerCase();
  const twins = (h) => all.filter((x) => lower(x.name) === lower(h.name)).length > 1;
  const label = (h) => (twins(h) ? `${h.name} (${h.hint})` : h.name);
  const match = (typed) => {
    const m = typed.match(/^(.*)\(([^)]*)\)$/);
    const n = m ? m[1].trim() : typed;
    let found = all.filter((h) => lower(h.name) === lower(n));
    if (!found.length) found = all.filter((h) => lower(h.name.split(' ')[0]) === lower(n));
    if (m && found.length > 1) {
      const said = lower(m[2].trim());
      const picked = found.filter((h) => lower(h.hint) === said || (/^\d{4}$/.test(said) && h.hint.includes(said)));
      if (picked.length) found = picked;
    }
    return found;
  };
  const replaceSeg = (index, text) => {
    const list = segs().map((x) => x.trim());
    list[index] = text;
    input.value = list.filter(Boolean).join(', ') + ', ';
    input.focus();
    paint();
  };
  const paint = () => {
    const chips = [];
    const words = [];
    segs().forEach((raw, index) => {
      const n = raw.trim().replace(/\s+/g, ' ');
      if (!n) return;
      const m = match(n);
      if (m.length === 1) words.push(label(m[0]));
      else if (!m.length) words.push(`${n} (new person)`);
      else {
        words.push(`${n} (more than one: choose below)`);
        for (const h of m) chips.push(el('button', { class: 'btn', type: 'button', onclick: () => replaceSeg(index, label(h)) }, label(h)));
      }
    });
    hint.textContent = words.length ? `Will be in the memory: ${words.join(', ')}.` : 'Type one name, or several separated by commas.';
    // Suggest names for the part being typed.
    const last = lower(segs().pop().trim());
    if (!chips.length && last) {
      for (const h of all.filter((x) => lower(x.name).split(/\s+/).some((w) => w.startsWith(last)) && lower(label(x)) !== last).slice(0, 6)) {
        chips.push(el('button', { class: 'btn', type: 'button', onclick: () => replaceSeg(segs().length - 1, label(h)) }, label(h)));
      }
    }
    suggest.replaceChildren(...chips);
  };
  input.addEventListener('input', paint);
  paint();
  input.addEventListener('input', paint);
  paint();
  return { ready, input, parts, match, nodes: [el('label', { for: id }, labelText), input, suggest, hint],
    unclear: () => parts().filter((n) => match(n).length > 1) };
}

// Several people at once, separated by commas (sprint 12, Jared): a name
// not in the Journey yet becomes a new person straight away.
async function assignToSomeone(ids) {
  // Same-named people are told apart by their years (Jared, 2026-11-01):
  // "Ada Hale (born 1912)" is not "Ada Hale (born 2015)".
  const box = namesBox('People or pets');
  await box.ready;
  const { input, parts, match } = box;
  let made = null;
  const shown = modal(`Assign ${plural(ids.length, 'item', 'items')} to people`, [
    el('p', { class: 'quiet' }, 'This makes one memory about them holding the items, with a role you can change later.'),
    ...box.nodes],
  { ok: 'Done', check: async () => {
    if (!parts().length) return 'Type at least one name.';
    const unclear = parts().filter((n) => match(n).length > 1);
    if (unclear.length) return `More than one person is called ${unclear.join(', ')}. Choose the right one under the box.`;
    const r = await act(invoke('assign_to_names', { names: input.value, content: ids }));
    made = r.memory;
    const notes = ['A memory about them is made.'];
    if (r.added.length) notes.push(`Added as new: ${r.added.join(', ')}.`);
    if (r.unsure.length) notes.push(`More than one person is called ${r.unsure.join(', ')}; add the right one in the memory.`);
    toast(notes.join(' '), { label: 'Undo', run: undoNow });
    return true;
  } });
  setTimeout(() => input.focus(), 0);
  if (!(await shown)) return null;
  await whatNext(made);
  return made;
}

// ---------- Large preview (Waiting and the memory screen) ----------

let viewer = null;

// actions(item), when given, returns [label, run] pairs shown under the
// item, such as the Waiting list's own buttons (sprint 7). The viewer closes
// before one runs, since most open a dialog or move the item away.
async function openViewer(list, index, context = '', actions = null) {
  if (!viewer) {
    viewer = el('dialog', { class: 'viewer', 'aria-labelledby': 'viewer-h' });
    document.body.append(viewer);
    viewer.addEventListener('keydown', (e) => {
      if (e.target.closest('audio, video') && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) return;
      if (e.key === 'ArrowRight') { e.preventDefault(); viewer.step(1); }
      if (e.key === 'ArrowLeft') { e.preventDefault(); viewer.step(-1); }
    });
    viewer.addEventListener('close', () => { $('audio, video', viewer)?.pause(); viewer.replaceChildren(); });
  }
  let at = index;
  const show = async () => {
    const it = list[at];
    let s;
    try { s = await invoke('show_content', { id: it.id }); } catch (e) { showError(e); return; }
    let body;
    if (s.text !== null && s.text !== undefined) body = el('pre', { class: 'text', tabindex: '0' }, s.text);
    else if (!s.path) body = el('p', { class: 'warn' }, 'The file for this item is missing from the Journey folder.');
    else {
      const src = fileUrl(s.path);
      if (s.kind === 'picture') body = el('img', { src, alt: s.name, class: it.turn ? `turn-${it.turn}` : null });
      else if (s.kind === 'video') body = el('video', { src, controls: true, autoplay: true });
      else {
        // A quiet start is skipped (sprint 10); the file itself is whole.
        body = el('audio', { src: s.start ? `${src}#t=${s.start}` : src, controls: true, autoplay: true });
        if (s.quiet) {
          const full = !s.start;
          body = el('div', { class: 'stack' }, body, el('p', { class: 'quiet' },
            full ? `This recording starts with ${s.quiet} seconds of quiet. ` : `The first ${s.quiet} seconds of quiet are skipped; the recording itself is kept whole. `,
            smallBtn(full ? 'Skip the quiet start' : 'Play the whole recording', s.name, async () => {
              try { await act(invoke('set_play_full', { id: it.id, full: !full }), full ? 'The quiet start is skipped.' : 'The whole recording plays.'); show(); } catch { /* shown */ }
            })));
        }
      }
    }
    const close = el('button', { class: 'btn', type: 'button', onclick: () => viewer.close() }, 'Close');
    viewer.replaceChildren(...[
      el('header', {}, el('div', {}, el('h2', { id: 'viewer-h' }, s.name),
        el('p', { class: 'quiet' }, [KIND_NAME[s.kind] || s.kind, s.date || 'date not known', context, list.length > 1 ? `${at + 1} of ${list.length}` : ''].filter(Boolean).join(' · '))),
      el('div', { class: 'btns' },
        list.length > 1 ? el('button', { class: 'btn', type: 'button', onclick: () => viewer.step(-1), disabled: at === 0 }, '← Previous') : null,
        list.length > 1 ? el('button', { class: 'btn', type: 'button', onclick: () => viewer.step(1), disabled: at === list.length - 1 }, 'Next →') : null,
        close)),
      el('div', { class: 'stage' }, body),
      actions ? el('div', { class: 'btns viewer-actions', role: 'group', 'aria-label': `With ${s.name}` },
        ...actions(it).map(([label, run], i) => el('button', { class: `btn${i === 0 ? ' primary' : ''}`, type: 'button',
          onclick: () => { viewer.close(); run(); } }, label))) : null,
      await viewerTranscript(it),
      list.length > 1 ? el('p', { class: 'quiet' }, 'Left and right arrow keys move between items; Esc closes.') : null].filter(Boolean));
    if (!viewer.open) viewer.showModal();
    close.focus();
  };
  viewer.step = (d) => { const next = at + d; if (next >= 0 && next < list.length) { at = next; show(); } };
  await show();
}

// ---------- Home: the Journey's title and introduction, featured order ----------

$('#edit-journey').addEventListener('click', async () => {
  const h = await invoke('home');
  const name = textField('Title', h.name, { hint: 'For example "The Rowan Family". It is the title of the exported history.' });
  const intro = textField('Introduction', h.introduction, { multiline: true, hint: 'Opens the exported history.' });
  intro.input.rows = 5;
  const ok = await modal('The Journey', [name, intro], { ok: 'Save', wide: true, check: async () => {
    if (!name.input.value.trim()) return 'The Journey needs a title.';
    const ch = {};
    if (name.input.value.trim() !== h.name) ch.name = name.input.value.trim();
    if (intro.input.value.trim() !== h.introduction) ch.introduction = intro.input.value.trim();
    if (Object.keys(ch).length) await act(invoke('update_journey', { changes: ch }), 'Saved.');
    return true;
  } });
  if (ok) loadHome();
});

async function moveFeatured(id, d) {
  await act(invoke('move_featured', { id, delta: d }), 'Order changed.');
  loadHome();
}

// Mark a memory finished or not, from the memory screen or Home.
async function setFinished(id, name, on) {
  try {
    await act(invoke('update_memory', { id, changes: { finished: on } }), on ? `${name} is marked finished.` : `${name} is no longer marked finished.`);
    go(current, { push: false, focus: false, id: currentId });
  } catch { /* shown */ }
}
