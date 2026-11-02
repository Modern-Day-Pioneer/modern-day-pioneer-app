// Screen check for sprint 7: home counts that open their screens, the live
// waiting count, Memories to finish with "could add", the home's small
// bookshelf, Stories & Memories, Select all, the large preview's
// actions, "Add an individual", and back up and restore from Settings.
// Usage as check6.mjs: node check7.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8771'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
await page.goto('http://127.0.0.1:8771/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const badge = async () => Number((await page.textContent('#wbadge')) || 0);
const home = async () => { await page.click('button.navbtn[data-go="home"]'); await page.waitForSelector('#h-stories', { state: 'hidden' }); await page.waitForTimeout(200); };

// 1. Home: Memories to finish holds placeholders and memories that could use
// more. A memory holding only a typed note could use a date, place, people
// and a story. The set-aside coffee goes back to waiting, so two items wait.
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const typed = await call('add_memory', { name: 'A picture from the box', story: null });
await call('add_note', { memory: typed, title: '', text: 'Found in the attic.', fromCaption: false });
const aside = await call('raw_content', { kind: 'set aside' });
await call('set_state', { ids: aside.map((x) => x.id), to: 'waiting' });
await page.click('button.navbtn[data-go="people"]');
await home();
await page.waitForSelector('#to-finish li');
results.toFinish = await page.$$eval('#to-finish li', (l) => l.map((x) => x.textContent.replace(/\s+/g, ' ').trim()));
check('placeholdersListed', results.toFinish.some((t) => t.includes('Needs a photo')));
check('couldAddListed', results.toFinish.some((t) => t.includes('Could add')));
check('emptyLineHidden', await page.isHidden('#to-finish-empty'));
// The preview card is a small bookshelf of the stories the export shows.
results.shelf = await page.$$eval('#shelf-mini .spine', (l) => l.map((x) => x.textContent));
check('shelfShowsStories', results.shelf.length >= 2);
await axe('h1-home');

// 2. Each count opens the screen where those are changed.
results.counts = await page.$$eval('#counts button', (l) => l.map((x) => `${x.textContent.trim()} -> ${x.dataset.go}`));
check('countsAreButtons', results.counts.length === 6);
await page.click('#counts button:has-text("memories")');
check('memoriesOpenStories', !!(await page.waitForSelector('#h-stories')));
await home();
await page.click('#counts button:has-text("places")');
check('placesOpenPlaces', !!(await page.waitForSelector('#h-places')));
await home();
await page.click('#counts button:has-text("pet")');
check('petsOpenPeople', !!(await page.waitForSelector('#h-people')));

// 3. Add an individual, everywhere.
check('addIndividualButton', (await page.textContent('#add-person-btn')).trim() === 'Add an individual…');
await page.click('#add-person-btn');
check('addIndividualDialog', (await page.textContent('#add-dialog-h')) === 'Add an individual');
await page.click('#add-cancel');

// 4. Stories & Memories: each story opens to its memories, then Assorted memories.
check('sidebarRenamed', (await page.textContent('button.navbtn[data-go="stories"]')).trim() === 'Stories & Memories');
await page.click('button.navbtn[data-go="stories"]');
await page.waitForSelector('details.storygroup');
results.groups = await page.$$eval('details.storygroup', (l) => l.map((d) => ({ open: d.open, story: d.querySelector('summary .name').textContent,
  memories: [...d.querySelectorAll('.todo .t .link')].map((x) => x.textContent) })));
check('storiesGrouped', results.groups.length >= 2 && results.groups.every((g) => g.open) && results.groups.some((g) => g.memories.length >= 2));
check('assortedHeading', (await page.textContent('#h-loose')) === 'Assorted memories');
await axe('h2-stories-and-memories');

// 5. A memory with content says what it could still use; each word jumps to its box.
await page.click('#loose-memories .link:has-text("A picture from the box")');
await page.waitForSelector('#h-memory');
results.could = await page.textContent('#memory-body .could');
check('couldAddOnMemory', /could use/.test(results.could));
await page.click('#memory-body .could button:has-text("a date")').catch(() => {});
await axe('h3-memory-could-add');

// 6. Waiting: Select all is the box at the top; the sidebar count follows every move.
await page.click('button.navbtn[data-go="waiting"]');
await page.waitForSelector('#waiting-list li');
const before = await badge();
const rows = await page.$$eval('#waiting-list li', (l) => l.length);
check('badgeMatchesList', before === rows);
await page.check('#sel-all');
check('selectAllSelects', (await page.$$eval('#waiting-list input[type=checkbox]', (l) => l.every((c) => c.checked))) && /selected/.test(await page.textContent('#sel-count')));
await page.uncheck('#sel-all');
await page.check('#waiting-list li:first-child input[type=checkbox]');
check('halfChecked', rows < 2 || await page.$eval('#sel-all', (c) => c.indeterminate));
await page.uncheck('#waiting-list li:first-child input[type=checkbox]');
await axe('h4-waiting-select-all');

// 7. The large preview has the list's actions; Set aside from there moves the item and the count.
await page.click('#waiting-list li:first-child .t .link');
await page.waitForSelector('dialog.viewer[open] .viewer-actions');
results.viewerActions = await page.$$eval('dialog.viewer .viewer-actions button', (l) => l.map((x) => x.textContent));
check('viewerHasWaitingActions', ['Make a memory…', 'Assign to an individual…', 'Set aside', 'Remove from Journey…'].every((a) => results.viewerActions.includes(a)));
await axe('h5-viewer-actions');
await page.click('dialog.viewer .viewer-actions button:has-text("Set aside")');
await page.waitForFunction((n) => document.querySelectorAll('#waiting-list li').length === n - 1, rows);
check('badgeFollowsSetAside', (await badge()) === before - 1);
// Undo puts it back, and the count with it.
await page.keyboard.press('Control+z');
await page.waitForFunction((n) => document.querySelectorAll('#waiting-list li').length === n, rows);
check('badgeFollowsUndo', (await badge()) === before);

// 8. Back up from Settings, then restore the backup into a new folder.
await page.click('button.navbtn[data-go="settings"]');
await page.waitForSelector('#bk-last:not(:empty)');
const info = await page.evaluate(() => window.__TAURI__.core.invoke('backup_info'));
await page.evaluate((p) => { window.nextSave = p; }, info.suggested);
await page.click('#bk-start');
await page.waitForSelector('#bk-done:not([hidden])');
results.backupMsg = await page.textContent('#bk-done-msg');
check('backupMade', results.backupMsg.startsWith('Backed up to') && fs.existsSync(info.suggested));
check('lastBackupShown', (await page.textContent('#bk-last')).startsWith('Last backed up'));
await axe('h6-backup-done');
await page.evaluate((p) => { window.nextOpen = p; }, info.suggested);
await page.click('#bk-restore');
await page.waitForSelector('dialog.ask[open]');
results.restoreAsk = await page.textContent('dialog.ask[open] p');
await axe('h7-restore-ask');
await page.click('dialog.ask[open] button[type=submit]');
await page.waitForSelector('#bk-done:not([hidden]) #bk-open:not([hidden])');
results.restoreMsg = await page.textContent('#bk-done-msg');
const restored = results.restoreMsg.replace(/^Restored into /, '').replace(/\. Your current.*$/, '');
check('restored', fs.existsSync(`${restored}/catalog.sqlite`));
fs.rmSync(info.suggested, { force: true });
fs.rmSync(restored, { recursive: true, force: true });

// 9. With suggestions off, Home lists only the placeholders.
await page.uncheck('#s-tips');
await page.click('#settings-form button[type=submit]');
await page.waitForTimeout(300);
await home();
results.toFinishOff = await page.$$eval('#to-finish li', (l) => l.map((x) => x.textContent.replace(/\s+/g, ' ').trim()));
check('suggestionsOffHidesCouldAdd', results.toFinishOff.length > 0 && results.toFinishOff.every((t) => t.includes('Needs a photo')));

console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /(light|dark)$/.test(k) && v.length);
process.exit(errors.length || bad.length ? 1 : 0);
