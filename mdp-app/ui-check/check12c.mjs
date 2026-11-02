// Screen check for sprint 12, viewing: cover pictures on listed memories,
// Previous and Next on an open memory, and the family summary beside the
// families list.
// Usage as check8.mjs: node check12c.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8779'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
await page.goto('http://127.0.0.1:8779/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);

const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check12c-'));
const files = [1, 2, 3].map((n) => {
  const p = path.join(src, `view ${n}.gif`);
  fs.writeFileSync(p, Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, n, 21]));
  return p;
});
await call('test_import', { paths: files });
const raw = (await call('raw_content', { kind: 'waiting' })).filter((r) => /^view \d/.test(r.name));
await call('memory_each', { content: raw.map((r) => r.id) });
const made = (await call('memory_list')).filter((m) => /^view \d/i.test(m.name));
check('threeMemories', made.length === 3);
const fam = await call('add_family', { name: 'The Viewers' });
const who = await call('add_individual', { kind: 'person', name: 'Vera Viewer' });
await call('add_family_member', { family: fam, individual: who });
await call('link_to_memory', { memory: made[0].id, who, role: 'in the picture' });

// 1. Assorted memories show a cover picture each, and Previous/Next move through them.
await page.click('button.navbtn[data-go="stories"]');
await page.waitForSelector('#loose-memories li');
const looseRows = page.locator('#loose-memories li', { hasText: /view \d/i });
check('assortedThumbs', (await looseRows.locator('.thumb').count()) === 3);
await axe('s12c-1-assorted');
const names = await page.locator('#loose-memories li .t .link').allTextContents();
await looseRows.first().locator('.t .link').click();
await page.waitForSelector('#h-memory');
const first = await page.textContent('#h-memory');
results.navText = await page.textContent('.memnav');
check('navShown', /1 of \d+ in Assorted memories/.test(results.navText));
check('prevDisabled', await page.locator('#mem-prev').isDisabled());
await axe('s12c-2-memory-nav');
await page.keyboard.press('ArrowRight');
await page.waitForFunction((f) => document.querySelector('#h-memory')?.textContent !== f, first);
check('arrowNext', (await page.textContent('#h-memory')) === names[names.indexOf(first) + 1]);
await page.click('#mem-prev');
await page.waitForFunction((f) => document.querySelector('#h-memory')?.textContent === f, first);
check('prevBack', true);
await page.click('#memory-body header .link'); // Back goes to the list, not the previous memory
await page.waitForSelector('#loose-memories li');
check('backToList', await page.locator('#loose-memories').isVisible());

// 2. A person's memories show their covers.
await page.evaluate((id) => openItem(id), who);
await page.waitForSelector('#h-person');
check('personMemoryThumb', (await page.locator('#person-body .lines li .thumb').count()) >= 1);

// 3. The family summary beside the families list.
await page.click('button.navbtn[data-go="people"]');
await page.waitForSelector('#family-list section.fam');
await page.click('#family-list section.fam h2 .link:text("The Viewers")');
await page.waitForSelector('#family-summary:not([hidden]) #h-famsum');
results.summary = await page.textContent('#family-summary .quiet');
check('summaryCounts', /1 person, \d+ stor(y|ies), 1 memory/.test(results.summary));
// Memory lines now also name their people (sprint 13), so look for the person's own link.
check('summaryPeople', (await page.locator('#family-summary .lines li button.link', { hasText: /^Vera Viewer$/ }).count()) === 1);
check('summaryMemoryThumb', (await page.locator('#family-summary .lines li .thumb').count()) === 1);
check('listStays', await page.locator('#family-list').isVisible());
check('sideBySide', (await page.$eval('#famsplit', (e) => getComputedStyle(e).display)) === 'grid');
await axe('s12c-3-family-summary');
await page.click('#family-summary .btns .btn.primary');
await page.waitForSelector('#h-family');
check('opensFamily', (await page.textContent('#h-family')) === 'The Viewers');
await page.click('button.navbtn[data-go="people"]');
await page.waitForSelector('#family-summary:not([hidden])');
check('summaryKept', true);
await page.click('#family-summary .row .btn');
check('summaryCloses', await page.locator('#family-summary').isHidden());

// 4. Home: memories to finish show their covers.
await page.click('button.navbtn[data-go="home"]');
await page.waitForSelector('#to-finish li');
await page.waitForFunction(() => !document.querySelector('#to-finish [data-cover-of]'));
results.homeThumbs = await page.locator('#to-finish li .thumb').count();
check('homeThumbs', results.homeThumbs >= 1);
await axe('s12c-4-home');

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
fs.rmSync(src, { recursive: true, force: true });
