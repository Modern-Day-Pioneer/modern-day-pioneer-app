// Screen check for sprint 12: Add from waiting on a story's page makes a
// new memory in the story from the items ticked.
// Usage as check8.mjs: node check12f.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8782'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
await page.goto('http://127.0.0.1:8782/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);

const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check12f-'));
const files = [1, 2].map((n) => {
  const p = path.join(src, `porch ${n}.gif`);
  fs.writeFileSync(p, Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, n, 51]));
  return p;
});
await call('test_import', { paths: files });
const story = await call('add_story', { name: 'Summers' });
await page.evaluate((id) => go('story', { id }), story);
await page.waitForSelector('#h-story');
await page.click('#story-body button:has-text("Add from waiting…")');
await page.waitForSelector('dialog[open] ul.pick');
await axe('s12f-1-pick');
await page.locator('dialog[open] li', { hasText: 'porch 1' }).locator('input').check();
await page.locator('dialog[open] li', { hasText: 'porch 2' }).locator('input').check();
await page.click('dialog[open] button:has-text("Done")');
await page.waitForSelector('dialog[open]:has-text("The memory is ready")');
await page.click('dialog[open] button:has-text("Stay on this page")');
await page.waitForSelector('#story-body li:has-text("porch 1")');
const v = await call('story_view', { id: story });
results.storyMemories = v.memories.map((m) => m.name);
check('memoryInStory', v.memories.length === 1 && v.memories[0].name === 'porch 1');
const mv = await call('memory_view', { id: v.memories[0].id });
check('bothItems', mv.items.length === 2);
await axe('s12f-2-story');

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
fs.rmSync(src, { recursive: true, force: true });
