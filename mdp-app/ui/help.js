// Sprint 14: the getting-started card on Home and the Help panel.
// Help pages live on the website, so a fix needs no app update; they open in
// the browser. The card's state is kept per Journey on this computer only
// (localStorage), so nothing in the Journey or its catalog changes.

const SITE = 'https://moderndaypioneer.org/';
const openSite = (path) => invoke('open_link', { url: SITE + path }).catch(showError);
const openHelp = (page = '') => openSite(`help/${page}`);

// The five steps (2026-11-04: "Add to a story" in place of "Record a story").
const GETTING_STEPS = [
  { label: 'Add a person', page: 'add-people/', done: (c) => c.people > 0 },
  { label: 'Bring in photos', page: 'bring-in-photos/', done: (c) => c.pictures > 0 },
  { label: 'Make a memory', page: 'make-a-memory/', done: (c) => c.memories > 0 },
  { label: 'Add to a story', page: 'make-a-story/', done: (c) => c.in_stories > 0 },
  { label: 'Preview your history', page: 'preview-and-export/', done: (c, seen) => seen },
];

let gettingJourney = null;
let gettingThanking = false; // the thank-you shows once, in this session only
const gettingKey = (what) => `mdp-getting-${what}:${gettingJourney}`;
function gettingGet(what) { try { return localStorage.getItem(gettingKey(what)); } catch { return null; } }
function gettingSet(what, v) { try { localStorage.setItem(gettingKey(what), v); } catch { /* not kept */ } }

function markPreviewed() { if (gettingJourney) gettingSet('previewed', '1'); }

function renderGetting(h) {
  gettingJourney = h.id;
  let state = gettingGet('state');
  // A Journey that already has memories starts with the card hidden
  // (2026-11-04); Help can show it.
  if (!state) { state = h.counts.memories > 0 ? 'hidden' : 'shown'; gettingSet('state', state); }
  const seen = gettingGet('previewed') === '1';
  const done = GETTING_STEPS.map((s) => s.done(h.counts, seen));
  const n = done.filter(Boolean).length;
  if (state === 'shown' && n === GETTING_STEPS.length) { state = 'thanked'; gettingSet('state', state); gettingThanking = true; }
  const thanks = state === 'thanked' && gettingThanking;
  $('#getting').hidden = state === 'hidden' || (state === 'thanked' && !gettingThanking) || h.read_only;
  $('#getting-list').hidden = thanks;
  $('#getting-done').hidden = !thanks;
  if (thanks) return;
  $('#getting-count').textContent = `${n} of ${GETTING_STEPS.length} done`;
  const next = done.indexOf(false);
  $('#getting-steps').replaceChildren(...GETTING_STEPS.map((s, i) => el('li', { class: done[i] ? 'done' : i === next ? 'next' : '' },
    el('span', { class: 'tick', 'aria-hidden': 'true' }, done[i] ? '✓' : ''),
    el('span', { class: 't' }, s.label, done[i] ? el('small', {}, ' · Done') : i === next ? el('small', {}, ' · next') : null),
    done[i] ? null : el('button', { class: 'link', type: 'button', onclick: () => openHelp(s.page) }, 'Show me', el('span', { class: 'sr' }, `: ${s.label}`)))));
}

function hideGetting() { gettingSet('state', 'hidden'); gettingThanking = false; $('#getting').hidden = true; }
$('#getting-hide').addEventListener('click', () => { hideGetting(); toast('Hidden. Help can show the getting-started card again.'); });
$('#getting-gotit').addEventListener('click', hideGetting);
$('#getting-help').addEventListener('click', () => $('#help-dialog').showModal());

$('#help-open').addEventListener('click', () => $('#help-dialog').showModal());
for (const b of $$('#help-dialog [data-help]')) b.addEventListener('click', () => openHelp(b.dataset.help));
for (const b of $$('#help-dialog [data-site]')) b.addEventListener('click', () => openSite(b.dataset.site));
$('#help-feedback').addEventListener('click', () => $('#send-feedback').click());
$('#help-getting').addEventListener('click', () => {
  if (gettingJourney) gettingSet('state', 'again'); // the list, even when every step is done
  gettingThanking = false;
  $('#help-dialog').close();
  go('home');
});

// "?" beside the few words new to people.
const TERMS = {
  journey: ['Journey', 'A Journey holds one family history: its people, pictures, recordings, memories and stories. Most families need only one.', 'start-a-journey/'],
  story: ['Memories and stories', 'A memory is one moment: a few photos or recordings, with who, when and where. A story gathers memories, in order, around an event, a person or a group, and becomes a book in the exported history.', 'make-a-story/'],
  waiting: ['Waiting', 'Pictures and recordings you bring in wait here until they go into a memory. They are kept safe, but left out of the export until then.', 'make-a-memory/'],
};
let termPage = '';
for (const b of $$('button.term')) b.addEventListener('click', () => {
  const [h, text, page] = TERMS[b.dataset.term];
  $('#term-h').textContent = h;
  $('#term-text').textContent = text;
  termPage = page;
  $('#term-dialog').showModal();
});
$('#term-more').addEventListener('click', () => openHelp(termPage));
