// Screen check for sprint 12: Places found in the import preview. Google's
// locations grouped by place and named from the town list built in.
//
// Usage as check8.mjs: node check12e.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8781'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
async function axe(name) {
  for (const scheme of ['light', 'dark']) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.waitForTimeout(150);
    await page.evaluate(axeSrc);
    results[`${name} ${scheme}`] = await page.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'best-practice'] }))
      .violations.map((v) => `${v.id}: ${v.nodes.length} (${v.nodes.slice(0, 2).map((n) => n.target.join(' ')).join(' | ')})`));
  }
  await page.emulateMedia({ colorScheme: 'light' });
  await page.screenshot({ path: `${out}/${name}.png`, fullPage: true });
}
page.setDefaultTimeout(8000);
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8781/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

// A Takeout folder: two photos in Idaho Falls, 80 m apart, and one at sea.
const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check12e-'));
const album = path.join(src, 'Takeout', 'Google Photos', 'Trip');
fs.mkdirSync(album, { recursive: true });
[['a.gif', 43.4927, -112.0401], ['b.gif', 43.4930, -112.0410], ['c.gif', 0.0, -150.0]].forEach(([name, la, lo], n) => {
  fs.writeFileSync(path.join(album, name), Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, n, 41]));
  fs.writeFileSync(path.join(album, `${name}.supplemental-metadata.json`), JSON.stringify({ title: name, photoTakenTime: { timestamp: String(1562846400 + n * 86400) }, geoData: { latitude: la, longitude: lo } }));
});

// 1. Places found: the two Idaho Falls photos together, the town suggested.
await nav('waiting', '#h-waiting');
await page.click('#add-btn');
await page.click('#add-bulk');
await page.waitForSelector('#h-bulk');
await page.evaluate((f) => { window.nextOpen = f; }, path.join(src, 'Takeout'));
await page.click('#bulk-pick');
await page.waitForSelector('#bulk-places:not([hidden]) #bulk-places-todo li');
results.places = await page.locator('#bulk-places-todo li .t').allTextContents();
check('twoGroups', results.places.length === 2);
const falls = page.locator('#bulk-places-todo li', { hasText: 'Idaho Falls' });
check('townSuggested', /2 files · Nearest town/.test(await falls.textContent()));
check('rowNote', /near Idaho Falls, Idaho, United States/.test(await page.locator('#bulk-rows tr', { hasText: 'a.gif' }).locator('.note').textContent()));
await axe('s12e-1-places');

// 2. Add it as a new place (the name is filled in), leave the other out.
check('namePrefilled', (await falls.locator('input[aria-label^="Name for the new place"]').inputValue()) === 'Idaho Falls, Idaho, United States');
await falls.locator('input[aria-label^="Name for the new place"]').fill('Idaho Falls');
await falls.locator('button:has-text("Add as a new place")').click();
await page.waitForFunction(() => document.querySelectorAll('#bulk-places-todo li').length === 1);
await page.click('#bulk-places-todo li button:has-text("Leave out")');
await page.waitForFunction(() => document.querySelector('#bulk-places-todo').hidden);
check('placesFilled', (await page.inputValue('input[aria-label="Place for Google Photos/Trip/b.gif"]')) === 'Idaho Falls');
check('notesGone', (await page.locator('#bulk-rows .note').count()) === 0);
await axe('s12e-2-chosen');

// 3. Bring in: one new place with the location, on both photos.
await page.click('#bulk-go');
await page.waitForTimeout(2500);
const places = await call('browse', { kinds: ['place'] });
const ifalls = places.filter((p) => p.name === 'Idaho Falls');
check('onePlace', ifalls.length === 1);
const pv = await call('place_view', { id: ifalls[0].id });
results.coords = [pv.latitude, pv.longitude];
check('keepsLocation', Math.abs(pv.latitude - 43.4927) < 0.001);
results.placeMemories = (pv.memories || []).length;
check('bothPhotos', results.placeMemories === 2);

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
fs.rmSync(src, { recursive: true, force: true });
