// Screen check for sprint 8 (release 1.1): sorting and filtering memories,
// picture sizes per screen, typed changes kept when adding a relationship,
// portraits, adding people to a family by choosing and by dragging, and
// turning a picture.
// Usage as check7.mjs: node check8.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8772'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 1100 } });
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
page.on('response', (r) => { if (r.status() >= 400) errors.push('HTTP ' + r.status() + ' ' + r.url()); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8772/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

// 1. Stories & Memories: sort and filter, and the picture size is kept per screen.
await nav('stories', '#h-stories');
check('byStoryFirst', await page.isHidden('#memory-results') && await page.isVisible('#story-list'));
await page.selectOption('#mf-sort', 'name');
await page.waitForSelector('#memory-results:not([hidden])');
const names = await page.$$eval('#results-list .t .link', (l) => l.map((x) => x.textContent));
results.byName = names;
check('sortedByName', names.length >= 4 && names.join('|') === [...names].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' })).join('|'));
await page.selectOption('#mf-sort', 'date');
results.byDate = await page.$$eval('#results-list .t .link', (l) => l.map((x) => x.textContent));
check('weddingFirstByDate', results.byDate[0] === "Walter and June's wedding day");
await page.selectOption('#mf-person', { label: 'Ellen Rowan' });
results.ellen = await page.$$eval('#results-list .t .link', (l) => l.map((x) => x.textContent));
check('filterByPerson', results.ellen.length === 1 && results.ellen[0] === 'Summers in Boise');
await page.selectOption('#mf-person', '');
await page.fill('#mf-text', 'camera');
check('filterByTitle', (await page.$$eval('#results-list li', (l) => l.length)) === 1);
await page.fill('#mf-text', '');
await page.check('#mf-needs');
check('filterNeedsContent', /of/.test(await page.textContent('#results-count')));
await page.uncheck('#mf-needs');
await page.selectOption('#thumbs-stories', 'l');
check('largeThumbs', (await page.$eval('section.pane[data-pane="stories"]', (p) => p.dataset.thumbs)) === 'l');
await axe('s8-1-sort-filter');
await page.selectOption('#mf-sort', 'story');
await page.reload();
await page.waitForSelector('#journey-title:not(:empty)');
await nav('stories', '#h-stories');
check('sizeRemembered', (await page.inputValue('#thumbs-stories')) === 'l');

// 2. Typed changes are kept when a relationship is added (the sprint 8 bug).
await nav('people', '#h-people');
await page.click('.personcard button.person:has-text("Ellen Rowan")');
await page.waitForSelector('#h-person');
const about = page.getByLabel('About (shown on their page)');
await about.fill('Typed but not saved yet.');
const rel = page.locator('#person-body form[aria-label="Add a relationship"]');
const who = rel.getByRole('combobox', { name: 'Who' });
await who.click();
await who.fill('Pepper');
await page.waitForTimeout(300);
await page.keyboard.press('ArrowDown');
await page.keyboard.press('Enter');
const lis = await page.$$eval('#person-body ul.lines li', (l) => l.length);
await rel.locator('button[type=submit]').click();
await page.waitForFunction((n) => document.querySelectorAll('#person-body ul.lines li').length > n, lis);
check('typedStillShown', (await about.inputValue()) === 'Typed but not saved yet.');
const ellenView = await call('individual_view', { id: await page.evaluate(() => currentId) });
check('typedChangeSaved', ellenView.description === 'Typed but not saved yet.');

// 3. A portrait from a picture in their memories.
await nav('people', '#h-people');
await page.click('.personcard button.person:has-text("Walter Rowan")');
await page.waitForSelector('#person-body .portraits button');
await page.click('#person-body .portraits button');
await page.waitForSelector('#person-body button:has-text("No portrait")');
const faces = await call('portraits', {});
check('portraitChosen', Object.keys(faces).length === 1);
await axe('s8-3-portrait');

// 4. Choose people and add them to a family; then drag one onto a family.
const fam = await call('add_family', { name: 'The Hales' });
await nav('people', '#h-people');
await page.check('section.fam[aria-label="The Rowans"] .personcard:has-text("June Rowan") input');
await page.check('section.fam[aria-label="The Rowans"] .personcard:has-text("Ellen Rowan") input');
check('chosenBar', /2 people chosen/.test(await page.textContent('#chosen-count')));
await page.selectOption('#choose-family', fam);
await page.click('#add-chosen');
await page.waitForFunction(() => document.querySelectorAll('section.fam[aria-label="The Hales"] .personcard').length === 2);
check('addedByChoosing', true);
await axe('s8-4-add-to-family');
const card = page.locator('section.fam[aria-label="The Rowans"] .personcard:has-text("Walter Rowan") button.person');
const target = page.locator('section.fam[aria-label="The Hales"]');
const a = await card.boundingBox();
const b = await target.boundingBox();
await page.mouse.move(a.x + 10, a.y + 10);
await page.mouse.down();
await page.mouse.move(a.x + 40, a.y + 40, { steps: 4 });
await page.mouse.move(b.x + 30, b.y + 15, { steps: 6 });
await page.mouse.up();
await page.waitForFunction(() => document.querySelectorAll('section.fam[aria-label="The Hales"] .personcard').length === 3);
check('addedByDragging', true);

// 5. Turn a picture in a memory: the menu has Turn right and Turn left.
await nav('stories', '#h-stories');
await page.click('details.storygroup .link:has-text("Walter\'s camera on the lawn")');
await page.waitForSelector('#h-memory');
const pic = (await call('memory_view', { id: await page.evaluate(() => currentId) })).items.find((i) => i.kind === 'picture');
await page.locator('#memory-body li.item').filter({ hasText: pic.name }).locator('button[aria-haspopup]').click();
await page.click('button:has-text("Turn right")');
await page.waitForTimeout(400);
const after = (await call('memory_view', { id: await page.evaluate(() => currentId) })).items.find((i) => i.id === pic.id);
check('turnedRight', after.turn === 1);
await axe('s8-5-turned');
console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /(light|dark)$/.test(k) && v.length);
process.exit(errors.length || bad.length ? 1 : 0);
