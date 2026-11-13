// Screen check for import rules in the Bring in a folder preview: a folder's
// own _import-rules.md fills in memory, story, family, series and who took
// the pictures; the coverage report lists what no rule caught; the rules
// can be switched to none or the built-in set; the report and the built-in
// rules can be saved. Every name here is made up.
// Usage: node check23.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8777'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const results = {};
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const page = await browser.newPage({ viewport: { width: 1280, height: 1100 } });
page.setDefaultTimeout(8000);
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8777/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check23-'));
let n = 0;
const gif = (name) => {
  const p = path.join(src, name);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, ++n, 23]));
};
const builtIn = fs.readFileSync(`${repo}/app/mdp-core/data/import-rules/family-history.md`, 'utf8');
fs.writeFileSync(path.join(src, '_import-rules.md'), builtIn.split('\n# Names')[0] + '\n# Names\n- Rosa\n- Ellen\n\n# Settings\nday 01 means month only: yes\n');
gif('Walter and June/2015 Family/Tolver (2).gif');
gif('Walter and June/2015 Family/Tolver (1).gif');
gif('Walter and June/2018 Alaska/Rosa/IMG_0001.gif');
gif('Walter and June/1961-1970 (School)/19650301-Ellen and Walter Rowan-Rowan home in Provo, UT.gif');
gif('Walter and June/2010-2019 Kimball/IMG_0041.gif');

// 1. The folder's own rules fill the preview in, and the coverage report shows.
await nav('waiting', '#h-waiting');
await page.evaluate((f) => { window.nextOpen = f; }, src);
await page.click('#add-btn');
await page.click('#add-folder');
await page.waitForSelector('#bulk-plan:not([hidden]) #bulk-rows tr');
check('fiveRows', (await page.locator('#bulk-rows tr').count()) === 5);
const rows = await page.textContent('#bulk-rows');
check('familyShown', /Family: Walter and June family/.test(rows));
check('seriesShown', /Number 2/.test(rows));
check('photoByShown', /Pictures from Rosa/.test(rows));
check('rulesShown', /Rules 1, 7, 17/.test(rows));
const memories = await page.$$eval('#bulk-rows input[aria-label^="Memory for"]', (els) => els.map((e) => e.value));
check('memories', memories.includes('2015 Family') && memories.includes('2018 Alaska') && memories.includes('Ellen and Walter Rowan'));
check('coverageShown', await page.isVisible('#bulk-coverage'));
check('coverageSum', /4 files given a memory, 1 file no rule caught/.test(await page.textContent('#bulk-coverage-sum')));
await page.click('#bulk-coverage-sum');
check('uncaughtListed', /IMG_0041/.test(await page.textContent('#bulk-coverage-uncaught')));
check('namesToConfirm', /Tolver \(2 files\)/.test(await page.textContent('#bulk-coverage-names')));
check('storySwitchHidden', !(await page.isVisible('label[for="bulk-story"]')));
await page.screenshot({ path: `${out}/c23-1-rules-preview.png`, fullPage: true });
await page.locator('.bulk-wrap').screenshot({ path: `${out}/c23-1b-rows.png` });

// 2. The report saves as text.
const report = path.join(src, 'coverage.txt');
await page.evaluate((f) => { window.nextSave = f; }, report);
await page.click('#bulk-coverage-save');
await page.waitForTimeout(500);
check('reportSaved', /FILES NO RULE CAUGHT \(1\)/.test(fs.readFileSync(report, 'utf8')));

// 3. No rules: the preview is as before.
await page.selectOption('#bulk-rules', 'none');
await page.waitForFunction(() => document.querySelector('#bulk-coverage').hidden);
check('noRulesNoFamily', !/Family:/.test(await page.textContent('#bulk-rows')));

// 4. The built-in set, and saving it as a file to change.
await page.selectOption('#bulk-rules', 'built in');
await page.waitForFunction(() => !document.querySelector('#bulk-coverage').hidden);
check('builtInSource', /built into the app/.test(await page.textContent('#bulk-coverage-from')));
const copy = path.join(src, 'copy.md');
await page.evaluate((f) => { window.nextSave = f; }, copy);
await page.click('#bulk-rules-copy');
await page.waitForTimeout(500);
check('builtInSaved', fs.readFileSync(copy, 'utf8').startsWith('# Import rules: Family history folders'));
await page.screenshot({ path: `${out}/c23-2-built-in.png`, fullPage: true });

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
process.exit(errors.length ? 1 : 0);
