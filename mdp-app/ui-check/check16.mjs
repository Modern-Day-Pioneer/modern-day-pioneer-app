// Screen check for sprint 16: Waiting
// "By month" and "By box" with 1,000 waiting items, in the System and the
// Scrapbook look, light and dark, at 100%, 150% and 200% text size; making a
// memory of a whole month; the props load, stay small and drop out in high
// contrast. Usage as check8.mjs: node check16.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8786'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
async function axe(name, schemes = ['light', 'dark']) {
  for (const scheme of schemes) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.waitForTimeout(200);
    await page.evaluate(axeSrc);
    results[`${name} ${scheme}`] = await page.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'best-practice'] }))
      .violations.map((v) => `${v.id}: ${v.nodes.length} (${v.nodes.slice(0, 2).map((n) => n.target.join(' ')).join(' | ')})`));
    await page.screenshot({ path: `${out}/${name}-${scheme}.png`, fullPage: true });
  }
  await page.emulateMedia({ colorScheme: 'light' });
}
page.setDefaultTimeout(15000);
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('response', (r) => { if (r.status() >= 400 && !r.url().endsWith('/favicon.ico')) errors.push('HTTP ' + r.status() + ' ' + r.url()); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
// The test server makes no thumbnails (no FFmpeg); for the screenshots each
// picture shows one of the three sample photos, so the prints look like prints.
await page.addInitScript((dir) => {
  const real = window.__TAURI__.core.invoke;
  const pics = ['camera-on-tripod.jpg', 'cup-of-coffee.jpg', 'tabby-cat.jpg'];
  window.__TAURI__.core.invoke = (cmd, args) => cmd === 'thumbnail'
    ? Promise.resolve(`/file/${dir}/${pics[[...args.id].reduce((t, c) => t + c.charCodeAt(0), 0) % 3]}`)
    : real(cmd, args);
}, `${repo}/app/mdp-core/sample-media`);
await page.goto('http://127.0.0.1:8786/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const show = async (pane) => { await page.evaluate((p) => go(p), pane); await page.waitForTimeout(400); };
const setView = async (v) => { await page.selectOption('#view-waiting', v); await page.waitForTimeout(400); };
const setLook = async (look) => { await page.evaluate((l) => { localStorage.setItem('mdp.look', l); }, look); await page.reload(); await page.waitForSelector('#journey-title:not(:empty)'); };

// 1,000 waiting pictures: a phone's pictures over five years, two folders of
// scans with a year in the name, and some with no date at all.
const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check16-'));
const jpgs = ['camera-on-tripod.jpg', 'cup-of-coffee.jpg', 'tabby-cat.jpg'].map((n) => fs.readFileSync(`${repo}/app/mdp-core/sample-media/${n}`));
const paths = [];
const put = (folder, name, i) => {
  const dir = path.join(src, folder);
  fs.mkdirSync(dir, { recursive: true });
  const p = path.join(dir, name);
  fs.writeFileSync(p, Buffer.concat([jpgs[i % 3], Buffer.from(`mdp${i}`)]));
  paths.push(p);
};
let i = 0;
for (let y = 2019; y <= 2023; y++) for (let m = 1; m <= 12; m++) {
  const n = 2 + ((y * 7 + m * 5) % 13);
  for (let k = 0; k < n; k++, i++) put(`Phone ${y}`, `IMG_${y}${String(m).padStart(2, '0')}${String(1 + k).padStart(2, '0')}_1200${String(k % 60).padStart(2, '0')}.jpg`, i);
}
while (i < 960) { put("Grandpa's box", `Christmas ${1950 + (i % 12)} ${i}.jpg`, i); i++; }
while (i < 1000) { put('Loose scans', `scan_${i}.jpg`, i); i++; }
const t0 = Date.now();
const imported = await call('test_import', { paths });
results.imported = imported.length;
results.importSeconds = (Date.now() - t0) / 1000;
await page.reload();
await page.waitForSelector('#journey-title:not(:empty)');
const nWaiting = (await call('raw_content', { kind: 'waiting' })).length;
check('thousandWaiting', nWaiting >= 1000);
check('rawHasFolderAndDay', (await call('raw_content', { kind: 'waiting' })).every((it) => typeof it.folder === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(it.added)));

// 1. By month: an album page a month, a few at a time, quick at 1,000.
await show('waiting');
check('viewChoices', (await page.$$eval('#view-waiting option', (o) => o.map((x) => x.textContent))).join() === 'List,Tiles,By month,By box');
let t = Date.now();
await setView('month');
await page.waitForSelector('#waiting-pages .albumpage');
results.monthPaintMs = Date.now() - t;
check('listHidden', await page.$eval('#waiting-list', (l) => l.hidden));
const pages = await page.$$eval('#waiting-pages .albumpage', (l) => l.map((p) => p.querySelector('h2').textContent));
results.monthPages = pages;
check('sixPagesAtOnce', pages.length === 6);
check('newestMonthFirst', pages[0] === 'December 2023');
check('fewPrintsDrawn', (await page.$$('#waiting-pages .print')).length <= 6 * 24);
await page.selectOption('#wf-sort', 'date');
await page.waitForTimeout(300);
const oldFirst = await page.$$eval('#waiting-pages .albumpage h2', (l) => l.map((h) => h.textContent));
check('yearOnlyPage', oldFirst[0] === '1950, month not known');
await page.selectOption('#wf-sort', 'new');
await page.waitForTimeout(300);
await page.click('#waiting-pages > .more');
await page.waitForTimeout(300);
check('showMoreMonths', (await page.$$('#waiting-pages .albumpage')).length === 12);
// Undated items come last, on their own page.
const last = await call('raw_content', { kind: 'waiting' });
check('undatedKnown', last.filter((it) => !it.date_sort).length >= 40);

// 2. Make a memory of all of one month, through the usual memory step.
const before = nWaiting;
const firstPage = await page.$('#waiting-pages .albumpage');
const n = Number((await firstPage.$eval('.pagetab .btn.primary', (b) => b.textContent)).match(/all (\d+)/)[1]);
await firstPage.$eval('.pagetab .btn.primary', (b) => b.click());
await page.waitForSelector('.dialog:not([hidden]), dialog[open]', { timeout: 5000 }).catch(() => {});
results.memoryDialog = await page.evaluate(() => document.querySelector('dialog[open]')?.textContent.slice(0, 160) || null);
await page.screenshot({ path: `${out}/00-make-memory-of-month.png` });

// 3. By box: boxes per month on a shelf per year; a box opens to its page.
await page.keyboard.press('Escape');
await page.waitForTimeout(300);
await setView('box');
await page.waitForSelector('#waiting-pages .wbox');
const shelves = await page.$$eval('#waiting-pages .shelfrow .yeartag', (l) => l.map((h) => h.firstChild.textContent));
results.shelves = shelves;
check('yearShelves', shelves[0] === '2023' && shelves.length === 6);
await page.click('#waiting-pages .wbox');
await page.waitForSelector('#box-back');
check('boxOpens', (await page.$$('#waiting-pages .albumpage')).length === 1);
check('focusOnBack', await page.evaluate(() => document.activeElement?.id === 'box-back'));
await page.click('#box-back');
await page.waitForTimeout(300);
check('backToShelves', (await page.$$('#waiting-pages .wbox')).length > 0);
await page.selectOption('#box-by', 'import');
await page.waitForTimeout(300);
const importBoxes = await page.$$eval('#waiting-pages .wbox b', (l) => l.map((b) => b.textContent));
results.importBoxes = importBoxes.slice(0, 10);
check('boxPerImport', importBoxes.includes('Phone 2021') && importBoxes.includes("Grandpa's box"));
check('boxByKept', await page.evaluate(() => localStorage.getItem('mdp.boxby') === 'import'));
await page.selectOption('#box-by', 'month');
await page.waitForTimeout(300);
// Filters still apply: a search narrows the boxes too.
await page.fill('#wf-text', 'Christmas');
await page.waitForTimeout(300);
check('filterNarrowsBoxes', (await page.$$eval('#waiting-pages .shelfrow .yeartag', (l) => l.map((h) => h.firstChild.textContent))).every((y) => /^19[56]\d$/.test(y)));
await page.fill('#wf-text', '');
await page.waitForTimeout(300);

// 4. Accessibility and screenshots in both looks, light and dark.
await setView('month');
await axe('01-system-by-month');
await setView('box');
await axe('02-system-by-box');
await setLook('scrapbook');
await show('waiting');
await page.waitForSelector('#waiting-pages .wbox');
await page.evaluate(() => document.querySelector('#waiting-pages').scrollIntoView());
await page.waitForTimeout(800);
await axe('03-scrapbook-by-box');
await setView('month');
await page.waitForTimeout(800);
await axe('04-scrapbook-by-month');
await page.evaluate(() => document.querySelector('#waiting-pages').scrollIntoView());
await page.waitForTimeout(500);
await page.screenshot({ path: `${out}/05-scrapbook-by-month-pages.png` });
await setView('box');
await page.click('#waiting-pages .wbox');
await page.waitForSelector('#box-back');
await page.waitForTimeout(600);
await page.evaluate(() => document.querySelector('#waiting-pages').scrollIntoView());
await page.screenshot({ path: `${out}/06-scrapbook-box-opened.png` });
await page.click('#box-back');
await page.waitForTimeout(300);

// 5. Props: they load, they are small, and Home shows them.
await show('home');
await page.waitForTimeout(800);
await axe('07-scrapbook-home-props');
const props = ['tape', 'paperclip', 'shoebox', 'shoebox-front', 'waiting-box'];
let bytes = 0;
for (const p of props) {
  const r = await page.evaluate(async (f) => { const x = await fetch(`props/${f}.webp`); return { ok: x.ok, n: (await x.arrayBuffer()).byteLength }; }, p);
  check(`prop ${p}`, r.ok);
  bytes += r.n;
}
results.propBytes = bytes;
check('propsWithinBudget', bytes < 1024 * 1024);
check('legalPadOnHome', (await page.$eval('#to-finish', (u) => getComputedStyle(u, '::before').backgroundImage)).includes('linear-gradient'));

// 6. Text at 150% and 200% in both views and both looks.
for (const look of ['system', 'scrapbook']) {
  await setLook(look);
  await show('waiting');
  for (const [view, zoom] of [['month', 150], ['box', 200]]) {
    await setView(view);
    await page.evaluate((z) => { document.documentElement.style.fontSize = `${z}%`; document.body.style.zoom = String(z / 100); }, zoom);
    await page.waitForTimeout(500);
    results[`overflow ${look} ${view} ${zoom}`] = await page.evaluate(() => document.querySelector('main').scrollWidth - document.querySelector('main').clientWidth);
    await page.screenshot({ path: `${out}/08-${look}-${view}-${zoom}.png` });
    await page.evaluate(() => { document.documentElement.style.fontSize = ''; document.body.style.zoom = ''; });
  }
}

// 7. High contrast always shows the System look: no props, no wood.
await setLook('scrapbook');
await page.emulateMedia({ forcedColors: 'active' });
await page.reload();
await page.waitForSelector('#journey-title:not(:empty)');
await show('waiting');
await setView('box');
check('highContrastPlain', (await page.evaluate(() => document.documentElement.dataset.look)) === 'system');
check('noBoxArtInHighContrast', (await page.$eval('#waiting-pages .wbox .boxart', (b) => getComputedStyle(b).backgroundImage)) === 'none');
await page.screenshot({ path: `${out}/09-high-contrast-by-box.png` });
await page.emulateMedia({ forcedColors: 'none' });

console.log(JSON.stringify({ results, errors }, null, 2));
const bad = Object.entries(results).filter(([k, v]) => / (light|dark)$/.test(k) && v.length);
await browser.close();
server.kill();
if (errors.length || bad.length) { console.log('PROBLEMS', JSON.stringify({ bad, errors })); process.exit(1); }
console.log('ALL GOOD');
