// Screen check for sending folders to the preview: Add > Choose a folder and a dropped folder open
// the Bring in a folder preview; dropped files still go straight in; with
// several folders dropped, an exported history is left out with a note.
// Usage: node check21.mjs <folder for screenshots>
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
await page.goto('http://127.0.0.1:8775/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check21-'));
let n = 0;
const gif = (name) => {
  const p = path.join(src, name);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, ++n, 20]));
  return p;
};
gif('Lake/1975 dock.gif'); gif('Lake/1976 boat.gif');
gif('Garden/1980 roses.gif');
gif('Barn/1990 barn.gif');
const loose = gif('loose.gif');
const waitDone = async () => { await page.waitForSelector('#job-done:not([hidden])'); const t = await page.textContent('#job-done-msg'); await page.click('#job-dismiss'); return t; };

// 1. Add > Choose a folder opens the preview with that folder looked through.
await nav('waiting', '#h-waiting');
check('menuLabel', /with a preview/.test(await page.textContent('#add-folder')));
await page.evaluate((f) => { window.nextOpen = f; }, path.join(src, 'Lake'));
await page.click('#add-btn');
await page.click('#add-folder');
await page.waitForSelector('#h-bulk');
await page.waitForSelector('#bulk-plan:not([hidden]) #bulk-rows tr');
check('menuPreview', (await page.locator('#bulk-rows tr').count()) === 2);
await page.screenshot({ path: `${out}/c21-1-menu-folder.png`, fullPage: true });

// 2. A file and a folder dropped: the file goes straight in, the folder to the preview.
await page.evaluate((f) => { window.nextOpen = null; }, null);
await nav('waiting', '#h-waiting');
await page.evaluate((p) => window.drop(p), [loose, path.join(src, 'Garden')]);
await page.waitForSelector('#h-bulk');
await page.waitForFunction(() => /Added 1 item/.test(document.querySelector('#job-done-msg').textContent));
check('fileStraightIn', true);
check('fileNote', /straight into Waiting/.test(await page.textContent('#toast')));
await page.waitForSelector('#bulk-plan:not([hidden]) #bulk-rows tr');
check('dropPreview', (await page.locator('#bulk-rows tr').count()) === 1 && /roses/.test(await page.textContent('#bulk-rows')));
await page.screenshot({ path: `${out}/c21-2-drop-folder.png`, fullPage: true });

// 3. Two folders and an exported history dropped together: the folders go
// straight in as before, the export is left out with a note.
await call('start_export', {});
const exp = (await call('export_status', {})).folder;
await nav('waiting', '#h-waiting');
await page.evaluate((p) => window.drop(p), [path.join(src, 'Barn'), path.join(src, 'Lake'), exp]);
await page.waitForFunction(() => /history this app exported/.test(document.querySelector('#toast').textContent));
check('exportNote', true);
await page.screenshot({ path: `${out}/c21-3-export-left-out.png`, fullPage: true });
check('foldersStraightIn', /Added/.test(await waitDone()));
check('stayedOnWaiting', await page.isVisible('#h-waiting'));

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
process.exit(errors.length ? 1 : 0);
