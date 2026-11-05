// Screen check for sprint 17: Scrapbook is the default Look (a chosen System
// stays); featured stories show their cover; Waiting opens as Tiles and shows
// kinds as named icons, not words; the export's Scrapbook style is a Journey
// setting, reads without its stylesheet (E15) and falls back to plain for
// high contrast and print. axe runs light and dark.
// Usage as check8.mjs: node check17.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8787'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
async function axe(name, schemes = ['light', 'dark']) {
  for (const scheme of schemes) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.waitForTimeout(200);
    await page.evaluate(axeSrc);
    results[`${name} ${scheme}`] = await page.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'best-practice'] }))
      .violations.map((v) => `${v.id}: ${v.nodes.length} (${v.nodes.slice(0, 2).map((n) => n.target.join(' ')).join(' | ')})`));
    await page.screenshot({ path: `${out}/${name}-${scheme}.png` });
  }
  await page.emulateMedia({ colorScheme: 'light' });
}
page.setDefaultTimeout(15000);
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
// The test server makes no thumbnails (no FFmpeg): each picture shows a sample photo.
await page.addInitScript((dir) => {
  const real = window.__TAURI__.core.invoke;
  const pics = ['camera-on-tripod.jpg', 'cup-of-coffee.jpg', 'tabby-cat.jpg'];
  window.__TAURI__.core.invoke = (cmd, args) => cmd === 'thumbnail'
    ? Promise.resolve(`/file/${dir}/${pics[[...args.id].reduce((t, c) => t + c.charCodeAt(0), 0) % 3]}`)
    : real(cmd, args);
}, `${repo}/app/mdp-core/sample-media`);
await page.goto('http://127.0.0.1:8787/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const show = async (pane) => { await page.evaluate((p) => go(p), pane); await page.waitForTimeout(500); };
const reload = async () => { await page.reload(); await page.waitForSelector('#journey-title:not(:empty)'); };

// Scrapbook is the default Look for now; a chosen System stays.
check('scrapbookByDefault', (await page.evaluate(() => document.documentElement.dataset.look)) === 'scrapbook');
await page.evaluate(() => localStorage.setItem('mdp.look', 'system'));
await reload();
check('chosenSystemKept', (await page.evaluate(() => document.documentElement.dataset.look)) === 'system');
await page.evaluate(() => localStorage.removeItem('mdp.look'));
await reload();

// Featured stories show their cover picture.
for (const s of await call('stories', {})) await call('set_featured', { id: s.id, on: true }).catch(() => {});
await reload();
await show('home');
await page.waitForTimeout(800);
const feat = await page.$$eval('#featured li', (ls) => ls.map((l) => !!l.querySelector('.thumb img')));
results.featured = feat;
check('featuredCovers', feat.length > 0 && feat.every(Boolean));
// Home's Waiting box: small tiles by default; its tag switches to a list and steps the size.
const hb = '.homebox';
check('homeTilesByDefault', (await page.$eval(hb, (b) => b.dataset.view)) === 'tiles');
const tileOf = () => page.$eval(hb, (b) => b.style.getPropertyValue('--tile'));
const before = await tileOf();
await page.click(`${hb} .boxtag button[aria-label^="Larger"]`);
check('homeBigger', (await tileOf()) !== before);
await page.click(`${hb} .boxtag button[aria-label$="as a list"]`);
check('homeList', (await page.$eval(hb, (b) => b.dataset.view)) === 'list');
await page.click(`${hb} .boxtag button[aria-label$="as tiles"]`);
await page.click(`${hb} .boxtag button[aria-label^="Smaller"]`);
await page.$eval('#featured-panel', (p) => p.scrollIntoView());
await axe('01-home');

// Waiting opens as Tiles, with kinds as named icons, not words.
await show('waiting');
check('tilesByDefault', (await page.$eval('#view-waiting', (s) => s.value)) === 'tiles');
await axe('02-waiting-tiles');
// The Waiting box's tag drives the same choices as the selects.
await page.click('.waitbox:not(.homebox) .boxtag button[aria-label^="Larger"]');
check('waitingBigger', (await page.$eval('#thumbs-waiting', (s) => s.value)) === 'l');
await page.click('.waitbox:not(.homebox) .boxtag button[aria-label$="as a list"]');
await page.waitForTimeout(400);
const lines = await page.$$eval('section.pane[data-pane="waiting"] .kindline', (ls) => ls.map((l) => l.textContent.trim()));
results.kindlines = lines.slice(0, 3);
check('noKindWords', lines.length > 0 && lines.every((t) => !/^(Picture|Recording|Video|Text)\b/.test(t)));
check('iconsNamed', await page.$$eval('section.pane[data-pane="waiting"] .kindline .kind-ic[role="img"]',
  (ls) => ls.length > 0 && ls.every((l) => l.getAttribute('aria-label'))));
await axe('03-waiting-list');
await page.evaluate(() => localStorage.removeItem('mdp.view.waiting'));

// The export's look: a Journey setting beside the front door.
await show('settings');
check('exportStyleSetting', (await page.$$eval('#s-style option', (o) => o.map((x) => x.value))).join() === 'plain,scrapbook');
await call('start_export', { export_style: 'scrapbook' });
const st = await call('export_status', {});
check('exported', !!st.report);
const sheet = path.join(st.folder, 'pages', 'scrapbook.css');
check('scrapbookSheet', fs.existsSync(sheet));
const front = 'file://' + path.join(st.folder, 'Open me - Family History.html');
const ex = await browser.newPage({ viewport: { width: 1280, height: 900 } });
const bodyImage = () => ex.$eval('body', (b) => getComputedStyle(b).backgroundImage);
for (const scheme of ['light', 'dark']) {
  await ex.emulateMedia({ colorScheme: scheme });
  await ex.goto(front);
  await ex.waitForTimeout(200);
  await ex.evaluate(axeSrc);
  results[`04-export ${scheme}`] = await ex.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa'] })).violations.map((v) => `${v.id}: ${v.nodes.length}`));
  await ex.screenshot({ path: `${out}/04-export-${scheme}.png` });
}
await ex.emulateMedia({ colorScheme: 'light' });
check('themed', (await bodyImage()) !== 'none');
// E15: with the Scrapbook sheet removed, the page reads as plain.
fs.renameSync(sheet, sheet + '.off');
await ex.goto(front);
check('readsWithoutTheme', (await bodyImage()) === 'none' && (await ex.$$('.book')).length > 0);
fs.renameSync(sheet + '.off', sheet);
// High contrast and print fall back to plain.
await ex.emulateMedia({ forcedColors: 'active' });
await ex.goto(front);
check('highContrastPlain', (await bodyImage()) === 'none');
await ex.emulateMedia({ forcedColors: 'none', media: 'print' });
await ex.goto(front);
check('printPlain', (await bodyImage()) === 'none');

console.log(JSON.stringify({ results, errors }, null, 2));
const bad = Object.entries(results).filter(([k, v]) => / (light|dark)$/.test(k) && v.length);
await browser.close();
server.kill();
if (errors.length || bad.length) { console.log('PROBLEMS', JSON.stringify({ bad, errors })); process.exit(1); }
console.log('ALL GOOD');
