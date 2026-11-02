// Screen check for sprint 13, second part: memory previews (first words and
// people) and the Choose memories dialog with several ticked: on a story, a
// person, a place (with the warning when one moves), Waiting's existing
// memory, and covers in the search box. axe in light and dark.
// Usage as check8.mjs: node check13b.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8784'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
await page.goto('http://127.0.0.1:8784/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);


const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check13b-'));
const gif = (n) => { const p = path.join(src, `porch ${n}.gif`); fs.writeFileSync(p, Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, n, 7])); return p; };
const txt = path.join(src, 'letter.txt');
fs.writeFileSync(txt, 'Dear June, the apple tree finally bloomed this spring and the whole street came to see it.');
const [g1, g2, g3, t1] = await call('test_import', { paths: [gif(1), gif(2), gif(3), txt] });
const ada = await call('add_individual', { kind: 'Person', name: 'Ada Hale' });
const mk = async (name, content) => { const m = await call('add_memory', { name, story: null }); await call('add_to_memory', { memory: m, content }); return m; };
const porch = await mk('On the porch', [g1]);
const garden = await mk('In the garden', [g2]);
const letter = await mk('A letter about the tree', [t1]);
await call('link_to_memory', { memory: garden, who: ada, role: 'in the picture', roleNote: null, probably: false });
const ogden = await call('add_place', { name: 'Ogden' });
const boise = await call('add_place', { name: 'Boise' });
await call('update_memory', { id: porch, changes: { place_id: boise } });

// Previews in the list of memories: first words of a text, and who is in it.
const rows = await call('memory_list', {});
check('wordsFromText', rows.find((m) => m.id === letter).words.startsWith('Dear June, the apple tree'));
check('whoNames', rows.find((m) => m.id === garden).who === 'Ada Hale');

// A story: add two at once from the dialog.
const story = await call('add_story', { name: 'Summers' });
await page.evaluate((id) => go('story', { id }), story);
await page.click('#story-body button:has-text("Add memories already in the Journey…")');
await page.waitForSelector('dialog[open] ul.pick.memories');
check('previewInDialog', (await page.textContent('dialog[open] ul.pick')).includes('Dear June'));
await page.fill('dialog[open] input[type=search], dialog[open] .field input', 'tree');
check('filter', await page.locator('dialog[open] li', { hasText: 'On the porch' }).isHidden());
await page.fill('dialog[open] .field input', '');
await page.locator('dialog[open] li', { hasText: 'On the porch' }).locator('input').check();
await page.locator('dialog[open] li', { hasText: 'A letter about the tree' }).locator('input').check();
await axe('s13b-1-choose');
await page.click('dialog[open] button:has-text("Add")');
await page.waitForSelector('#story-body li:has-text("A letter about the tree")');
check('twoAdded', (await call('story_view', { id: story })).memories.length === 2);
check('wordsOnStory', (await page.textContent('#story-body')).includes('Dear June'));
await axe('s13b-2-story');

// A person: link two memories with one role.
await page.evaluate((id) => go('person', { id }), ada);
await page.click('#person-body button:has-text("Link memories already in the Journey…")');
await page.waitForSelector('dialog[open] ul.pick.memories');
check('alreadyLinkedLeftOut', await page.locator('dialog[open] li', { hasText: 'In the garden' }).count() === 0);
await page.locator('dialog[open] li', { hasText: 'On the porch' }).locator('input').check();
await page.locator('dialog[open] li', { hasText: 'A letter about the tree' }).locator('input').check();
await page.click('dialog[open] button:has-text("Next")');
await page.waitForSelector('dialog[open] select');
await axe('s13b-3-role');
await page.click('dialog[open] button:has-text("Link")');
await page.waitForSelector('#person-body li:has-text("A letter about the tree")');
const linked = (await call('memory_list', {})).filter((m) => m.people.includes(ada)).length;
check('threeLinked', linked === 3);

// A place: add two, one of which moves from Boise after the warning.
await page.evaluate((id) => go('place', { id }), ogden);
await page.click('#place-body button:has-text("Add memories already in the Journey…")');
await page.waitForSelector('dialog[open] ul.pick.memories');
await page.locator('dialog[open] li', { hasText: 'On the porch' }).locator('input').check();
await page.locator('dialog[open] li', { hasText: 'In the garden' }).locator('input').check();
await page.click('dialog[open] button:has-text("Add")');
await page.waitForSelector('#place-body li:has-text("On the porch")');
check('warned', (await page.evaluate(() => window.lastDialog || '')).includes('On the porch already has another place'));
const at = (await call('memory_list', {})).filter((m) => m.place_id === ogden).length;
check('twoAtOgden', at === 2);
await axe('s13b-4-place');

// Waiting: put an item into a memory already in the Journey, chosen from tiles.
await page.evaluate((id) => { window.__made = putIntoMemory([id], 'porch 3.gif'); }, g3);
await page.waitForSelector('dialog[open]');
await page.click('dialog[open] label:has-text("A memory already in the Journey")');
await page.locator('dialog[open] li', { hasText: 'In the garden' }).locator('input').check();
await axe('s13b-5-waiting');
await page.click('dialog[open] button:has-text("Put it in")');
await page.waitForFunction(() => !document.querySelector('dialog[open] ul.pick'));
await page.keyboard.press('Escape').catch(() => {});
const mv = await call('memory_view', { id: garden });
check('putIn', mv.items.length === 2);

// The search box: a memory shows its cover box and date.
await page.evaluate(() => { document.querySelectorAll('dialog[open]').forEach((d) => d.close()); go('home'); });
await page.fill('#q', 'porch');
await page.waitForSelector('#results [role=option] .thumb');
await axe('s13b-6-search');
check('searchCover', true);

const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /light|dark/.test(k) && v.length);
console.log(JSON.stringify({ results, errors, axeProblems: bad.length }, null, 2));
server.kill();
await browser.close();
fs.rmSync(src, { recursive: true, force: true });
