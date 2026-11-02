// Screen check for sprint 10 (release 2, easier sorting and tidying):
// Waiting in suggested groups, proposed values on a memory, suggested
// relationships, merging two entries for one person, and a recording's
// quiet start skipped in the large view.
// Usage as check8.mjs: node check10.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8774'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 1100 } });
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
page.on('response', (r) => { if (r.status() >= 400 && !/\.(wav|gif)(#|$)/.test(r.url())) errors.push('HTTP ' + r.status() + ' ' + r.url()); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8774/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

// Files to bring in: two phone pictures of one day, three from a box, and a recording.
const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check10-'));
const gif = (name, n) => {
  const p = path.join(src, name);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, n]));
  return p;
};
const files = [gif('phone/IMG_20240612_100000.gif', 1), gif('phone/IMG_20240612_110000.gif', 2),
  gif("Grandpa's box/1952 Christmas.gif", 3), gif("Grandpa's box/1952 Christmas tree.gif", 4), gif("Grandpa's box/1952 Christmas lights.gif", 5)];
fs.copyFileSync(`${repo}/app/mdp-core/tests/fixtures/hello.wav`, path.join(src, 'hello.wav'));
files.push(path.join(src, 'hello.wav'));
const ids = await call('test_import', { paths: files });
check('imported', ids.length === 6);

// 1. Waiting in suggested groups: make one memory from a group in one step.
await nav('waiting', '#h-waiting');
await page.click('.tab[data-state="groups"]');
await page.waitForSelector('#group-list section.wgroup');
const cards = await page.$$eval('#group-list section.wgroup h2', (l) => l.map((x) => x.textContent));
results.groups = cards;
check('dayGroup', cards.includes('June 12, 2024'));
check('folderGroup', cards.includes("Grandpa's box"));
const box = page.locator('section.wgroup', { hasText: "Grandpa's box" });
check('folderDate', /Date: 1952 \(from the file name\)/.test(await box.textContent()));
await axe('s10-1-groups');
const day = page.locator('section.wgroup', { hasText: 'June 12, 2024' });
await day.getByLabel('Title of the memory').fill('Lake day');
await day.locator('button:has-text("Make a memory")').click();
await page.waitForSelector('dialog.ask[open]');
await page.click('dialog.ask[open] button:has-text("Back to Waiting")');
await page.waitForFunction(() => document.querySelectorAll('#group-list section.wgroup').length === 1);
const lake = (await call('memories')).find((m) => m.name === 'Lake day');
check('memoryMade', !!lake);
const lakeView = await call('memory_view', { id: lake.id });
check('memoryDated', lakeView.date?.edtf === '2024-06-12' && lakeView.items.length === 2);

// 2. A memory named by its file gets a title and a date to accept.
const xmas = (await call('raw_content', { kind: 'waiting' })).find((i) => i.name === '1952 Christmas.gif');
check('titleFromName', xmas.title === 'Christmas' && xmas.name_date === '1952');
const mid = await call('new_memory_with', { name: '1952 Christmas.gif', content: [xmas.id], story: null });
await page.evaluate((id) => go('memory', { id }), mid);
await page.waitForSelector('#h-memory');
await page.waitForSelector('#memory-body .proposals');
const props = await page.textContent('#memory-body .proposals');
results.proposals = props;
check('proposals', /Title: Christmas/.test(props) && /Date: 1952/.test(props));
await axe('s10-2-proposals');
await page.locator('#memory-body .proposals li', { hasText: 'Date:' }).locator('button').click();
await page.waitForFunction(() => !/Date:/.test(document.querySelector('#memory-body .proposals')?.textContent || ''));
check('dateAccepted', (await call('memory_view', { id: mid })).date?.edtf === '1952');

// 3. Suggested relationships on a person's page.
const people = (await call('people')).flatMap((g) => g.members);
let target = null;
for (const p of people) {
  const s = await call('suggested_relationships', { id: p.id }).catch(() => []);
  if (s.length) { target = { p, s }; break; }
}
if (!target) {
  // The sample may have none: give Ellen a grandparent through Walter.
  const by = (n) => people.find((p) => p.name === n)?.id;
  const ada = await call('add_individual', { kind: 'person', name: 'Ada Hale' });
  await call('add_relationship', { from: ada, kind: 'parent', to: by('Walter Rowan'), qualifier: null, certainty: null, sourceNote: null });
  target = { p: people.find((p) => p.name === 'Ellen Rowan'), s: await call('suggested_relationships', { id: by('Ellen Rowan') }) };
}
results.suggestedFor = target.p.name;
check('someSuggested', target.s.length > 0);
await page.evaluate((id) => go('person', { id }), target.p.id);
await page.waitForSelector('#h-person');
await page.waitForSelector('#person-body h3:has-text("Suggested from what you entered")');
await axe('s10-3-suggested');
const before = target.s.length;
await page.locator('#person-body button:has-text("Yes, add it")').first().click();
await page.waitForTimeout(500);
check('suggestionAccepted', (await call('suggested_relationships', { id: target.p.id })).length === before - 1);

// 4. Merge two entries for one person.
const walter = people.find((p) => p.name === 'Walter Rowan');
const walt = await call('add_individual', { kind: 'person', name: 'Walt Rowan' });
await page.evaluate((id) => go('person', { id }), walter.id);
await page.waitForSelector('#h-person');
const merge = page.getByRole('combobox', { name: 'Merge another person into this one' });
await merge.click();
await merge.fill('Walt Rowan');
await page.waitForTimeout(300);
await page.keyboard.press('ArrowDown');
await page.keyboard.press('Enter');
await page.waitForTimeout(600);
check('mergeAsked', /Merge Walt Rowan into Walter Rowan/.test(await page.evaluate(() => window.lastDialog || '')));
const merged = await call('individual_view', { id: walter.id });
check('merged', /Walt Rowan/.test(merged.other_names || ''));
check('mergedGone', !(await call('people')).flatMap((g) => g.members).some((p) => p.id === walt));
await axe('s10-4-merged');

// 5. A recording's quiet start is skipped in the large view, and can be played whole.
await call('test_quiet', { id: ids[5], seconds: 2.5 });
await nav('waiting', '#h-waiting');
await page.click('.tab[data-state="waiting"]');
await page.click('#waiting-list .link:has-text("hello")');
await page.waitForSelector('dialog.viewer[open] audio');
check('skipsQuiet', (await page.getAttribute('dialog.viewer audio', 'src')).endsWith('#t=2.5'));
await axe('s10-5-quiet');
await page.click('dialog.viewer button:has-text("Play the whole recording")');
await page.waitForFunction(() => !document.querySelector('dialog.viewer audio')?.getAttribute('src').includes('#t='));
check('playsWhole', (await call('show_content', { id: ids[5] })).start === 0);

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /(light|dark)$/.test(k) && v.length);
process.exit(errors.length || bad.length ? 1 : 0);
