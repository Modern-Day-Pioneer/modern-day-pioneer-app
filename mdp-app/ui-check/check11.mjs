// Screen check for sprint 11 (release 2, bulk folder import): a folder
// looked through with its guesses shown, grouped and saved as an import
// plan, brought in as memories, a blank sample plan, and an exported
// history brought back in.
// Usage as check8.mjs: node check11.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8775'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
await page.goto('http://127.0.0.1:8775/');

await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

// A sorted folder: dates and people in the names.
const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check11-'));
const gif = (name, n) => {
  const p = path.join(src, name);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, n, 11]));
  return p;
};
const folder = path.join(src, 'Family photos');
gif('Family photos/1970s/June Rowan in the garden.gif', 1);
gif('Family photos/1970s/Walter and June.gif', 2);
gif('Family photos/1980-1985 Lake trips/IMG_0001.gif', 3);
gif('Family photos/1980-1985 Lake trips/IMG_0002.gif', 4);
fs.writeFileSync(path.join(folder, '1980-1985 Lake trips', 'IMG_0001.txt'), 'Ellen on the dock.');

// 1. The preview of a folder.
await nav('waiting', '#h-waiting');
await page.click('#add-btn');
await page.click('#add-bulk');
await page.waitForSelector('#h-bulk');
await axe('s11-1-start');
await page.evaluate((f) => { window.nextOpen = f; }, folder);
await page.click('#bulk-pick');
await page.waitForSelector('#bulk-plan:not([hidden]) #bulk-rows tr');
const rows = await page.locator('#bulk-rows tr').count();
check('fourRows', rows === 4);
check('juneSuggested', (await page.inputValue('input[aria-label="People for 1970s/June Rowan in the garden.gif"]')) === 'June Rowan');
check('folderDate', (await page.inputValue('input[aria-label="Date for 1970s/Walter and June.gif"]')) === '1970s');
check('rangeDate', (await page.inputValue('input[aria-label="Date for 1980-1985 Lake trips/IMG_0001.gif"]')) === '1980 to 1985');
check('dateSource', /from the folder/.test(await page.textContent('#bulk-rows tr:nth-child(3)')));
check('counted', /4 files ticked, making 4 memories/.test(await page.textContent('#bulk-count')));
await axe('s11-2-preview');

// 2. Suggested groups, a fix, and the plan saved for a script or spreadsheet.
await page.click('#bulk-groups');
check('grouped', /making 3 memories/.test(await page.textContent('#bulk-count')));
await page.fill('input[aria-label="Title for 1970s/Walter and June.gif"]', 'Walter and June in the 1970s');
await page.uncheck('input[aria-label="Bring in 1970s/June Rowan in the garden.gif"]');
check('unticked', /3 files ticked, making 2 memories/.test(await page.textContent('#bulk-count')));
const planFile = path.join(src, 'import-plan.csv');
await page.evaluate((f) => { window.nextSave = f; }, planFile);
await page.click('#bulk-save');
await page.waitForTimeout(400);
const csv = fs.readFileSync(planFile, 'utf8');
check('planSaved', csv.includes('Walter and June in the 1970s') && csv.includes(',no,') && csv.includes('Lake trips'));
await axe('s11-3-grouped');

// 3. Bring them in: memories made in one step.
const before = (await call('memory_list')).length;
await page.click('#bulk-go');
await page.waitForSelector('#job-done:not([hidden])');
check('importDone', /Added 3 files in 2 new memories/.test(await page.textContent('#job-done-msg')));
const after = await call('memory_list');
check('memoriesMade', after.length === before + 2);
check('lakeTrip', after.some((m) => m.name === 'Lake trips'));
await axe('s11-4-done');

// 4. The plan opened again: the files are in, so nothing is added twice.
await page.click('#job-dismiss');
await page.click('#add-btn');
await page.click('#add-bulk');
await page.evaluate((f) => { window.nextOpen = f; }, planFile);
await page.click('#bulk-open');
await page.waitForSelector('#bulk-plan:not([hidden]) #bulk-rows tr');
check('planOpened', /From the import plan/.test(await page.textContent('#bulk-from')) && (await page.locator('#bulk-rows tr').count()) === 4);
check('planKeepsUntick', !(await page.isChecked('input[aria-label="Bring in Family photos/1970s/June Rowan in the garden.gif"]')));

// 5. A blank sample plan with its read-me.
const sample = path.join(src, 'sample', 'import-plan.csv');
fs.mkdirSync(path.dirname(sample));
await page.evaluate((f) => { window.nextSave = f; }, sample);
await page.click('#bulk-sample');
await page.waitForTimeout(400);
check('sampleSaved', fs.existsSync(sample) && fs.readdirSync(path.dirname(sample)).some((f) => /read-me/.test(f)));

// 6. An exported history brought back in, choosing what comes in. Two
// stories are removed after the export, so the export holds them and the Journey doesn't.
await call('start_export', {});
const exp = (await call('export_status', {})).folder;
const stories = await call('stories');
check('twoStories', stories.length >= 2);
for (const st of stories.slice(0, 2)) await call('remove_item', { id: st.id });
await page.evaluate((f) => { window.nextOpen = f; }, exp);
await page.click('#bulk-pick');
await page.waitForSelector('#bulk-export:not([hidden])');
check('exportSeen', /exported by the app/.test(await page.textContent('#bulk-ex-sum')) && /already in this Journey/.test(await page.textContent('#bulk-ex-sum')));
check('storiesOffered', /Stories: 2 of 2 ticked/.test(await page.textContent('#bulk-ex-groups')));
await page.uncheck(`#bulk-ex-groups label:has-text("${stories[0].name}") input`);
check('oneLeftOut', /leaving out 1 item/.test(await page.textContent('#bulk-ex-go')));
await axe('s11-5-export');
const people = (await call('people')).flatMap((g) => g.members).length;
await page.click('#bulk-ex-go');
await page.waitForSelector('#job-done:not([hidden])');
check('exportBroughtIn', /already in your Journey/.test(await page.textContent('#job-done-msg')));
const back = (await call('stories')).map((st) => st.name);
check('chosenStoryBack', back.includes(stories[1].name) && !back.includes(stories[0].name));
check('noDoubles', (await call('people')).flatMap((g) => g.members).length === people);
await axe('s11-6-export-done');

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /(light|dark)$/.test(k) && v.length);
process.exit(errors.length || bad.length ? 1 : 0);
