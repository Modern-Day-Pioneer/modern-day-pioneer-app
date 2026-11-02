// Screen check for sorting and filtering the Waiting screen (after sprint 8).
// Usage as check8.mjs: node check8w.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8773'], { stdio: ['ignore', 'inherit', 'inherit'] });
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
const fail = async (e) => { console.log('FAILED', String(e).slice(0, 800)); 
// More for the selected items: set aside two, then remove one.
check('moreDisabledEmpty', await page.isDisabled('#sel-more'));
await page.check('#waiting-list li:nth-child(1) input[type=checkbox]');
await page.check('#waiting-list li:nth-child(2) input[type=checkbox]');
await page.click('#sel-more');
check('menuOpens', await page.isVisible('#sel-aside') && (await page.getAttribute('#sel-more', 'aria-expanded')) === 'true');
await axe('s8w-2-more-menu');
await page.click('#sel-aside');
await page.waitForFunction((n) => document.querySelectorAll('#waiting-list li').length === n, first.length - 2);
check('setAsideTwo', (await page.textContent('#n-aside')) === '(2)');
await page.check('#waiting-list li:nth-child(1) input[type=checkbox]');
await page.click('#sel-more');
await page.click('#sel-remove');
await page.waitForFunction((n) => document.querySelectorAll('#waiting-list li').length === n, first.length - 3);
check('removedOne', (await page.evaluate(() => window.lastDialog)).startsWith('Remove '));
console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); };
process.on('unhandledRejection', fail);
process.on('uncaughtException', fail);
page.on('console', (m) => { if (m.type() === 'error' && !m.location().url.endsWith('/favicon.ico')) errors.push(m.text() + ' at ' + m.location().url); });
page.on('response', (r) => { if (r.status() >= 400) errors.push('HTTP ' + r.status() + ' ' + r.url()); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8773/');
await page.waitForSelector('#journey-title:not(:empty)');
const check = (name, ok) => { results[name] = ok; if (!ok) throw new Error(`check failed: ${name}`); };
const call = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [cmd, args]);
const nav = async (pane, h) => { await page.click(`button.navbtn[data-go="${pane}"]`); await page.waitForSelector(h); await page.waitForTimeout(250); };

// The sample has few waiting items: bring the set-aside ones back and take
// some pictures out of their memories so they wait too.
const aside = await call('raw_content', { kind: 'set aside' });
await call('set_state', { ids: aside.map((x) => x.id), to: 'waiting' });
for (const m of (await call('memory_list', {})).slice(0, 4)) {
  const pic = (await call('memory_view', { id: m.id })).items.find((i) => i.kind === 'picture');
  if (pic) await call('remove_from_memory', { memory: m.id, content: pic.id });
}
await nav('waiting', '#h-waiting');
const shown = () => page.$$eval('#waiting-list .t .link', (l) => l.map((x) => x.textContent));
const first = await shown();
results.waiting = first;
check('someWaiting', first.length >= 3);
check('noCountUnfiltered', await page.isHidden('#waiting-count'));
await page.selectOption('#wf-sort', 'old');
check('oldestFirst', (await shown()).join('|') === [...first].reverse().join('|'));
await page.selectOption('#wf-sort', 'name');
const byName = await shown();
check('sortedByName', byName.join('|') === [...byName].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' })).join('|'));
await page.selectOption('#wf-sort', 'date');
results.byDate = await page.$$eval('#waiting-list .t small', (l) => l.map((x) => x.textContent));
await page.selectOption('#wf-kind', 'picture');
const pics = await page.$$eval('#waiting-list .t small', (l) => l.map((x) => x.textContent));
check('kindFilter', pics.length > 0 && pics.length < first.length && pics.every((t) => t.startsWith('Picture')));
check('countShown', await page.isVisible('#waiting-count'));
await page.click('#sel-all');
check('selectAllOnlyShown', (await page.textContent('#sel-count')).startsWith(`${pics.length} item`));
await page.click('#sel-all');
await axe('s8w-1-filtered');
await page.selectOption('#wf-kind', '');
await page.fill('#wf-text', 'zzzz-nothing');
check('nothingMatches', (await page.textContent('#waiting-list-empty')).startsWith('Nothing matches'));
await page.fill('#wf-text', '');
check('allBack', (await shown()).length === first.length);
console.log(JSON.stringify({ results, errors }, null, 2));
server.kill();
await browser.close();
const bad = Object.entries(results).filter(([k, v]) => Array.isArray(v) && /(light|dark)$/.test(k) && v.length);
process.exit(errors.length || bad.length ? 1 : 0);
