// Screen check for sprint 15: brand navy and gold, the Density setting
// (Comfortable = 44 px targets), titles and dates on Waiting, and the
// Scrapbook look (Settings, kept on this device, journal spread, prints on
// the table, high contrast and reduced motion back to plain). axe runs in
// both looks, light and dark, and at 100%, 150% and 200% text size.
// Usage as check8.mjs: node check15.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8785'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
    await page.screenshot({ path: `${out}/${name}-${scheme}.png` });
  }
  await page.emulateMedia({ colorScheme: 'light' });
}
page.setDefaultTimeout(8000);
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8785/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const html = (k) => page.evaluate((key) => document.documentElement.dataset[key], k);
const show = async (pane) => { await page.evaluate((p) => go(p), pane); await page.waitForTimeout(300); };

// Pictures with a camera's name, a name with a title and year, and three in a memory.
const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check15-'));
const gif = (n) => Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, n]);
const names = ['IMG_20240512_103344.gif', 'Clipping 1995.gif', 'DSC_0042.gif', 'Pepper on the porch.gif', 'dinner-1.gif', 'dinner-2.gif'];
const paths = names.map((n, i) => { const p = path.join(src, n); fs.writeFileSync(p, gif(20 + i)); return p; });
const imported = await call('test_import', { paths });
results.imported = imported.length;
const mem = await call('add_memory', { name: 'Sunday dinners at the kitchen table', story: null });
await call('add_to_memory', { memory: mem, content: imported.slice(4) });
await page.reload();
await page.waitForSelector('#journey-title:not(:empty)');

// 1. Brand colors in the System look: navy buttons, gold on Home.
check('defaultLook', (await html('look')) === 'system' && (await html('density')) === 'compact');
const primary = () => page.$eval('.pane:not([hidden]) .btn.primary, #pv-open', (b) => getComputedStyle(b).backgroundColor);
await show('waiting');
check('navyButton', (await page.$eval('#add-btn', (b) => getComputedStyle(b).backgroundColor)) === 'rgb(44, 62, 80)');
await show('home');
check('goldEyebrow', (await page.$eval('.journey .eyebrow', (b) => getComputedStyle(b).color)) === 'rgb(154, 90, 20)');
results.primaryDark = await (async () => { await page.emulateMedia({ colorScheme: 'dark' }); const c = await page.$eval('.previewcard .btn.primary', (b) => getComputedStyle(b).backgroundColor); await page.emulateMedia({ colorScheme: 'light' }); return c; })();

// 2. Titles and dates on Waiting; the file name stays on the small line.
await show('waiting');
await page.waitForSelector('#waiting-list li');
const rows = await page.$$eval('#waiting-list li .t', (l) => l.map((t) => `${t.querySelector('.link').textContent}|${t.querySelector('small').textContent}`));
results.rows = rows;
check('titleFromName', rows.some((r) => r.startsWith('Clipping|') && r.includes('Clipping 1995.gif')));
check('dateWhenNoTitle', rows.some((r) => /^Picture from 2024/.test(r) && r.includes('IMG_20240512_103344.gif')));
check('fileNameWhenNothing', rows.some((r) => r.startsWith('DSC_0042.gif|')));
await page.fill('#wf-text', 'pepper');
await page.waitForTimeout(150);
check('searchFindsTitle', (await page.$$eval('#waiting-list li', (l) => l.length)) === 1);
await page.fill('#wf-text', 'IMG_2024');
await page.waitForTimeout(150);
check('searchFindsFileName', (await page.$$eval('#waiting-list li', (l) => l.length)) === 1);
await page.fill('#wf-text', '');
await page.dispatchEvent('#wf-text', 'input');
await axe('s15-1-system-waiting');

// 3. Density: Comfortable makes every visible button and field 44 px or more.
await show('settings');
await page.selectOption('#s-density', 'comfortable');
check('densitySet', (await html('density')) === 'comfortable');
const small = async () => page.$$eval('.pane:not([hidden]) :is(.btn, .tab, select, input[type=search], input[type=text]), nav .navbtn', (l) => l
  .filter((b) => b.offsetParent && !b.closest('[hidden]')).filter((b) => b.getBoundingClientRect().height < 44).map((b) => `${b.id || b.className}:${Math.round(b.getBoundingClientRect().height)}`));
for (const pane of ['home', 'waiting', 'settings']) {
  await show(pane);
  results[`small-${pane}`] = await small();
  check(`targets44-${pane}`, results[`small-${pane}`].length === 0);
}
await show('waiting');
await axe('s15-2-comfortable-waiting');
await page.reload();
await page.waitForSelector('#journey-title:not(:empty)');
check('densityKept', (await html('density')) === 'comfortable');
await show('settings');
await page.selectOption('#s-density', 'compact');

// 4. The Scrapbook look, chosen in Settings and kept on this device.
await page.selectOption('#s-look', 'scrapbook');
check('lookSet', (await html('look')) === 'scrapbook');
check('lookStored', (await page.evaluate(() => localStorage.getItem('mdp.look'))) === 'scrapbook');
await page.reload();
await page.waitForSelector('#journey-title:not(:empty)');
check('lookKept', (await html('look')) === 'scrapbook');
check('settingShown', (await page.$eval('#s-look', (s) => s.value)) === 'scrapbook');
check('paperIsCream', (await page.$eval('section.pane[data-pane="settings"]', (p) => getComputedStyle(p).backgroundColor)) === 'rgb(251, 247, 239)');
await show('settings');
await axe('s15-3-scrapbook-settings');
await show('home');
await axe('s15-4-scrapbook-home');
await show('waiting');
await page.selectOption('#view-waiting', 'tiles');
await page.waitForTimeout(200);
await axe('s15-5-scrapbook-tiles');
// Only the paper and the picture tilt; the caption and buttons stay straight.
check('printTilts', (await page.$eval('#waiting-list li', (li) => getComputedStyle(li, '::before').transform)) !== 'none');
check('buttonsStraight', (await page.$eval('#waiting-list li .split-main', (b) => getComputedStyle(b).transform)) === 'none'
  && (await page.$eval('#waiting-list li', (li) => getComputedStyle(li).transform)) === 'none');
await page.selectOption('#view-waiting', 'list');
await page.evaluate((id) => go('memory', { id }), mem);
await page.waitForSelector('#h-memory');
await page.waitForTimeout(300);
check('journalSpread', (await page.$eval('#memory-body', (b) => getComputedStyle(b).display)) === 'grid'
  && (await page.$eval('#memory-body > .page-left', (b) => b.getBoundingClientRect().right)) < (await page.$eval('#memory-body > form.page-right', (b) => b.getBoundingClientRect().left)));
await axe('s15-6-scrapbook-memory');

// 5. High contrast and "more contrast" always show the System look.
await page.emulateMedia({ forcedColors: 'active' });
await page.waitForTimeout(150);
check('forcedColorsPlain', (await html('look')) === 'system');
await show('settings');
check('forcedColorsNote', await page.isVisible('#s-look-hc'));
await page.screenshot({ path: `${out}/s15-7-forced-colors.png` });
await page.emulateMedia({ forcedColors: 'none' });
await page.waitForTimeout(150);
check('backToScrapbook', (await html('look')) === 'scrapbook');
await page.emulateMedia({ contrast: 'more' }).catch(() => { results.contrastEmulation = 'not supported'; });
if (!results.contrastEmulation) {
  await page.waitForTimeout(150);
  check('moreContrastPlain', (await html('look')) === 'system');
  await page.emulateMedia({ contrast: 'no-preference' });
}

// 6. Reduced motion: nothing moves, the tilts stay.
await page.emulateMedia({ reducedMotion: 'reduce' });
await show('waiting');
await page.selectOption('#view-waiting', 'tiles');
check('noMotion', (await page.$eval('#waiting-list li', (li) => getComputedStyle(li, '::before').transitionDuration)) === '0s');
await page.emulateMedia({ reducedMotion: 'no-preference' });

// 7. Text size 150% and 200% (the window acts 1.5 and 2 times narrower): no
// sideways scrolling in either look.
for (const [size, w, h] of [['150', 853, 667], ['200', 640, 500]]) {
  await page.setViewportSize({ width: w, height: h });
  for (const look of ['system', 'scrapbook']) {
    await page.evaluate((l) => { localStorage.setItem('mdp.look', l); applyLook(); }, look);
    for (const pane of ['home', 'waiting', 'settings']) {
      await show(pane);
      const wide = await page.$eval('main', (m) => m.scrollWidth - m.clientWidth);
      results[`overflow-${size}-${look}-${pane}`] = wide;
      check(`noSideways-${size}-${look}-${pane}`, wide <= 1);
    }
    await page.screenshot({ path: `${out}/s15-8-${size}-${look}-settings.png` });
  }
}
await page.setViewportSize({ width: 1280, height: 1000 });

check('noErrors', errors.length === 0);
console.log(JSON.stringify({ results, errors }, null, 2));
await browser.close();
server.kill();
