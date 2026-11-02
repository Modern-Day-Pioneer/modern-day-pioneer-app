// Screen check for the sprint 5 fixes, second pass (feedback round 2): name
// lists that open on click, the chosen name shown in the box, a waiting
// picture put into a new memory in a story (and shown there), "open this
// memory or carry on", Done on the assign dialog, the More menu on a memory's
// items, Esc, and the person page's stories and add buttons.
// Usage as check5.mjs: node check5g.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8769'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 1400 } });
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
page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8769/');
await page.waitForSelector('#journey-title:not(:empty)');
// Sprint 6 put the sample's cat picture into the Sunday dinners memory; these
// steps were written when it was still waiting, so take it back out first.
{
  const inv = (c, a) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [c, a]);
  const dm = (await inv('browse', { kinds: ['memory'] })).find((h) => h.name.startsWith('Sunday dinners'));
  for (const it of (await inv('memory_view', { id: dm.id })).items) await inv('remove_from_memory', { memory: dm.id, content: it.id });
  await page.reload();
  await page.waitForSelector('#journey-title:not(:empty)');
}
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };

// 1. A waiting picture into a new memory, in a story, then open it.
await page.click('button.navbtn[data-go="waiting"]');
const pic = (await call('raw_content', { kind: 'waiting' })).find((w) => w.kind === 'picture');
await page.click(`#waiting-list li:has-text("${pic.name}") button:has-text("Make a memory")`);
await page.waitForSelector('dialog.ask[open]');
await page.getByLabel('Title of the new memory').fill('Pepper at the table');
await page.getByLabel('In a story (optional)').selectOption({ label: 'The Kitchen Table' });
await axe('g1-put-into-story');
await page.click('dialog.ask[open] button[type=submit]');
await page.waitForSelector('dialog.ask[open]:has-text("The memory is ready")');
results.nextButtons = await page.$$eval('dialog.ask[open] button', (b) => b.map((x) => x.textContent));
check('backToWaitingOffered', results.nextButtons.includes('Back to Waiting'));
await axe('g2-what-next');
await page.click('dialog.ask[open] button:has-text("Open this memory")');
await page.waitForSelector('#h-memory:text("Pepper at the table")');
results.memoryInStory = await page.textContent('#memory-body');
check('memoryNamesItsStory', results.memoryInStory.includes('The Kitchen Table'));

// The picture item: Show large plus a More menu.
results.pictureButtons = await page.$$eval('#memory-body ul.items li:first-child .btns > button, #memory-body ul.items li:first-child .btns .menuwrap > button', (b) => b.map((x) => x.textContent.trim()));
await page.click('#memory-body ul.items li:first-child button:has-text("More")');
results.pictureMenu = await page.$$eval('#memory-body ul.items li:first-child [role=menuitem]', (b) => b.map((x) => x.textContent));
check('moreMenuHasTakeOut', results.pictureMenu.includes('Take out of this memory'));
await axe('g3-memory-more-menu');
// Esc closes the menu first and stays on the memory.
await page.keyboard.press('Escape');
check('escClosesMenu', await page.$eval('#memory-body ul.items li:first-child .menu', (m) => m.hidden));
check('escStaysOnMemory', await page.isVisible('#h-memory'));

// 2. The Who list opens on click, before typing; the chosen name stays in the box.
await page.click('#memory-body form[aria-label="Add a person, pet or family"] input[role=combobox]');
await page.waitForSelector('#memory-body form[aria-label="Add a person, pet or family"] [role=option]');
results.whoListBeforeTyping = await page.$$eval('#memory-body form[aria-label="Add a person, pet or family"] [role=option] span', (o) => o.map((x) => x.textContent));
check('whoListShowsEveryone', results.whoListBeforeTyping.includes('Ellen Rowan') && results.whoListBeforeTyping.includes('The Rowans'));
await axe('g4-who-list-open');
await page.click('#memory-body form[aria-label="Add a person, pet or family"] [role=option]:has-text("Pepper")');
results.boxAfterChoosing = await page.inputValue('#memory-body form[aria-label="Add a person, pet or family"] input[role=combobox]');
check('chosenNameInBox', results.boxAfterChoosing === 'Pepper');
await axe('g5-who-chosen');
await page.click('#memory-body form[aria-label="Add a person, pet or family"] button[type=submit]');
await page.waitForSelector('#memory-body .lines .link:text("Pepper")');
results.linked = true;

// 3. The story screen shows the memory with its picture.
await page.click('#memory-body button.link:text("The Kitchen Table")').catch(async () => {
  await page.evaluate(async () => go('story', { id: (await window.__TAURI__.core.invoke('stories')).find((s) => s.name === 'The Kitchen Table').id }));
});
await page.waitForSelector('#h-story:text("The Kitchen Table")');
// (ui_server has no thumbnails, so check the picture asked for.)
await page.waitForSelector('#story-body .lines li:has-text("Pepper at the table") .thumb');
const kitchen = (await call('stories')).find((s) => s.name === 'The Kitchen Table').id;
check('storyShowsPicture', (await call('story_view', { id: kitchen })).memories.find((m) => m.name === 'Pepper at the table').cover_id === pic.id);
await axe('g6-story-pictures');

// 4. Esc goes back from an inner screen, and Home from a main one.
await page.click('#h-story');
await page.keyboard.press('Escape');
await page.waitForSelector('#h-memory:text("Pepper at the table")');
results.escBackToMemory = true;
await page.click('button.navbtn[data-go="stories"]');
await page.click('h1:visible');
await page.keyboard.press('Escape');
await page.waitForSelector('.pane[data-pane="home"]:not([hidden])');
results.escHomeFromStories = true;

// 5. Assign from Waiting: the list opens on click, and Done finishes.
await page.click('button.navbtn[data-go="waiting"]');
await page.waitForSelector('#waiting-list input[type=checkbox]');
await (await page.$$('#waiting-list input[type=checkbox]'))[0].check();
await page.click('#sel-person');
await page.fill('dialog.ask[open] input', 'Walter Rowan');
results.assignButtons = await page.$$eval('dialog.ask[open] .btns button', (b) => b.map((x) => x.textContent));
check('assignHasDone', results.assignButtons[0] === 'Done');
await axe('g7-assign-done');
await page.click('dialog.ask[open] button:text("Done")');
await page.click('dialog.ask[open]:has-text("The memory is ready") button:has-text("Back to Waiting")');
await page.waitForSelector('dialog.ask[open]', { state: 'detached' });
check('backOnWaiting', await page.isVisible('.pane[data-pane="waiting"]'));

// 6. Walter's page: his stories, the links, Add from waiting and Link a memory.
const walter = (await call('browse', { kinds: ['person'] })).find((h) => h.name === 'Walter Rowan').id;
await page.evaluate((id) => go('person', { id }), walter);
await page.waitForSelector('#h-person:text("Walter Rowan")');
results.walterStories = await page.$$eval('#person-body section:has(h2:text("Stories")) li', (l) => l.map((x) => x.textContent));
check('walterHasHisStory', results.walterStories.some((s) => s.includes("Walter's Camera")));
results.memoryButtons = await page.$$eval('#person-body section:has(h2:text-is("Memories")) .btns button', (b) => b.map((x) => x.textContent));
check('personAddButtons', results.memoryButtons.join('|') === 'Add from waiting…|Go to Waiting|Go to Stories');
const before = (await call('individual_view', { id: walter })).memories.length;
await page.click('#person-body form[aria-label="Link a memory"] input[role=combobox]');
await page.click('#person-body form[aria-label="Link a memory"] [role=option]:has-text("Pepper at the table")');
await page.waitForSelector('#person-body form[aria-label="Link a memory"] select');
await axe('g8-person-page');
await page.click('#person-body form[aria-label="Link a memory"] button[type=submit]');
await page.waitForFunction(([id, n]) => window.__TAURI__.core.invoke('individual_view', { id }).then((v) => v.memories.length === n + 1), [walter, before]);
results.linkedFromPerson = true;
const waitingLeft = (await call('raw_content', { kind: 'waiting' })).length;
if (waitingLeft) {
  await page.click('#person-body button:has-text("Add from waiting")');
  await page.waitForSelector('dialog.ask[open] ul.pick input');
  await page.check('dialog.ask[open] ul.pick input');
  await page.click('dialog.ask[open] button:text("Done")');
  await page.click('dialog.ask[open]:has-text("The memory is ready") button:has-text("Stay on this page")');
  await page.waitForFunction(([id, n]) => window.__TAURI__.core.invoke('individual_view', { id }).then((v) => v.memories.length === n + 2), [walter, before]);
  results.addedFromWaitingOnPerson = true;
}
await page.click('#person-body button:has-text("Go to Stories")');
await page.waitForSelector('.pane[data-pane="stories"]:not([hidden])');
results.goToStories = true;

fs.writeFileSync(`${out}/results5g.json`, JSON.stringify({ results, errors }, null, 2));
console.log(JSON.stringify({ results, errors }, null, 2));
await browser.close();
server.kill();
