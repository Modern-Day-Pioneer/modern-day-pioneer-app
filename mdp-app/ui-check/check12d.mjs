// Screen check for sprint 12: Names found in the import preview. Each name
// once; a choice counts for every file with it and its notes go away.
//
// Usage as check8.mjs: node check12d.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8780'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
await page.goto('http://127.0.0.1:8780/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

// A Takeout folder: Google tagged "Grandma Jo" in two photos and "Stranger" in one.
const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check12d-'));
const album = path.join(src, 'Takeout', 'Google Photos', 'Trip');
fs.mkdirSync(album, { recursive: true });
[['a.gif', ['Grandma Jo']], ['b.gif', ['Grandma Jo', 'Stranger']], ['c.gif', ['June']]].forEach(([name, people], n) => {
  fs.writeFileSync(path.join(album, name), Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, n, 31]));
  fs.writeFileSync(path.join(album, `${name}.supplemental-metadata.json`), JSON.stringify({ title: name, photoTakenTime: { timestamp: String(1562846400 + n * 86400) }, people: people.map((p) => ({ name: p })) }));
});

// 1. The names, once each, with what they need.
await nav('waiting', '#h-waiting');
await page.click('#add-btn');
await page.click('#add-bulk');
await page.waitForSelector('#h-bulk');
await page.evaluate((f) => { window.nextOpen = f; }, path.join(src, 'Takeout'));
await page.click('#bulk-pick');
await page.waitForSelector('#bulk-names:not([hidden]) #bulk-names-todo li');
const todo = async () => page.locator('#bulk-names-todo li b').allTextContents();
results.todo = await todo();
check('namesOnce', JSON.stringify(results.todo) === JSON.stringify(['Grandma Jo', 'Stranger']));
check('countsFiles', /2 files · Not in this Journey yet/.test(await page.textContent('#bulk-names-todo li[data-name="Grandma Jo"]')));
const noteFor = (f) => page.locator('#bulk-rows tr', { hasText: f }).locator('.note');
check('rowNote', /not in this Journey yet: Grandma Jo/.test(await noteFor('a.gif').textContent()));
await axe('s12d-1-names');

// 1b. A sure match (Google tagged "June": June Rowan, the one June) can still be changed.
const juneLine = page.locator('#bulk-names-done li[data-name="June Rowan"]');
check('juneKnown', /Is June Rowan, already in this Journey/.test(await juneLine.textContent()));
await page.click('#bulk-names-done-sum');
await juneLine.locator('button:has-text("Change")').click();
await page.waitForSelector('#bulk-names-todo li[data-name="June Rowan"] button:has-text("Leave out")');
check('knownChangeable', true);
await page.click('#bulk-names-todo li[data-name="June Rowan"] button:has-text("Keep June Rowan")');
await page.waitForSelector('#bulk-names-done li[data-name="June Rowan"]');

// 2. Grandma Jo is June Rowan: every file with the name, and its note goes.
const jo = page.locator('#bulk-names-todo li[data-name="Grandma Jo"]');
await jo.locator('input[role=combobox]').fill('June');
await page.locator('[role=option]', { hasText: 'June Rowan' }).first().click();
await page.waitForFunction(() => !document.querySelector('#bulk-names-todo li[data-name="Grandma Jo"]'));
check('joChosen', /Is June Rowan/.test(await page.textContent('#bulk-names-done li[data-name="Grandma Jo"]')));
check('keepTicked', await page.locator('#bulk-names-done li[data-name="Grandma Jo"] input[type=checkbox]').isChecked());
check('noteGoneA', (await noteFor('a.gif').count()) === 0);
results.noteB = await noteFor('b.gif').textContent();
check('noteB', !/Grandma Jo/.test(results.noteB) && /Stranger/.test(results.noteB));

// 3. Stranger left out; then every note is gone.
await page.click('#bulk-names-todo li[data-name="Stranger"] button:has-text("Leave out")');
await page.waitForFunction(() => document.querySelector('#bulk-names-todo').hidden);
check('allNotesGone', (await page.locator('#bulk-rows .note').count()) === 0);
await axe('s12d-2-chosen');

// 4. A name typed into People shows up in the list.
const ppl = page.locator('input[aria-label^="People for"]').last();
await ppl.fill('Uncle Ray');
await ppl.press('Tab');
await page.waitForSelector('#bulk-names-todo li[data-name="Uncle Ray"]');
check('typedNameListed', true);
await page.click('#bulk-names-todo li[data-name="Uncle Ray"] button:has-text("Add as a new person")');
await page.waitForFunction(() => document.querySelector('#bulk-names-todo').hidden);

// 5. Bring in: June is linked to both photos, Stranger nowhere, Uncle Ray added.
await page.click('#bulk-go');
await page.waitForFunction(async () => (await window.__TAURI__.core.invoke('import_status')).running === false, null, { timeout: 20000 }).catch(() => {});
await page.waitForTimeout(1500);
const mems = (await call('memory_list')).filter((m) => /^(a|b|c)$/i.test(m.name) || m.name.startsWith('Trip'));
results.memories = mems.map((m) => m.name);
const people = await call('browse', { kinds: ['person'] });
const june = people.find((p) => p.name === 'June Rowan');
const ray = people.find((p) => p.name === 'Uncle Ray');
check('rayAdded', !!ray);
check('noStranger', !people.some((p) => p.name === 'Stranger' || p.name === 'Grandma Jo'));
const juneMems = mems.filter((m) => m.people.includes(june.id)).length;
results.juneMems = juneMems;
results.memPeople = mems.map((m) => [m.name, m.people.map((id) => people.find((p) => p.id === id)?.name)]);
check('juneOnBoth', juneMems >= 2);
const jv = await call('individual_view', { id: june.id });
check('otherNameKept', /Grandma Jo/i.test(jv.other_names || ''));

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
fs.rmSync(src, { recursive: true, force: true });
