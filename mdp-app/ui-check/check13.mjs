// Screen check for sprint 13: an "In memory of" page started from a
// person, a note on why a memory matters, the front door setting, and the
// exported coffee table, In memory of page and search page (axe, light
// and dark, desktop and phone width).
// Usage as check8.mjs: node check13.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8783'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
async function axe(name, p = page) {
  for (const scheme of ['light', 'dark']) {
    await p.emulateMedia({ colorScheme: scheme });
    await p.waitForTimeout(150);
    await p.evaluate(axeSrc);
    results[`${name} ${scheme}`] = await p.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'best-practice'] }))
      .violations.map((v) => `${v.id}: ${v.nodes.length} (${v.nodes.slice(0, 2).map((n) => n.target.join(' ')).join(' | ')})`));
  }
  await p.emulateMedia({ colorScheme: 'light' });
  await p.screenshot({ path: `${out}/${name}.png`, fullPage: true });
}
page.setDefaultTimeout(8000);
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8783/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);

// A person with a memory that holds a picture.
const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check13-'));
const gif = path.join(src, 'garden.gif');
fs.writeFileSync(gif, Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, 13]));
const imported = await call('test_import', { paths: [gif] });
const person = await call('add_individual', { kind: 'Person', name: 'Ada Hale' });
const mem = await call('add_memory', { name: 'Ada in her garden', story: null });
const content = Array.isArray(imported) ? imported : (imported.imported || []);
results.imported = content;
await call('add_to_memory', { memory: mem, content });
await call('link_to_memory', { memory: mem, who: person, role: 'in the picture', roleNote: null, probably: false }).catch((e) => { results.linkNote = String(e); });

// Start the page from the person's screen.
await page.evaluate((id) => go('person', { id }), person);
await page.waitForSelector('#h-person');
await page.click('#person-body button:has-text("Start an In memory of page…")');
await page.waitForSelector('dialog[open]');
const title = await page.inputValue('dialog[open] input');
results.suggestedTitle = title;
check('suggestedTitle', /^(In memory of|Celebrating) Ada/.test(title));
await page.click('dialog[open] button:has-text("Add")');
await page.waitForSelector('#h-story');
check('tributeHeader', (await page.textContent('#story-body')).includes('In memory of page, about Ada Hale'));
const tribute = (await call('tributes_about', { about: person }))[0].id;
await call('add_memories_to_story', { story: tribute, memories: [mem] });
await page.evaluate((id) => go('story', { id }), tribute);
await page.waitForSelector('#story-body li:has-text("Ada in her garden")');
await page.click('#story-body button:has-text("Why it matters…")');
await page.waitForSelector('dialog[open] textarea');
await axe('s13-1-why');
await page.fill('dialog[open] textarea', 'She grew everything we ate that summer.');
await page.click('dialog[open] button:has-text("Save")');
await page.waitForSelector('#story-body small.why');
const v = await call('story_view', { id: tribute });
check('noteSaved', v.genre === 'tribute' && v.memories[0].note === 'She grew everything we ate that summer.');
await axe('s13-2-tribute');
await page.evaluate((id) => go('person', { id }), person);
await page.waitForSelector('#person-body button:has-text("Start another…")');
await axe('s13-3-person');

// The front door setting.
await page.evaluate(() => go('settings'));
await page.waitForSelector('#s-front');
check('frontDoorSetting', (await page.$$eval('#s-front option', (o) => o.map((x) => x.value))).join() === 'bookshelf,coffee table');
await axe('s13-4-settings');

// The exported pages: coffee table, In memory of, search; desktop and phone.
await call('start_export', { front_door: 'coffee table' });
const st = await call('export_status', {});
check('exported', !!st.report && !st.error);
const dest = st.folder;
const view = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
view.on('pageerror', (e) => errors.push(`export: ${e}`));
const front = `file://${dest}/Open me - Family History.html`;
const tpage = fs.readdirSync(path.join(dest, 'pages', 'tributes')).find((f) => f.endsWith('.html'));
for (const [w, tag] of [[1280, 'wide'], [390, 'phone']]) {
  await view.setViewportSize({ width: w, height: 900 });
  await view.goto(front);
  check(`table ${tag}`, (await view.$$('.coffee-table .mag')).length >= 1);
  results[`noSideScroll ${tag}`] = await view.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1);
  check(`noSideScroll ${tag}`, results[`noSideScroll ${tag}`]);
  await axe(`s13-5-table-${tag}`, view);
  await view.goto(`file://${dest}/pages/tributes/${tpage}`);
  check(`why ${tag}`, (await view.textContent('main')).includes('She grew everything'));
  await axe(`s13-6-tribute-${tag}`, view);
  await view.goto(`file://${dest}/pages/search.html?q=garden`);
  await view.waitForSelector('#results li');
  check(`search ${tag}`, (await view.textContent('#results')).includes('Ada in her garden'));
  await axe(`s13-7-search-${tag}`, view);
}
// Typing filters as you go; a result opens its page.
await view.fill('#q', 'ada hale');
await view.waitForFunction(() => document.querySelector('#results').textContent.includes('Ada Hale'));
await view.click('#results a:has-text("Ada Hale")');
await view.waitForSelector('h1:has-text("Ada Hale")');
check('resultOpens', true);
// Without scripts the search page still says where to look.
const noJs = await browser.newContext({ javaScriptEnabled: false });
const nj = await noJs.newPage();
await nj.goto(`file://${dest}/pages/search.html`);
check('noscriptNote', (await nj.textContent('body')).includes('Search needs JavaScript'));

const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /light|dark/.test(k) && v.length);
console.log(JSON.stringify({ results, errors, axeProblems: bad.length }, null, 2));
server.kill();
await browser.close();
fs.rmSync(src, { recursive: true, force: true });
