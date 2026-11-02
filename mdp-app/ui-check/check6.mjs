// Screen check for sprint 6: the Preview screen's note of what is left out,
// the preview itself (made by the export's own templates), and Export with
// its progress and result. Usage as check5.mjs: node check6.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8770'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 1100 } });
async function axe(name, p = page) {
  for (const scheme of ['light', 'dark']) {
    await p.emulateMedia({ colorScheme: scheme });
    await p.waitForTimeout(150);
    await p.evaluate(axeSrc);
    results[`${name} ${scheme}`] = await p.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'best-practice'] }))
      .violations.map((v) => `${v.id}: ${v.nodes.length} (${v.nodes.slice(0, 2).map((n) => n.target.join(' ')).join(' | ')})`));
  }
  await p.emulateMedia({ colorScheme: 'light' });
  await p.screenshot({ path: `${out}/${name}.png`, fullPage: true });
}
page.setDefaultTimeout(8000);
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('response', (r) => { if (r.status() >= 400) errors.push('HTTP ' + r.status() + ' ' + r.url()); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8770/');
await page.waitForSelector('#journey-title:not(:empty)');
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };

// 1. The Preview screen says what is left out and where the export goes.
await page.click('button.navbtn[data-go="preview"]');
await page.waitForSelector('#pv-note:not(:empty)');
results.note = await page.textContent('#pv-note');
check('noteSaysWhatIsLeftOut', /^Left out: .*memor/.test(results.note) && results.note.includes('not in a memory'));
check('noteNamesPrivacy', results.note.includes('Hidden for privacy'));
check('folderShown', (await page.textContent('#ex-folder')).length > 0);
await axe('p1-preview-screen');

// 2. The preview is made by the same templates and opens on its own.
await page.click('#pv-open');
await page.waitForSelector('#pv-open:not([disabled])');
const settings = await call('get_settings');
const front = `${settings.journey_folder}/cache/preview/Open me - Family History.html`;
check('previewMade', fs.existsSync(front));
const pv = await browser.newPage({ viewport: { width: 1100, height: 900 } });
pv.on('response', (r) => { if (r.status() >= 400) errors.push('HTTP ' + r.status() + ' ' + r.url()); });
await pv.goto(`http://127.0.0.1:8770/file/${encodeURIComponent(front).replace(/%2F/g, '/')}`);
check('previewHasNote', await pv.isVisible('.pv-banner'));
check('previewIsBookshelf', await pv.isVisible('.bookcase'));
await axe('p2-preview-bookshelf', pv);
await pv.click('nav a:has-text("Timeline")');
await pv.waitForSelector('h1:text("Timeline")');
results.timeline = await pv.$$eval('ol.timeline li', (l) => l.map((x) => x.textContent.replace(/\s+/g, ' ').trim()));
check('timelineInOrder', results.timeline.length >= 2);
await axe('p3-preview-timeline', pv);
await pv.goto(`http://127.0.0.1:8770/file/${encodeURIComponent(front).replace(/%2F/g, '/')}`);
await pv.click('.book.face');
await pv.waitForSelector('.leaf.cover');
results.storyImages = await pv.$$eval('img', (i) => i.filter((x) => x.complete && x.naturalWidth > 0).length);
check('storyPicturesShow', results.storyImages >= 1);
await axe('p4-preview-story', pv);
// Phone width: nothing wider than the screen.
await pv.setViewportSize({ width: 390, height: 800 });
results.phoneWidth = await pv.evaluate(() => document.documentElement.scrollWidth);
check('storyFitsPhone', results.phoneWidth <= 390);
await pv.screenshot({ path: `${out}/p5-preview-story-phone.png`, fullPage: true });
await pv.close();

// 3. Export: progress, then the result with Open and Show the folder.
await page.click('#ex-start');
await page.waitForSelector('#ex-done:not([hidden])');
results.exported = await page.textContent('#ex-done-msg');
check('exportReported', results.exported.startsWith('Exported '));
check('openOffered', await page.isVisible('#ex-open'));
await axe('p6-exported');
const info = await call('export_info');
check('exportOnDisk', info.exported && fs.existsSync(`${info.folder}/catalog.json`) && fs.existsSync(`${info.folder}/ABOUT-THIS-FOLDER.txt`));
// Exporting again replaces the old export whole.
await page.click('#ex-start');
await page.waitForSelector('#ex-done:not([hidden])');
check('exportAgain', (await page.textContent('#ex-done-msg')).startsWith('Exported '));

// Home's Preview card leads here.
await page.click('button.navbtn[data-go="home"]');
await page.click('.previewcard button[data-go="preview"]');
check('homeLeadsToPreview', await page.isVisible('#h-preview'));

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /(light|dark)$/.test(k) && v.length);
process.exit(errors.length || bad.length ? 1 : 0);
