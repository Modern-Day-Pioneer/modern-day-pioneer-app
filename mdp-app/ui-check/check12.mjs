// Screen check for sprint 12: the Waiting row's split button (click makes a
// memory; the arrow, a right-click or a long press opens the other
// actions) and assigning items to several people typed with commas.
// Usage as check8.mjs: node check12.mjs <folder for screenshots>
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
await page.goto('http://127.0.0.1:8776/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);

const src = fs.mkdtempSync(path.join(os.tmpdir(), 'mdp-check12-'));
const files = [1, 2, 3].map((n) => {
  const p = path.join(src, `picture ${n}.gif`);
  fs.writeFileSync(p, Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 4, 1, 0, 0, 0, 0, 0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3B, n, 12]));
  return p;
});
await call('test_import', { paths: files });
// Two people with the same name, told apart by their years.
for (const born of ['1912', '2015']) {
  const who = await call('add_individual', { kind: 'person', name: 'Ada Twin' });
  await call('update_individual', { id: who, changes: { born } });
}
await page.click('button.navbtn[data-go="waiting"]');
await page.waitForSelector('#waiting-list li .split-main');

// 1. One split button per row, nothing else.
const row = page.locator('#waiting-list li', { hasText: 'picture 1' });
results.rowButtons = await row.locator('button.btn').allTextContents();
check('oneSplitButton', results.rowButtons.length === 2 && /Make a memory/.test(results.rowButtons[0]));
await row.locator('.split-more').click();
results.menu = await row.locator('[role=menuitem]').allTextContents();
check('menuHasActions', ['Assign to an individual…', 'Set aside', 'Remove from Journey…'].every((a) => results.menu.includes(a)) && results.menu.length === 3);
await axe('s12-1-row-menu');
await page.keyboard.press('Escape');
check('escapeCloses', await row.locator('.rowmenu').isHidden());
// A right-click and a long press open it too.
await row.locator('.split-main').click({ button: 'right' });
check('rightClickOpens', await row.locator('.rowmenu').isVisible());
await page.mouse.click(5, 5);
check('clickOutsideCloses', await row.locator('.rowmenu').isHidden());
const box = await row.locator('.split-main').boundingBox();
await page.mouse.move(box.x + 10, box.y + 10);
await page.mouse.down();
await page.waitForTimeout(700);
await page.mouse.up();
check('longPressOpens', await row.locator('.rowmenu').isVisible());
check('longPressNoMemoryDialog', !(await page.isVisible('dialog.ask[open]')));

// 2. Assign to several people with commas; a new name is added at once.
await row.locator('[role=menuitem]:has-text("Assign")').click();
await page.waitForSelector('dialog.ask[open] input');
await page.fill('dialog.ask[open] input', 'Walter Rowan, Brand New Cousin');
results.hint = await page.textContent('dialog.ask[open] [aria-live]');
check('hintShowsNew', /Walter Rowan/.test(results.hint) && /Brand New Cousin \(new person\)/.test(results.hint));
await axe('s12-2-assign-names');
await page.click('dialog.ask[open] button:text("Done")');
await page.click('dialog.ask[open]:has-text("The memory is ready") button:has-text("Back to Waiting")');
await page.waitForSelector('dialog.ask[open]', { state: 'detached' });
const m = (await call('memories')).find((x) => x.name.startsWith('Walter Rowan and Brand New Cousin'));
check('memoryMade', !!m);
check('newPersonAdded', (await call('browse', { kinds: ['person'] })).some((h) => h.name === 'Brand New Cousin'));

// 3. Same-named people: choose by year.
await page.locator('#waiting-list li', { hasText: 'picture 3' }).locator('.split-more').click();
await page.locator('#waiting-list li', { hasText: 'picture 3' }).locator('[role=menuitem]:has-text("Assign")').click();
await page.fill('dialog.ask[open] input', 'Ada Twin');
results.twinChips = await page.locator('dialog.ask[open] .chips button').allTextContents();
check('twinsShowYears', results.twinChips.includes('Ada Twin (born 1912)') && results.twinChips.includes('Ada Twin (born 2015)'));
await page.click('dialog.ask[open] button:text("Done")');
check('doneAsksWhichOne', /Choose the right one/.test(await page.textContent('dialog.ask[open]')));
await page.click('dialog.ask[open] .chips button:text("Ada Twin (born 2015)")');
await axe('s12-3-twins');
await page.click('dialog.ask[open] button:text("Done")');
await page.click('dialog.ask[open]:has-text("The memory is ready") button:has-text("Back to Waiting")');
await page.waitForSelector('dialog.ask[open]', { state: 'detached' });
check('twinMemory', (await call('memories')).some((x) => x.name.startsWith('Ada Twin: picture 3')));

// 4. A plain click still makes a memory.
await page.locator('#waiting-list li', { hasText: 'picture 2' }).locator('.split-main').click();
await page.waitForSelector('dialog.ask[open]');
check('clickMakesMemory', /memory/i.test(await page.textContent('dialog.ask[open]')));

await page.keyboard.press('Escape');
await page.waitForSelector('dialog.ask[open]', { state: 'detached' });

// 5. The memory screen: the Who box takes several names; the Place box suggests or creates.
const mem = (await call('memories')).find((x) => x.name.startsWith('Walter Rowan and Brand New Cousin'));
await page.evaluate((id) => go('memory', { id }), mem.id);
await page.waitForSelector('#h-memory');
await page.fill('form[aria-label="Add people, pets or families"] input', 'June Rowan, Brand New Aunt');
await page.click('form[aria-label="Add people, pets or families"] button[type=submit]');
await page.waitForTimeout(600);
let mv = await call('memory_view', { id: mem.id });
check('whoBoxLinksSeveral', ['June Rowan', 'Brand New Aunt'].every((n) => mv.people.some((p) => p.name === n)));
await page.fill('.picker:has(label:text("Place")) input', 'Disneyland');
await page.click('[role=option]:has-text("Add “Disneyland” as a new place")');
await page.waitForTimeout(600);
mv = await call('memory_view', { id: mem.id });
check('placeCreatedAndSet', mv.place && mv.place.name === 'Disneyland');
await page.waitForSelector('#h-memory');
check('placeShown', (await page.inputValue('.picker:has(label:text("Place")) input')) === 'Disneyland');
await axe('s12-5-memory-who-place');

// 6. Waiting: tiles, and the selection bar stays in view.
await page.click('button.navbtn[data-go="waiting"]');
await page.waitForSelector('#waiting-list li');
check('barSticky', (await page.$eval('#sel-bar', (e) => getComputedStyle(e).position)) === 'sticky');
await page.selectOption('#view-waiting', 'tiles');
check('tilesGrid', (await page.$eval('#waiting-list', (e) => getComputedStyle(e).display)) === 'grid');
await page.locator('#waiting-list li').first().locator('input[type=checkbox]').check();
check('tileSelects', /selected/.test(await page.textContent('#sel-count')));
check('tileHasSplit', await page.locator('#waiting-list li').first().locator('.split-main').isVisible());
await axe('s12-6-tiles');
await page.reload();
await page.waitForSelector('#journey-title:not(:empty)');
check('viewRemembered', (await page.inputValue('#view-waiting')) === 'tiles');

// 7. Bring in from Google Photos: the steps, with a button to Google Takeout.
await page.click('button.navbtn[data-go="waiting"]');
await page.waitForSelector('#waiting-list li');
await page.click('#add-btn');
await page.click('#add-zips');
await page.waitForSelector('#takeout-dialog[open]');
check('takeoutSteps', (await page.locator('#takeout-dialog .steps li').count()) === 3);
check('takeoutButtons', await page.locator('#takeout-open').isVisible() && await page.locator('#takeout-choose').isVisible());
await axe('s12-7-takeout-steps');
await page.click('#takeout-dialog button[value=cancel]');
check('takeoutCloses', !(await page.locator('#takeout-dialog[open]').count()));

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /(light|dark)$/.test(k) && v.length);
process.exit(errors.length || bad.length ? 1 : 0);
