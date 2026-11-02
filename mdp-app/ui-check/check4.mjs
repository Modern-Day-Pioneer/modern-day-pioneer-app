// Screen check for sprint 4: memories, roles, stories, the Journey and the
// Waiting screen, driven against the real core (cargo build --example
// ui_server), with axe in light and dark.
// Usage: npm i playwright axe-core; node check4.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8766'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
const page = await browser.newPage({ viewport: { width: 1280, height: 1800 } });
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
await page.goto('http://127.0.0.1:8766/');
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
await axe('0-home');

// Journey title and introduction
await page.click('#edit-journey');
await page.getByLabel('Title', { exact: true }).fill('The Rowan Family');
await page.click('dialog.ask[open] button[type=submit]');
await page.waitForSelector('#journey-title:text("The Rowan Family")');
results.journeyRenamed = true;

// Waiting: tick two items and put them into a new memory
await page.click('button.navbtn[data-go="waiting"]');
await page.waitForSelector('#waiting-list li');
results.waitingCount = await page.$$eval('#waiting-list li', (l) => l.length);
await axe('1-waiting');
// Large preview of the first item, then the next with the arrow key
await page.click('#waiting-list li:first-child .t .link');
await page.waitForSelector('dialog.viewer[open]');
results.viewerFirst = await page.textContent('#viewer-h');
results.viewerHasMedia = await page.$eval('dialog.viewer .stage', (s) => s.querySelector('img, audio, video, pre')?.tagName || 'none');
await axe('2-large-preview');
await page.keyboard.press('ArrowRight');
await page.waitForTimeout(300);
results.viewerSecond = await page.textContent('#viewer-h');
results.viewerMediaLoaded = await page.$eval('dialog.viewer .stage', async (s) => {
  const m = s.querySelector('img, audio');
  if (!m) return 'none';
  if (m.tagName === 'IMG') return m.complete && m.naturalWidth > 0 ? 'picture shown' : 'picture missing';
  await new Promise((r) => { if (m.readyState > 0) r(); else { m.onloadedmetadata = r; m.onerror = r; setTimeout(r, 2000); } });
  return m.readyState > 0 ? 'recording ready' : `recording not loaded (${m.error?.code})`;
});
await page.keyboard.press('Escape');
await page.waitForSelector('dialog.viewer:not([open])', { state: 'attached' });
const ticks = await page.$$('#waiting-list input[type=checkbox]');
await ticks[0].check();
await ticks[1].check();
results.tickedLine = await page.textContent('#sel-count');
await page.click('#sel-memory');
await page.waitForSelector('dialog.ask[open]');
await axe('3-put-into-memory');
await page.getByLabel('Title of the new memory').fill('Pepper and the coffee cup');
await page.click('dialog.ask[open] button[type=submit]');
// Round 2: the app then asks whether to open the memory or carry on.
await page.click('dialog.ask[open]:has-text("The memory is ready") button:has-text("Back to")');
await page.waitForSelector('dialog.ask[open]', { state: 'detached' });
results.waitingAfterPut = await page.$$eval('#waiting-list li', (l) => l.length).catch(() => 0);

// Stories screen lists the new memory as not in a story
await page.click('button.navbtn[data-go="stories"]');
await page.waitForSelector('#loose-memories >> text=Pepper and the coffee cup');
await axe('4-stories');
await page.click('#loose-memories button.link:has-text("Pepper and the coffee cup")');
await page.waitForSelector('#h-memory:text("Pepper and the coffee cup")');
results.memoryItems = await page.$$eval('#memory-body .items li', (l) => l.length);
// The picture (second item) is the cover, since a recording never is
// (sprint 5); then date and caption.
await page.waitForSelector('#memory-body .items li:nth-child(2) .chip:text("Cover")');
await page.getByLabel('Date', { exact: true }).fill('1970s');
await page.waitForSelector('text=Understood as the 1970s.');
await page.getByLabel('Caption').fill('Pepper waited under the table every morning. '.repeat(8));
await page.click('#memory-body form.form button[type=submit]');
await page.waitForSelector('text=This caption is long.');
results.longCaptionOffered = true;
await page.click('button:has-text("Make it a typed note")');
await page.waitForSelector('#memory-body .items li:nth-child(3)');
results.itemsAfterNote = await page.$$eval('#memory-body .items li', (l) => l.length);
// A role: Pepper, in the picture; Walter, other with a note
await page.getByRole('combobox', { name: 'Who', exact: true }).fill('Pepper');
await page.waitForSelector('#memory-body [role=option]');
await page.keyboard.press('Enter');
await page.click('form[aria-label="Add a person, pet or family"] button[type=submit]');
await page.waitForSelector('#memory-body .lines >> text=Pepper');
await page.getByRole('combobox', { name: 'Who', exact: true }).fill('Walter');
await page.waitForSelector('#memory-body [role=option]');
await page.keyboard.press('Enter');
await page.selectOption('form[aria-label="Add a person, pet or family"] select', 'other');
await page.click('form[aria-label="Add a person, pet or family"] button[type=submit]');
await page.waitForTimeout(300);
results.otherNeedsNote = await page.textContent('#toast');
await page.getByLabel('What "other" means here').last().fill('poured the coffee');
await page.click('form[aria-label="Add a person, pet or family"] button[type=submit]');
await page.waitForSelector('#memory-body .lines >> text=poured the coffee');
// Add the memory to a story from here
await page.getByRole('combobox', { name: 'Add to a story' }).fill('The Kitchen');
await page.waitForSelector('#memory-body [role=option]:has-text("The Kitchen Table")');
await page.click('#memory-body [role=option]:has-text("The Kitchen Table")');
await page.waitForSelector('#memory-body .lines >> text=The Kitchen Table');
await axe('5-memory');

// Story screen: order, focus, featured
await page.click('#memory-body .lines button.link:has-text("The Kitchen Table")');
await page.waitForSelector('#h-story:text("The Kitchen Table")');
results.storyOrderBefore = await page.$$eval('#story-body .lines li .t .link', (l) => l.map((x) => x.textContent));
await page.click('#story-body .lines li:last-child button:has-text("Move up")');
await page.waitForTimeout(300);
results.storyOrderAfter = await page.$$eval('#story-body .lines li .t .link', (l) => l.map((x) => x.textContent));
await axe('6-story');

// A new story from the home page
await page.click('button.navbtn[data-go="home"]');
await page.click('#home-story');
await page.getByLabel('Title, for example "The Kitchen Table"').fill('June\'s Scrapbook');
await page.click('dialog.ask[open] button[type=submit]');
await page.waitForSelector('#h-story:text("June\'s Scrapbook")');
await page.getByLabel('Who it is about (optional)').selectOption({ label: 'June Rowan' });
await page.click('#story-body form.form button[type=submit]');
await page.waitForSelector('#story-body .lede:text("Story, about June Rowan")');
await page.click('#story-body button:has-text("New memory in this story")');
await page.getByLabel('Title, for example "Walter and June\'s wedding day"').fill('The green scrapbook');
await page.click('dialog.ask[open] button[type=submit]');
await page.waitForSelector('#h-memory:text("The green scrapbook")');
results.placeholderWarning = await page.isVisible('text=This memory needs a photo, recording or note');
// Featured switch and home order
await page.check('#memory-body label.switch:has-text("Feature on the home page") input');
await page.waitForTimeout(300);
await page.click('button.navbtn[data-go="home"]');
await page.waitForSelector('#featured li');
results.featuredBefore = await page.$$eval('#featured li .link', (l) => l.map((x) => x.textContent));
await page.click('#featured li:last-child button:has-text("Up")');
await page.waitForTimeout(300);
results.featuredAfter = await page.$$eval('#featured li .link', (l) => l.map((x) => x.textContent));
results.toFinish = await page.$$eval('#to-finish li .link', (l) => l.map((x) => x.textContent));
await axe('7-home-after');

// Assign a waiting item to a person
await page.click('button.navbtn[data-go="waiting"]');
// Everything waiting went into the memory above, so bring the set-aside coffee back first.
await page.click('.tab[data-state="set aside"]');
await page.click('#waiting-list button:has-text("Back to waiting")');
await page.click('.tab[data-state="waiting"]');
await page.waitForSelector('#waiting-list input[type=checkbox]');
await (await page.$$('#waiting-list input[type=checkbox]'))[0].check();
await page.click('#sel-person');
await page.fill('dialog.ask[open] input', 'Ellen Rowan');
await page.click('dialog.ask[open] button[type=submit]');
// Round 2: the app then asks whether to open the memory or carry on.
await page.click('dialog.ask[open]:has-text("The memory is ready") button:has-text("Back to")');
await page.waitForSelector('dialog.ask[open]', { state: 'detached' });
results.assigned = (await page.evaluate(() => window.__TAURI__.core.invoke('memories'))).filter((m) => m.name.startsWith('Ellen Rowan:')).map((m) => m.name);

// Remove a memory with the warning
await page.click('button.navbtn[data-go="stories"]');
await page.click('#loose-memories button.link:has-text("Ellen Rowan:")');
await page.waitForSelector('#h-memory');
await page.click('#memory-body .btn.danger');
await page.waitForTimeout(400);
results.removeMemoryDialog = await page.evaluate(() => window.lastDialog);
results.waitingAfterRemove = (await page.evaluate(() => window.__TAURI__.core.invoke('raw_content', { kind: 'waiting' }))).length;

fs.writeFileSync(`${out}/results4.json`, JSON.stringify({ results, errors }, null, 2));
console.log(JSON.stringify({ results, errors }, null, 2));
await browser.close();
server.kill();
