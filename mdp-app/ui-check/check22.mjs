// Screen check for import progress: the main line shows the step and a count, never a file name;
// Show details lists the latest file names and errors, and stays open or closed as left.
// Usage: node check22.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8776'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
await page.goto('http://127.0.0.1:8776/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check21-'));
try {
  const fake = { running: true, done: 12, total: 340, message: 'Analyzing content…', recent: ['Beach day 1987.jpg', 'Grandpa fishing.jpg'], skipped: [{ item: 'C:\\Photos\\broken.jpg', reason: 'The file could not be read.' }] };
  await page.evaluate(() => localStorage.removeItem('mdp.jobDetails'));
  await page.evaluate(() => go('bulk'));
  await page.waitForSelector('#h-bulk');
  await page.evaluate((s) => { document.querySelector('#bulk-job').hidden = false; showJobProgress('bulk-job', s, 'Looking through the folder…'); }, fake);
  const line = await page.textContent('#bulk-job-msg');
  check('main line is the step and a count', line === 'Analyzing content: 12 of 340 files');
  check('details start closed', !(await page.$eval('#bulk-job-details', (d) => d.open)));
  check('no names while closed', (await page.$$('#bulk-job-files li')).length === 0);
  await page.click('#bulk-job-details summary');
  await page.evaluate((s) => showJobProgress('bulk-job', s, ''), fake);
  check('names listed in details', (await page.$$eval('#bulk-job-files li', (l) => l.map((x) => x.textContent))).join('|') === fake.recent.join('|'));
  check('errors listed in details', !(await page.$eval('#bulk-job-errors-h', (e) => e.hidden)) && (await page.textContent('#bulk-job-errors')).includes('broken.jpg'));
  check('open is remembered', await page.evaluate(() => localStorage.getItem('mdp.jobDetails')) === 'open');
  const box = await page.$('#bulk-job');
  await box.screenshot({ path: `${out}/22-bulk-progress.png` });
  await page.reload();
  await page.waitForSelector('#journey-title:not(:empty)');
  check('open state kept after reload', await page.$eval('#job-details', (d) => d.open));
  await page.evaluate(() => { document.querySelector('#job').hidden = false; showJobProgress('job', { running: true, done: 0, total: 0, message: '' }, 'Starting…'); });
  check('no count before the total is known', (await page.textContent('#job-msg')) === 'Starting…');
  check('no console errors', errors.length === 0);
  console.log(JSON.stringify({ results, errors }, null, 2));
} catch (e) { await fail(e); }
server.kill();
await browser.close();
