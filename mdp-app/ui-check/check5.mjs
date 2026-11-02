// Screen check for sprint 5: the storyteller screen, recordings in a memory
// and in Waiting, the transcript editor, the large preview and Settings >
// Speech to text, driven against the real core (cargo build --example
// ui_server, which stands in for the microphone and the speech model), with
// axe in light and dark.
// Usage: npm i playwright axe-core; node check5.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8767'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
await page.goto('http://127.0.0.1:8767/');
await page.waitForSelector('#journey-title:not(:empty)');
await axe('0-home');

// Home: Record a voice opens the storyteller screen; the recording waits.
const waitingBefore = (await page.evaluate(() => window.__TAURI__.core.invoke('raw_content', { kind: 'waiting' }))).length;
await page.click('#home-record');
await page.waitForSelector('dialog.storyteller[open]');
await axe('1-storyteller');
await page.click('dialog.storyteller .rec');
await page.waitForSelector('dialog.storyteller .rec[aria-pressed="true"]');
await page.waitForTimeout(1600);
results.timerWhileRecording = await page.textContent('dialog.storyteller .timer');
results.micLine = await page.textContent('dialog.storyteller .mic');
await axe('2-storyteller-recording');
await page.click('dialog.storyteller .rec');
await page.waitForSelector('dialog.storyteller .rec[aria-pressed="false"]');
results.savedLine = await page.textContent('dialog.storyteller .saved');
await page.click('dialog.storyteller .finish');
await page.waitForSelector('dialog.confirm[open]');
results.confirmText = await page.textContent('dialog.confirm');
await axe('3-done-recording');
await page.click('dialog.confirm button:has-text("Save")');
await page.waitForSelector('dialog.storyteller', { state: 'detached' });
const waiting = await page.evaluate(() => window.__TAURI__.core.invoke('raw_content', { kind: 'waiting' }));
results.waitingGrewBy = waiting.length - waitingBefore;
results.newWaitingName = waiting[0].name;

// A memory: record into it, then check the transcript.
const mem = (await page.evaluate(() => window.__TAURI__.core.invoke('memories')))[0];
results.memory = mem.name;
await page.evaluate((id) => go('memory', { id }), mem.id);
await page.waitForSelector('#h-memory');
await page.click('#memory-body button.slot:has-text("Record someone telling it")');
await page.waitForSelector('dialog.storyteller[open]');
results.storytellerTitle = await page.textContent('#st-title');
results.coverShown = await page.$eval('dialog.storyteller img', (i) => !i.hidden);
await page.click('dialog.storyteller .rec');
await page.waitForSelector('dialog.storyteller .rec[aria-pressed="true"]');
await page.waitForTimeout(1200);
// Finish while still recording: it asks, then stops and saves.
await page.click('dialog.storyteller .finish');
await page.waitForSelector('dialog.confirm[open]');
await page.click('dialog.confirm button:has-text("Save")');
await page.waitForSelector('dialog.storyteller', { state: 'detached' });
await page.waitForSelector('#memory-body .transcript-line .chip');
results.transcriptChip = await page.textContent('#memory-body .transcript-line .chip');
await axe('4-memory-with-recording');

await page.click('#memory-body .transcript-line button:has-text("Check the transcript")');
await page.waitForSelector('dialog.ask[open] ol.tlines');
results.editorLines = await page.$$eval('dialog.ask[open] ol.tlines li', (l) => l.length);
results.playButtons = await page.$$eval('dialog.ask[open] ol.tlines button', (l) => l.map((b) => b.textContent));
await axe('5-transcript-editor');
await page.fill('dialog.ask[open] ol.tlines li:first-child textarea', 'This is a corrected transcript line.');
await page.click('dialog.ask[open] button[type=submit]');
await page.waitForSelector('dialog.ask[open]', { state: 'detached' });
await page.waitForSelector('#memory-body .transcript-line .chip:has-text("checked")');
results.afterCorrect = await page.textContent('#memory-body .transcript-line .chip');
const rec = (await page.evaluate((id) => window.__TAURI__.core.invoke('memory_view', { id }), mem.id)).items.find((i) => i.kind === 'voice recording' && i.transcript);
const t = await page.evaluate((id) => window.__TAURI__.core.invoke('transcript', { id }), rec.id);
results.correctedVersion = t.version;
results.correctedText = t.text;

// Ctrl+Z goes back to the automatic transcript.
await page.click('#h-memory');
await page.keyboard.press('Control+z');
await page.waitForSelector('#memory-body .transcript-line .chip:has-text("automatically")');
results.afterUndo = await page.textContent('#memory-body .transcript-line .chip');

// The large preview shows the transcript under the player.
const idx = (await page.evaluate((id) => window.__TAURI__.core.invoke('memory_view', { id }), mem.id)).items.findIndex((i) => i.id === rec.id);
await (await page.$$('#memory-body ul.items li .thumb'))[idx].click();
await page.waitForSelector('dialog.viewer[open] .vtranscript');
results.previewTranscript = (await page.textContent('dialog.viewer[open] .vtranscript pre')).slice(0, 60);
await axe('6-large-preview-transcript');
await page.keyboard.press('Escape');

// With transcripts made on demand: "Make a transcript" shows what it is doing,
// a transcript that couldn't be made says why on the recording, and Try again.
await page.evaluate(() => window.__TAURI__.core.invoke('set_auto_transcribe', { on: false }));
await page.click('#memory-body button.slot:has-text("Record someone telling it")').catch(() => {});
if (!(await page.$('dialog.storyteller[open]'))) await page.evaluate((m) => openStoryteller({ memory: m.id, title: m.name }), mem);
await page.waitForSelector('dialog.storyteller[open]');
await page.click('dialog.storyteller .rec');
await page.waitForSelector('dialog.storyteller .rec[aria-pressed="true"]');
await page.waitForTimeout(1200);
await page.click('dialog.storyteller .finish');
await page.click('dialog.confirm button:has-text("Save")');
await page.waitForSelector('dialog.storyteller', { state: 'detached' });
await page.click('#memory-body .transcript-line button:has-text("Make a transcript")');
await page.waitForSelector('#memory-body .transcript-line [role=status]:has-text("Listening")');
results.progress = await page.textContent('#memory-body .transcript-line [role=status]:has-text("Listening")');
await page.waitForSelector('#memory-body .transcript-line [role=alert]', { timeout: 8000 });
results.failure = await page.textContent('#memory-body .transcript-line [role=alert]');
await axe('7-transcript-not-made');
await page.click('#memory-body .transcript-line button:has-text("Try again")');
await page.waitForFunction(() => document.querySelectorAll('#memory-body .transcript-line .chip').length === 2, null, { timeout: 8000 });
results.afterTryAgain = await page.$$eval('#memory-body .transcript-line .chip', (c) => c.map((x) => x.textContent));

// Settings > Speech to text
await page.click('button.navbtn[data-go="settings"]');
await page.waitForSelector('#speech fieldset');
results.speech = (await page.textContent('#speech')).replace(/\s+/g, ' ').slice(0, 220);
await axe('8-settings-speech');

fs.writeFileSync(`${out}/results5.json`, JSON.stringify({ results, errors }, null, 2));
console.log(JSON.stringify({ results, errors }, null, 2));
await browser.close();
server.kill();
