// Screen check for sprint 18: previews in Bring in a folder, read
// from the originals before anything is copied in. Pictures get a small
// picture as their row comes into view, recordings and videos play, and
// text shows its first lines. Needs ffmpeg on the PATH (to make the test files).
// Usage as check8.mjs: node check18.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn, execFileSync } from 'child_process';
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
const asked = [];
page.on('request', (r) => { const m = r.url().match(/\/invoke\/(preview_\w+)/); if (m) asked.push(m[1]); });
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8775/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

// A box of files: real pictures (a test pattern in several colors), a
// recording, a short video and a letter.
const ff = process.env.MDP_FFMPEG || 'ffmpeg';
const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check18-'));
const folder = path.join(src, 'Grandma box');
fs.mkdirSync(path.join(folder, 'Lake 1972'), { recursive: true });
const make = (args, file) => execFileSync(ff, ['-hide_banner', '-loglevel', 'error', '-y', ...args, file]);
const hues = ['red', 'orange', 'gold', 'green', 'teal', 'blue', 'purple', 'brown'];
for (let i = 0; i < 40; i++) {
  make(['-f', 'lavfi', '-i', `color=c=${hues[i % hues.length]}:s=640x480,drawbox=x=${40 + i * 10}:y=60:w=200:h=160:color=white@0.8:t=fill`, '-frames:v', '1'], path.join(folder, 'Lake 1972', `IMG_${String(1000 + i)}.jpg`));
}
make(['-f', 'lavfi', '-i', 'sine=frequency=440:duration=2'], path.join(folder, 'Grandpa telling the story.wav'));
make(['-f', 'lavfi', '-i', 'testsrc=size=320x240:rate=10:duration=1', '-pix_fmt', 'yuv420p'], path.join(folder, 'Birthday 1990.mp4'));
fs.writeFileSync(path.join(folder, 'Letter from June.txt'), 'Dear Ellen,\n\nWe got to the lake on Saturday.\nThe water was cold but the kids swam anyway.\nLove, June\n');
const before = Object.fromEntries(fs.readdirSync(folder).map((n) => [n, fs.statSync(path.join(folder, n)).mtimeMs]));

await nav('waiting', '#h-waiting');
await page.click('#add-btn');
await page.click('#add-bulk');
await page.waitForSelector('#h-bulk');
await page.evaluate((f) => { window.nextOpen = f; }, folder);
await page.click('#bulk-pick');
await page.waitForSelector('#bulk-plan:not([hidden]) #bulk-rows tr');
check('rows', (await page.locator('#bulk-rows tr').count()) === 43);

// 1. Rows in view get their small pictures; rows further down wait.
await page.locator('#bulk-rows tr').first().scrollIntoViewIfNeeded();
await page.waitForSelector('#bulk-rows .pv-pic img');
await page.waitForTimeout(1500);
const early = asked.filter((c) => c === 'preview_thumb').length;
check('lazy', early > 0 && early < 41);
await axe('s18-1-previews');

// 2. Scrolling down brings the rest in.
for (let i = 0; i < 12; i++) { await page.mouse.wheel(0, 300); await page.waitForTimeout(250); }
await page.locator('#bulk-rows tr').last().scrollIntoViewIfNeeded();
await page.waitForFunction(() => document.querySelectorAll('#bulk-rows .pv-pic img').length >= 30, null, { timeout: 30000 });
await page.waitForSelector('#bulk-rows .pv-lines:not([hidden])');
check('letterLines', /Dear Ellen,\s*We got to the lake on Saturday\.\s*The water was cold/.test(await page.textContent('#bulk-rows .pv-lines')));
check('moreLater', asked.filter((c) => c === 'preview_thumb').length > early);
await page.screenshot({ path: `${out}/s18-2-scrolled.png` });

// 3. A recording plays from the original, read only.
await page.click('button[aria-label="Play Grandpa telling the story.wav"]');
await page.waitForSelector('#preview-dialog[open] audio');
check('audioFromOriginal', decodeURIComponent(await page.getAttribute('#preview-body audio', 'src')).endsWith('Grandpa telling the story.wav'));
await axe('s18-3-play-recording');
await page.click('#preview-dialog button[type=submit]');
check('stopped', (await page.locator('#preview-body audio').count()) === 0);

// 4. A video has a picture and plays too.
await page.waitForSelector('button[aria-label="Play Birthday 1990.mp4"] img');
await page.click('button[aria-label="Play Birthday 1990.mp4"]');
await page.waitForSelector('#preview-dialog[open] video');
await page.screenshot({ path: `${out}/s18-4-play-video.png` });
await page.click('#preview-dialog button[type=submit]');

// 5. A picture can be looked at closer.
await page.locator('button[aria-label="Look at Lake 1972/IMG_1000.jpg"]').scrollIntoViewIfNeeded();
await page.click('button[aria-label="Look at Lake 1972/IMG_1000.jpg"]');
await page.waitForSelector('#preview-dialog[open] img');
await axe('s18-5-look-closer');
await page.click('#preview-dialog button[type=submit]');

// 6. A row ticked off keeps its preview; redrawing doesn't ask again.
const n = asked.length;
await page.click('#bulk-each');
await page.waitForTimeout(500);
check('noAskAgain', asked.length === n);

// The originals are as they were, and nothing was added beside them.
const after = Object.fromEntries(fs.readdirSync(folder).map((n) => [n, fs.statSync(path.join(folder, n)).mtimeMs]));
check('originalsUntouched', JSON.stringify(before) === JSON.stringify(after));

const bad = Object.entries(results).filter(([, v]) => Array.isArray(v) && v.length);
console.log(JSON.stringify({ results, errors }, null, 2));
await browser.close();
server.kill();
if (bad.length || errors.length) process.exit(1);
