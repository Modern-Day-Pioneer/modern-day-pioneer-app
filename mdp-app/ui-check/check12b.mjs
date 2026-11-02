// Screen check for sprint 12, public test: the backup offer when a new
// version opens a Journey, Send feedback, and the newer-version button.
// Usage as check8.mjs: node check12.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8778'], { stdio: ['ignore', 'inherit', 'inherit'], env: { ...process.env, MDP_TEST_BEFORE: '0.11.60', MDP_TEST_FEEDBACK: 'feedback@example.org' } });
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
await page.goto('http://127.0.0.1:8778/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };

// 1. A new version opening the Journey offers a backup first.
await page.waitForSelector('dialog.ask[open]:has-text("A new version of the app")');
results.versionText = await page.textContent('dialog.ask[open]');
check('offersBackup', /version 0\.11\.60/.test(results.versionText) && /Make a backup/.test(results.versionText));
await axe('s12b-1-new-version');
await page.click('dialog.ask[open] button:text("Not now")');
await page.waitForSelector('dialog.ask[open]', { state: 'detached' });

// 2. Settings > About: Send feedback and Check for a newer version.
await page.click('button.navbtn[data-go="settings"]').catch(() => page.evaluate(() => go('settings')));
await page.waitForSelector('#send-feedback:not([hidden])');
check('feedbackShown', await page.isVisible('#send-feedback'));
await page.click('#check-update');
await page.waitForFunction(() => /newest version/.test(document.querySelector('#update-note').textContent));
check('updateChecked', true);
await axe('s12b-2-about');

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /(light|dark)$/.test(k) && v.length);
process.exit(errors.length || bad.length ? 1 : 0);
