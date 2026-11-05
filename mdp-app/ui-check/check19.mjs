// Screen check for sprint 19 (combined with 18): a kind of event the family
// adds, offered again for every event, and the export's checksum switch.
// Usage as check8.mjs: node check19.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8775'], { stdio: ['ignore', 'inherit', 'inherit'], env: { ...process.env, MDP_FFMPEG: process.env.MDP_FFMPEG || 'ffmpeg' } });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
async function axe(name) {
  for (const scheme of ['light', 'dark']) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.waitForTimeout(150);
    await page.evaluate(axeSrc);
    results[`${name} ${scheme}`] = await page.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'best-practice'] }))
      .violations.map((v) => `${v.id}: ${v.nodes.length} (${v.nodes.slice(0, 2).map((n) => n.target.join(' ')).join(' | ')})`));
  }
  await page.emulateMedia({ colorScheme: 'light' });
  await page.screenshot({ path: `${out}/${name}.png` });
}
page.setDefaultTimeout(15000);
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8775/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

// 1. A person's page: add an event of a new kind.
const groups = await page.evaluate(() => window.__TAURI__.core.invoke('people'));
const all = groups.flatMap((g) => g.members);
const person = all.find((m) => m.name === 'June Rowan') || all[0];
await page.evaluate((id) => go('person', { id }), person.id);
await page.waitForSelector('#h-person');
const add = page.locator('form.addline[aria-label^="Add an event with"]');
await add.locator('select').selectOption('__new');
await add.getByLabel('New kind of event').fill('Mission');
await axe('s19-1-new-kind');
await add.locator('button[type=submit]').click();
await page.waitForSelector('#h-event');
const sel = page.locator('#event-body form.form').getByLabel('Kind of event', { exact: true });
check('missionChosen', (await sel.inputValue()) === 'Mission');
check('addedGroup', (await page.locator('optgroup[label="Added by your family"] option').allTextContents()).includes('Mission'));
await axe('s19-2-event-page');

// 2. The next event offers Mission too, and typing "mission" stays one kind.
await page.evaluate((id) => go('person', { id }), person.id);
await page.waitForSelector('#h-person');
await page.waitForFunction(() => [...document.querySelectorAll('form.addline select option')].some((o) => o.value === 'Mission'));
const kinds = await page.evaluate(() => window.__TAURI__.core.invoke('event_kinds'));
check('oneMission', kinds.filter((k) => k.added).length === 1);

// 3. Settings: the checksum switch, off by default.
await nav('settings', '#h-settings');
check('checksOff', !(await page.isChecked('#s-checks')));
await page.locator('#s-checks').scrollIntoViewIfNeeded();
await axe('s19-3-settings');

const bad = Object.entries(results).filter(([, v]) => Array.isArray(v) && v.length);
console.log(JSON.stringify({ results, errors }, null, 2));
await browser.close();
server.kill();
if (bad.length || errors.length) process.exit(1);
