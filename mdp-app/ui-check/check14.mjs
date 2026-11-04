// Screen check for sprint 14: the getting-started card on Home (new Journey,
// partway, all done with its thank-you, hidden and shown again from Help),
// the Help panel and the pages it opens, and a name on every icon-only
// button (axe, light and dark).
// Usage as check8.mjs: node check14.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8784'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
// Note every link the app asks to open.
await page.addInitScript(() => {
  window.opened = [];
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = (cmd, args) => { if (cmd === 'open_link') window.opened.push(args.url); return inv(cmd, args); };
});
await page.goto('http://127.0.0.1:8784/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const opened = () => page.evaluate(() => window.opened.slice());
const steps = () => page.$$eval('#getting-steps li', (l) => l.map((x) => `${x.className}|${x.querySelector('.t').textContent}`));

// The sample family already has memories, so the card starts hidden.
check('hiddenWithMemories', await page.isHidden('#getting'));

// A new, empty Journey: five steps, none done, the first marked next.
const zero = { stories: 0, memories: 0, people: 0, pets: 0, families: 0, places: 0, waiting: 0, pictures: 0, in_stories: 0 };
const fake = (counts) => page.evaluate((c) => { const h = { id: 'check14-new', read_only: false, counts: c }; renderGetting(h); }, counts);
await fake(zero);
check('newShown', await page.isVisible('#getting'));
check('newCount', (await page.textContent('#getting-count')) === '0 of 5 done');
results.newSteps = await steps();
check('newSteps', results.newSteps.join(',') === 'next|Add a person · next,|Bring in photos,|Make a memory,|Add to a story,|Preview your history');
await axe('s14-1-new');
await page.click('#getting-steps li:nth-child(1) button:has-text("Show me")');
check('showMe', (await opened()).at(-1) === 'https://moderndaypioneer.org/help/add-people/');

// Partway: a person and photos.
await fake({ ...zero, people: 1, pictures: 3 });
check('partCount', (await page.textContent('#getting-count')) === '2 of 5 done');
results.partSteps = await steps();
check('partSteps', results.partSteps[0].startsWith('done|Add a person · Done') && results.partSteps[2].startsWith('next|Make a memory'));
await axe('s14-2-partway');

// All done (preview opened): the thank-you, once, then gone.
await page.evaluate(() => markPreviewed());
await fake({ ...zero, people: 1, pictures: 3, memories: 1, in_stories: 1 });
check('thanks', await page.isVisible('#getting-done') && await page.isHidden('#getting-list'));
await axe('s14-3-done');
await page.click('#getting-gotit');
check('gotItHides', await page.isHidden('#getting'));
await fake({ ...zero, people: 1, pictures: 3, memories: 1, in_stories: 1 });
check('staysHidden', await page.isHidden('#getting'));

// Help: the panel, its pages, and the card back on the real Journey.
await page.evaluate(() => go('home'));
await page.waitForTimeout(300);
check('realStillHidden', await page.isHidden('#getting'));
await page.click('#help-open');
await page.waitForSelector('#help-dialog[open]');
await axe('s14-4-help');
await page.click('#help-dialog button:has-text("Getting started and how-to pages")');
await page.click('#help-dialog button:has-text("Videos")');
await page.click('#help-dialog button:has-text("Questions people ask")');
results.helpOpened = (await opened()).slice(-3);
check('helpLinks', results.helpOpened.join(' ') === 'https://moderndaypioneer.org/help/ https://moderndaypioneer.org/help/videos/ https://moderndaypioneer.org/faq/');
await page.click('#help-getting');
await page.waitForSelector('#getting:not([hidden])');
results.sampleSteps = await steps();
check('sampleTicks', results.sampleSteps.filter((s) => s.startsWith('done')).length === 4 && results.sampleSteps[4].startsWith('next|Preview'));
await page.click('#getting-hide');
check('hideAgain', await page.isHidden('#getting'));

// A "?" beside Waiting opens its explanation, with a link to Help.
await page.evaluate(() => go('waiting'));
await page.click('#h-waiting button.term');
await page.waitForSelector('#term-dialog[open]');
check('termText', (await page.textContent('#term-text')).includes('until they go into a memory'));
await axe('s14-5-term');
await page.click('#term-more');
check('termHelp', (await opened()).at(-1) === 'https://moderndaypioneer.org/help/make-a-memory/');
await page.click('#term-dialog button:has-text("Got it")');

// Every button has a name a screen reader can read (icon-only ones included).
results.unnamed = await page.$$eval('button', (l) => l.filter((b) => !b.hidden && !(b.getAttribute('aria-label') || b.textContent.trim() || b.title)).map((b) => b.id || b.className));
check('allNamed', results.unnamed.length === 0);

const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /light|dark/.test(k) && v.length);
console.log(JSON.stringify({ results, errors, axeProblems: bad.length }, null, 2));
server.kill();
await browser.close();
