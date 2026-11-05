// Screen check for sprint 18: on a wide window every detail page with a long
// list (a memory's photos; the memories of a person, family, story, event or
// place) keeps its details together beside the list, in view while the list
// scrolls, in both looks. On a narrow window the details come first.
// Usage as check8.mjs: node check18b.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8775'], { stdio: ['ignore', 'inherit', 'inherit'], env: { ...process.env, MDP_FFMPEG: process.env.MDP_FFMPEG || 'ffmpeg' } });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
async function axe(name) {
  for (const scheme of ['light', 'dark']) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.waitForTimeout(150);
    await page.evaluate(axeSrc);
    results[`${name} ${scheme}`] = await page.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'best-practice'] }))
      .violations.map((v) => `${v.id}: ${v.nodes.length} (${v.nodes.slice(0, 2).map((n) => n.target.join(' ')).join(' | ')})`));
  }
  await page.emulateMedia({ colorScheme: 'light' });
  await page.screenshot({ path: `${out}/${name}.png` });
}
page.setDefaultTimeout(15000);
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8775/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check18b-'));
const gif = (n) => Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, n, 7]);
const paths = Array.from({ length: 48 }, (_, i) => { const p = path.join(src, `P82704${30 + i}.gif`); fs.writeFileSync(p, gif(40 + i)); return p; });
const imported = await call('test_import', { paths });
const mem = await call('add_memory', { name: 'Baby pictures', story: null });
await call('add_to_memory', { memory: mem, content: imported.slice(0, 24) });
// A person, family, story, event and place, each in 24 memories.
const ada = await call('add_individual', { kind: 'Person', name: 'Ada Rowan' });
const fam = await call('add_family', { name: 'Rowan family' }).catch(() => null);
const story = await call('add_story', { name: 'Summers at the lake' });
const place = await call('add_place', { name: 'Bear Lake' });
const ev = await call('add_event', { kind: 'reunion', name: 'Rowan reunion 1972', people: [ada], when: '1972', place });
for (let i = 0; i < 24; i++) {
  const m = await call('add_memory', { name: `Summer ${1950 + i}`, story });
  await call('add_to_memory', { memory: m, content: [imported[24 + i]] });
  await call('link_to_memory', { memory: m, who: ada, role: 'in the picture', roleNote: null, probably: false });
  if (fam) await call('link_to_memory', { memory: m, who: fam, role: 'in the picture', roleNote: null, probably: false }).catch(() => {});
  await call('update_memory', { id: m, changes: { place_id: place, event_id: ev } }).catch(() => {});
}
const pages = [['memory', mem, '#memory-body .items li'], ['person', ada, '#person-body .page-left li'], ['story', story, '#story-body .page-left li'],
  ['event', ev, '#event-body .page-left li'], ['place', place, '#place-body .page-left li']];
if (fam) pages.push(['family', fam, '#family-body .page-left li']);

for (const look of ['scrapbook', 'system']) {
  await page.evaluate((l) => localStorage.setItem('mdp.look', l), look);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.reload();
  await page.waitForSelector('#journey-title:not(:empty)');
  check(`look-${look}`, await page.evaluate((l) => document.documentElement.dataset.look === l, look));
  for (const [pane, id, rows] of pages) {
    const tag = `${look}-${pane}`;
    await page.evaluate(([p, i]) => go(p, { id: i }), [pane, id]);
    await page.waitForSelector(`#h-${pane}`);
    await page.waitForTimeout(400);
    const body = `#${pane}-body`;
    check(`oneSide-${tag}`, (await page.locator(`${body} > .page-right`).count()) === 1);
    // Side by side, and the details sit together: no tall gap between parts.
    const lay = await page.evaluate((b) => {
      const side = document.querySelector(`${b} > .page-right`), main = document.querySelector(`${b} > .page-left`);
      const kids = [...side.children];
      let worst = 0;
      for (let i = 1; i < kids.length; i++) worst = Math.max(worst, kids[i].getBoundingClientRect().top - kids[i - 1].getBoundingClientRect().bottom);
      return { worst, beside: main.getBoundingClientRect().right <= side.getBoundingClientRect().left, sameTop: Math.abs(main.getBoundingClientRect().top - side.getBoundingClientRect().top) < 30 };
    }, body);
    results[`gap-${tag}`] = lay.worst;
    check(`beside-${tag}`, lay.beside && lay.sameTop);
    check(`tight-${tag}`, lay.worst < 40);
    if (pane === 'memory' || pane === 'person') await axe(`s18b-${tag}-top`);
    // Scroll far down the list: the details are still in view.
    const n = await page.locator(rows).count();
    results[`rows-${tag}`] = n;
    check(`longList-${tag}`, n >= 20);
    await page.locator(rows).nth(Math.floor(n / 2)).scrollIntoViewIfNeeded();
    await page.waitForTimeout(300);
    const inView = await page.evaluate((b) => {
      const side = document.querySelector(`${b} > .page-right`);
      const r = side.getBoundingClientRect();
      let sc = side.parentElement; while (sc && getComputedStyle(sc).overflowY === 'visible') sc = sc.parentElement;
      const box = sc ? sc.getBoundingClientRect() : { top: 0, bottom: window.innerHeight };
      window.__probe = { top: r.top, bottom: r.bottom, boxTop: box.top, boxBottom: box.bottom, scroll: sc && sc.scrollTop, sc: sc && (sc.tagName + sc.className), pos: getComputedStyle(side).position };
      return r.top >= box.top - 1 && r.bottom <= box.bottom + 1 && sc && sc.scrollTop > 100;
    }, body);
    results[`probe-${tag}`] = await page.evaluate(() => window.__probe);
    check(`stillInView-${tag}`, inView);
    if (pane === 'memory' || pane === 'person' || pane === 'story') await page.screenshot({ path: `${out}/s18b-${tag}-scrolled.png` });
  }
}
// A narrow window: one column, details first (except a memory, photos first).
await page.setViewportSize({ width: 900, height: 800 });
await page.evaluate((i) => go('person', { id: i }), ada);
await page.waitForSelector('#h-person');
await page.waitForTimeout(300);
check('narrowDetailsFirst', await page.evaluate(() => {
  const s = document.querySelector('#person-body > .page-right').getBoundingClientRect(), m = document.querySelector('#person-body > .page-left').getBoundingClientRect();
  return s.bottom <= m.top + 1 && getComputedStyle(document.querySelector('#person-body > .page-right')).position === 'static';
}));
await page.screenshot({ path: `${out}/s18b-narrow-person.png` });

const bad = Object.entries(results).filter(([, v]) => Array.isArray(v) && v.length);
console.log(JSON.stringify({ results, errors }, null, 2));
await browser.close();
server.kill();
if (bad.length || errors.length) process.exit(1);
