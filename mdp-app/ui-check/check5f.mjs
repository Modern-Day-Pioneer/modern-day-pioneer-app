// Screen check for the sprint 5 fixes: what to do with a finished recording
// (save, listen and review, delete), transcripts typed, deleted and made
// again, the progress bar on the memory screen, the cover only for pictures,
// waiting items on Home, and "Make a memory…" from one waiting item.
// Usage as check5.mjs: node check5f.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8768'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
await page.goto('http://127.0.0.1:8768/');
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

// Round 2: the less used transcript actions sit in the item's More menu.
async function more(text) {
  await page.click('#memory-body .transcript-line button:has-text("More")');
  await page.click(`#memory-body .transcript-line [role=menuitem]:has-text("${text}")`);
}

async function record(ms = 1200) {
  await page.click('dialog.storyteller .rec');
  await page.waitForSelector('dialog.storyteller .rec[aria-pressed="true"]');
  await page.waitForTimeout(ms);
}

// Home: a short list of waiting items.
await page.waitForSelector('#waiting-short li');
results.homeWaiting = await page.$$eval('#waiting-short li .t .link', (l) => l.map((x) => x.textContent));
await axe('f1-home-waiting');

// Finish while recording, then Delete it: nothing new is kept.
const before = (await call('raw_content', { kind: 'waiting' })).length;
await page.click('#home-record');
await page.waitForSelector('dialog.storyteller[open]');
await record();
await page.click('dialog.storyteller .finish');
await page.waitForSelector('dialog.confirm[open]');
results.askWhileRecording = await page.textContent('dialog.confirm');
await page.click('dialog.confirm button:has-text("Delete it")');
await page.waitForSelector('dialog.storyteller', { state: 'detached' });
results.deletedKeepsNothing = (await call('raw_content', { kind: 'waiting' })).length === before;

// Two recordings, Listen and review, delete one, save the other.
await page.click('#home-record');
await page.waitForSelector('dialog.storyteller[open]');
await record();
await page.click('dialog.storyteller .rec');
await page.waitForSelector('dialog.storyteller .rec[aria-pressed="false"]');
await record();
await page.click('dialog.storyteller .rec');
await page.waitForSelector('dialog.storyteller .rec[aria-pressed="false"]');
await page.click('dialog.storyteller .finish');
await page.waitForSelector('dialog.confirm[open]');
results.askTwo = await page.textContent('#st-q');
await page.click('dialog.confirm button:has-text("Listen and review")');
await page.waitForSelector('dialog.ask[open] audio');
await page.waitForFunction(() => document.querySelectorAll('dialog.ask[open] audio').length === 2);
await axe('f2-review');
await page.click('dialog.ask[open] li:first-child button:has-text("Delete this one")');
await page.waitForSelector('dialog.ask[open] [role=status]:not(:empty)');
results.reviewStatus = await page.textContent('dialog.ask[open] [role=status]');
await page.click('dialog.ask[open] button:has-text("Save and close")');
await page.waitForSelector('dialog.storyteller', { state: 'detached' });
results.reviewKeptOne = (await call('raw_content', { kind: 'waiting' })).length === before + 1;

// Waiting: "Make a memory…" from one item.
await page.click('button.navbtn[data-go="waiting"]');
await page.waitForSelector('#waiting-list li');
results.waitingButtons = await page.$$eval('#waiting-list li:first-child button', (b) => b.map((x) => x.textContent));
await axe('f3-waiting');
const first = await page.textContent('#waiting-list li:first-child .t .link');
await page.click('#waiting-list li:first-child button:has-text("Make a memory")');
await page.waitForSelector('dialog.ask[open]');
await page.click('dialog.ask[open] button[type=submit]');
await page.click('dialog.ask[open]:has-text("The memory is ready") button:has-text("Back to")');
await page.waitForSelector('dialog.ask[open]', { state: 'detached' });
results.memoryMadeFrom = first;
results.memoryExists = (await call('memories')).some((m) => first.startsWith(m.name));

// A memory with a picture and a recording: the cover is the picture, and the
// recording offers no "Make this the cover".
const waiting = await call('raw_content', { kind: 'waiting' });
const pic = waiting.find((w) => w.kind === 'picture');
const mid = await call('add_memory', { name: 'Cover check', story: null });
await call('add_to_memory', { memory: mid, content: [pic.id] });
await call('set_auto_transcribe', { on: false });
await page.evaluate((id) => go('memory', { id }), mid);
await page.waitForSelector('#h-memory');
await page.click('#memory-body button.slot:has-text("Record someone telling it")');
await page.waitForSelector('dialog.storyteller[open]');
await record();
await page.click('dialog.storyteller .finish');
await page.click('dialog.confirm button:has-text("Save it")');
await page.waitForSelector('dialog.storyteller', { state: 'detached' });
await page.waitForSelector('#memory-body .transcript-line [role=menuitem]:has-text("Type it myself")', { state: 'attached' });
const view = await call('memory_view', { id: mid });
results.coverIsPicture = view.items.find((i) => i.cover)?.kind;
results.recordingButtons = await page.$$eval('#memory-body ul.items li:nth-child(2) .btns button', (b) => b.map((x) => x.textContent));

// Progress bar at the top while the transcript is made.
await page.click('#memory-body .transcript-line button:has-text("Make a transcript")');
await page.waitForSelector('#memory-body > .job:not([hidden]) progress');
results.progressBar = await page.textContent('#memory-body > .job');
await axe('f4-memory-progress');
await page.waitForSelector('#memory-body .transcript-line [role=alert]', { timeout: 8000 });

// Type it myself: "Typed by hand", then delete it, then undo.
await more('Type it myself');
await page.waitForSelector('dialog.ask[open] textarea');
await page.fill('dialog.ask[open] textarea', 'We got the camera in 1958.\n\nJune took it over.');
await page.click('dialog.ask[open] button[type=submit]');
await page.waitForSelector('#memory-body .transcript-line .chip:has-text("typed by hand")');
results.typedChip = await page.textContent('#memory-body .transcript-line .chip');
results.typedButtons = await page.$$eval('#memory-body .transcript-line button', (b) => b.map((x) => x.textContent));
await axe('f5-typed-transcript');
await more('Delete the transcript');
await page.waitForSelector('#memory-body .transcript-line [role=menuitem]:has-text("Type it myself")', { state: 'attached' });
results.afterDelete = await page.textContent('#memory-body .transcript-line');
await page.click('#h-memory');
await page.keyboard.press('Control+z');
await page.waitForSelector('#memory-body .transcript-line .chip:has-text("typed by hand")');
results.undoBringsBack = true;

// Delete from the transcript editor.
await page.click('#memory-body .transcript-line button:has-text("Read or correct")');
await page.waitForSelector('dialog.ask[open] ol.tlines');
results.editorHead = await page.textContent('dialog.ask[open] p.quiet');
await page.click('dialog.ask[open] button:has-text("Delete the transcript")');
await page.waitForSelector('dialog.ask[open]', { state: 'detached' });
await page.waitForSelector('#memory-body .transcript-line [role=menuitem]:has-text("Type it myself")', { state: 'attached' });
results.deletedFromEditor = true;

// Settings: a model not downloaded can't be chosen; it offers Download.
await page.click('button.navbtn[data-go="settings"]');
await page.waitForSelector('#speech fieldset');
results.modelRadios = await page.$$eval('#speech fieldset input[type=radio]', (r) => r.map((x) => `${x.value} ${x.disabled ? 'disabled' : 'enabled'}${x.checked ? ' chosen' : ''}`));
await axe('f6-settings');

fs.writeFileSync(`${out}/results5f.json`, JSON.stringify({ results, errors }, null, 2));
console.log(JSON.stringify({ results, errors }, null, 2));
await browser.close();
server.kill();
