// Screen check for sprint 9 (Android): the phone layout at a phone's size
// with an Android browser name, so the screens take their phone form: search
// across the top, the main places along the bottom, the photo picker in the
// Add menu, and nothing the phone can't do (folders, File Explorer, the
// browser). Accessibility is checked with axe in light and dark.
// Usage as check8.mjs: node check9.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8773'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const ua = 'Mozilla/5.0 (Linux; Android 16; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Mobile Safari/537.36';
const context = await browser.newContext({ viewport: { width: 393, height: 852 }, userAgent: ua, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });
const page = await context.newPage();
async function axe(name) {
  for (const scheme of ['light', 'dark']) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.waitForTimeout(150);
    await page.evaluate(axeSrc);
    results[`${name} ${scheme}`] = await page.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'best-practice'] }))
      .violations.map((v) => `${v.id}: ${v.nodes.length} (${v.nodes.slice(0, 2).map((n) => n.target.join(' ')).join(' | ')})`));
  }
  await page.emulateMedia({ colorScheme: 'light' });
  await page.screenshot({ path: `${out}/phone-${name}.png` });
}
page.setDefaultTimeout(8000);
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/phone-fail.png` }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8773/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const box = (s) => page.locator(s).first().boundingBox();
const shown = (s) => page.locator(s).first().isVisible();

check('phone class set', await page.evaluate(() => document.documentElement.classList.contains('phone')));
const nav = await box('nav.side');
check('main places along the bottom', nav.y + nav.height >= 850 && nav.width >= 390 && nav.height < 90);
const search = await box('#q');
check('search across the top', search.y < 60 && search.width > 300);
const btns = await page.locator('nav.side .navbtn').all();
const tops = await Promise.all(btns.map(async (b) => (await b.boundingBox()).y));
check('nav buttons in one row', Math.max(...tops) - Math.min(...tops) < 2);
check('nav buttons big enough to tap', (await Promise.all(btns.map(async (b) => (await b.boundingBox()).height))).every((h) => h >= 44));
check('no sideways scrolling on Home', await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
const tiles = await Promise.all(['#home-record', '#home-add', '#home-story', '#home-people'].map(box));
check('Home actions are a 2 x 2 of tiles, recording first', tiles[0].y === tiles[1].y && tiles[2].y === tiles[3].y && tiles[0].x < tiles[1].x && tiles[2].y > tiles[0].y);
check('no Preview card on the phone Home (Preview is along the bottom)', !(await shown('.previewcard')));
await axe('home');
// Idea 55: Finished beside a memory that could use more takes it off the list.
await page.evaluate(async () => {
  const inv = window.__TAURI__.core.invoke;
  const m = await inv('add_memory', { name: 'The lake house', story: null });
  await inv('add_note', { memory: m, title: 'The lake', text: 'Hot summers at the lake.', fromCaption: false });
});
await page.click('nav.side [data-go="stories"]');
await page.click('nav.side [data-go="home"]');
await page.waitForSelector('#to-finish button.btn');
const fin = page.locator('#to-finish button.btn').first();
if (await fin.count()) {
  const before = await page.locator('#to-finish li').count();
  await fin.click();
  await page.waitForFunction((n) => document.querySelectorAll('#to-finish li').length < n, before);
  check('Finished takes a memory off Memories to finish', true);
  await page.click('#toast button');
  await page.waitForFunction((n) => document.querySelectorAll('#to-finish li').length === n, before);
  check('Undo puts it back', true);
} else check('a memory that could use more on Home', false);

await page.click('nav.side [data-go="waiting"]');
await page.waitForSelector('[data-pane="waiting"]:not([hidden])');
await page.click('#add-btn');
check('photo picker in the Add menu', await shown('#add-media'));
check('no folder picking on the phone', !(await shown('#add-folder')));
check('no drag-and-drop hint on the phone', !(await shown('.add-row .hint')));
await axe('waiting-add');
await page.click('#add-btn');
check('no buttons on each waiting row on the phone', await page.evaluate(() => document.querySelectorAll('#waiting-list li .btn, [data-pane="waiting"] li .btn').length === 0));
check('Select all in the selection bar', await page.evaluate(() => !!document.querySelector('#sel-bar #sel-all')));
// A long list (the sample has one waiting item), scrolled to its end.
await page.evaluate(() => { const li = document.querySelector('[data-pane="waiting"] li'); for (let i = 0; i < 30; i++) li.after(li.cloneNode(true)); });
await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
await page.waitForTimeout(200);
const bar = await box('#sel-bar');
const top = await box('nav.side .search');
check('selection bar stays in view below the search while scrolling', bar.y > top.y + top.height - 4 && bar.y < top.y + top.height + 12);
await page.screenshot({ path: `${out}/phone-waiting-scrolled.png` });
await page.evaluate(() => window.scrollTo(0, 0));

await page.click('nav.side [data-go="people"]');
await page.waitForSelector('[data-pane="people"]:not([hidden])');
check('no sideways scrolling on People', await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
await axe('people');

await page.click('nav.side [data-go="preview"]');
await page.waitForSelector('[data-pane="preview"]:not([hidden])');
check('no "open in my browser" on the phone', !(await shown('#pv-browser')));
check('no "show the folder" on the phone', !(await shown('#ex-show')));
await axe('preview');

await page.click('nav.side [data-go="settings"]');
await page.waitForSelector('[data-pane="settings"]:not([hidden])');
check('no File Explorer button on the phone', !(await shown('#show-folder')));
check('export copy folder can be chosen on the phone', await shown('#s-export-choose'));
check('no typing in folder fields', await page.evaluate(() => ['#s-folder', '#s-export'].every((s) => document.querySelector(s).readOnly)));
check('no opening other Journeys by folder on the phone', !(await shown('#s-open')));
check('no log folder button on the phone', !(await shown('#open-logs')));
check('no sideways scrolling on Settings', await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
await axe('settings');

// The phone's Back button: back a screen, and closes an open dialog first.
await page.click('nav.side [data-go="people"]');
await page.waitForSelector('[data-pane="people"]:not([hidden])');
await page.goBack();
await page.waitForSelector('[data-pane="settings"]:not([hidden])');
check('Back returns to the screen before', true);
await page.click('nav.side [data-go="home"]');
await page.waitForSelector('[data-pane="home"]:not([hidden])');
await page.click('#home-people');
await page.waitForSelector('dialog[open]');
const behind = await page.evaluate(() => document.querySelector('.pane:not([hidden])').dataset.pane);
await page.goBack();
await page.waitForFunction(() => !document.querySelector('dialog[open]'));
check('Back closes the dialog and stays on the screen', await page.evaluate(() => document.querySelector('.pane:not([hidden])').dataset.pane) === behind);

// Every axe run must be clean.
const bad = Object.entries(results).filter(([, v]) => Array.isArray(v) && v.length);
console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
if (bad.length || errors.length) { console.log('FAILED: axe or console errors'); process.exit(1); }
console.log('ALL PASSED');
