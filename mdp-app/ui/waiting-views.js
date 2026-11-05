// Waiting at scale (sprint 16): two grouped
// views beside List and Tiles. "By month" shows an album page for each month;
// "By box" puts a box for each month, or for each import, on a shelf for each
// year, and a box opens to its album page. Group first, prints second: only a
// few pages, shelves and prints are drawn until asked for more, so 1,000
// waiting items stay quick. Making a memory still goes through the usual
// memory step; the words are the same in both looks.

const MONTH_NAMES = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const PAGES_AT_ONCE = 6;
const PRINTS_AT_ONCE = 24;
const SHELVES_AT_ONCE = 6;
let pagesShown = PAGES_AT_ONCE;
let shelvesShown = SHELVES_AT_ONCE;
const pagesOpen = new Set(); // pages showing all their prints
let openBox = null; // the box opened on the shelves, by key

const waitingPane = () => $('section.pane[data-pane="waiting"]');
function groupedView() {
  const v = waitingPane().dataset.view;
  return v === 'month' || v === 'box' ? v : null;
}
function boxBy() {
  try { return localStorage.getItem('mdp.boxby') === 'import' ? 'import' : 'month'; } catch { return 'month'; }
}

// The month an item was taken: from its own date, else its file name's date.
function monthOf(it) {
  const d = it.date_sort || '';
  const y = d.slice(0, 4);
  const m = Number(d.slice(5, 7));
  if (/^\d{4}-\d{2}/.test(d) && m >= 1 && m <= 12) return { key: d.slice(0, 7), label: `${MONTH_NAMES[m - 1]} ${y}`, shelf: y };
  if (/^\d{4}/.test(d)) return { key: y, label: `${y}, month not known`, shelf: y };
  return { key: '~', label: 'Date not known', shelf: '' };
}
// The import an item came in with: the day it was brought in and its folder.
function importOf(it) {
  const day = it.added || '';
  const words = /^\d{4}-\d{2}-\d{2}$/.test(day) ? `${Number(day.slice(8))} ${MONTH_NAMES[Number(day.slice(5, 7)) - 1]} ${day.slice(0, 4)}` : day;
  return { key: `${day}|${it.folder}`, label: it.folder || 'Brought in', sub: words ? `Brought in ${words}` : '', shelf: day.slice(0, 4) };
}

// Items into groups, in the order the sort asks for; undated last.
function groupItems(items, by) {
  const groups = new Map();
  for (const it of items) {
    const g = by === 'import' ? importOf(it) : monthOf(it);
    if (!groups.has(g.key)) groups.set(g.key, { ...g, items: [] });
    groups.get(g.key).items.push(it);
  }
  const oldFirst = ['old', 'date'].includes($('#wf-sort').value);
  return [...groups.values()].sort((a, b) => (a.key === '~') - (b.key === '~') || (oldFirst ? a.key.localeCompare(b.key) : b.key.localeCompare(a.key)));
}

function countWords(items) {
  const kinds = new Set(items.map((it) => it.kind));
  if (kinds.size === 1 && kinds.has('picture')) return plural(items.length, 'picture', 'pictures');
  if (kinds.size === 1 && kinds.has('voice recording')) return plural(items.length, 'recording', 'recordings');
  if (kinds.size === 1 && kinds.has('video')) return plural(items.length, 'video', 'videos');
  return plural(items.length, 'item', 'items');
}

// Draws the grouped view into #waiting-pages; paintWaiting has already sorted,
// filtered and counted the items, and keeps the selection bar.
function paintGrouped(items, view) {
  const box = $('#waiting-pages');
  box.dataset.view = view;
  const draw = [];
  if (view === 'month') {
    const pages = groupItems(items, 'month');
    box.replaceChildren(
      ...pages.slice(0, pagesShown).map((g) => albumPage(g, draw)),
      pages.length > pagesShown ? el('button', { class: 'btn more', type: 'button', onclick: () => { pagesShown += PAGES_AT_ONCE; paintWaiting(); } },
        `Show more months (${pages.length - pagesShown} more)`) : '');
  } else {
    const by = boxBy();
    const boxes = groupItems(items, by);
    const open = openBox && boxes.find((g) => g.key === openBox);
    const choose = el('select', { id: 'box-by' }, el('option', { value: 'month', selected: by === 'month' }, 'Month taken'),
      el('option', { value: 'import', selected: by === 'import' }, 'Import'));
    choose.addEventListener('change', () => {
      try { localStorage.setItem('mdp.boxby', choose.value); } catch { /* not kept */ }
      openBox = null; shelvesShown = SHELVES_AT_ONCE; paintWaiting(); $('#box-by')?.focus();
    });
    const head = el('div', { class: 'row shelfbar' }, el('div', { class: 'field' }, el('label', { for: 'box-by' }, 'A box for each'), choose));
    if (open) {
      const back = el('button', { class: 'btn', type: 'button', id: 'box-back', onclick: () => { const k = openBox; openBox = null; paintWaiting(); $(`[data-box="${CSS.escape(k)}"]`)?.focus(); } }, '← Back to the shelves');
      box.replaceChildren(head, el('div', { class: 'row' }, back), albumPage(open, draw, true));
    } else {
      openBox = null;
      const shelves = [];
      for (const g of boxes) {
        const last = shelves[shelves.length - 1];
        if (last && last.shelf === g.shelf) last.boxes.push(g);
        else shelves.push({ shelf: g.shelf, boxes: [g] });
      }
      box.replaceChildren(head,
        el('p', { class: 'quiet shelfnote' }, by === 'import'
          ? 'Each box holds one import: the files brought in from one folder on one day. Open a box to see its pictures and make a memory of all of it.'
          : 'Each box holds one month. Open a box to see its pictures and make a memory of all of it.'),
        ...shelves.slice(0, shelvesShown).map((s) => shelfRow(s, draw)),
        shelves.length > shelvesShown ? el('button', { class: 'btn more', type: 'button', onclick: () => { shelvesShown += SHELVES_AT_ONCE; paintWaiting(); } },
          `Show more years (${shelves.length - shelvesShown} more)`) : '');
    }
  }
  for (const it of draw) loadThumb(it);
}

function shelfRow(s, draw) {
  const n = s.boxes.reduce((t, g) => t + g.items.length, 0);
  const hid = nextId('shelf');
  return el('section', { class: 'shelfrow', 'aria-labelledby': hid },
    el('h2', { class: 'yeartag', id: hid }, s.shelf || 'Date not known', el('small', {}, ` ${plural(n, 'item', 'items')}`)),
    el('ul', { class: 'boxes' }, ...s.boxes.map((g) => {
      const cover = g.items.find((it) => it.kind === 'picture' || it.kind === 'video') || g.items[0];
      draw.push(cover);
      return el('li', {}, el('button', { class: 'wbox', type: 'button', 'data-box': g.key, onclick: () => { openBox = g.key; paintWaiting(); $('#box-back')?.focus(); } },
        el('span', { class: 'boxart', 'aria-hidden': 'true' }, el('span', { class: 'thumb', 'data-thumb': cover.id }, KIND_WORD[cover.kind]?.[0] || '?')),
        el('b', {}, g.label), el('small', {}, [countWords(g.items), g.sub].filter(Boolean).join(' · ')),
        el('span', { class: 'sr' }, '. Open the box')));
    })));
}

// One album page: a tab with the month and "Make a memory of all", then prints.
function albumPage(g, draw, opened = false) {
  const hid = nextId('page');
  const all = opened || pagesOpen.has(g.key) || g.items.length <= PRINTS_AT_ONCE;
  const shown = all ? g.items : g.items.slice(0, PRINTS_AT_ONCE);
  draw.push(...shown);
  const ids = g.items.map((it) => it.id);
  const make = showing === 'waiting'
    ? el('button', { class: 'btn primary', type: 'button', onclick: async () => {
      const made = await putIntoMemory(ids, g.key === '~' ? g.items[0].title || g.items[0].name : g.label);
      if (made) { for (const id of ids) ticked.delete(id); openBox = null; loadWaiting(); }
    } }, `Make a memory of all ${g.items.length}`, el('span', { class: 'sr' }, ` from ${g.label}`))
    : null;
  return el('section', { class: 'albumpage', 'aria-labelledby': hid },
    el('div', { class: 'pagetab' }, el('h2', { id: hid }, g.label), el('small', {}, [countWords(g.items), g.sub].filter(Boolean).join(' · ')), make),
    el('ul', { class: 'prints' }, ...shown.map((it) => printEl(it, g.items))),
    all ? '' : el('button', { class: 'link more', type: 'button', onclick: () => { pagesOpen.add(g.key); paintWaiting(); } },
      `Show all ${g.items.length}`, el('span', { class: 'sr' }, ` in ${g.label}`)));
}

function printEl(it, items) {
  const label = waitingLabel(it);
  const tick = showing === 'waiting' ? el('input', { type: 'checkbox', checked: ticked.has(it.id), 'aria-label': `Select ${label}`,
    onchange: (e) => { if (e.target.checked) ticked.add(it.id); else ticked.delete(it.id); paintTicked(); } }) : null;
  return el('li', { class: 'print' }, tick,
    el('button', { class: 'link', type: 'button', title: waitingLine(it), onclick: () => openViewer(items, items.indexOf(it), '', waitingActions) },
      el('span', { class: 'thumb', 'data-thumb': it.id, 'aria-hidden': 'true' }, KIND_WORD[it.kind]?.[0] || '?'),
      el('span', { class: 'cap' }, label)));
}
