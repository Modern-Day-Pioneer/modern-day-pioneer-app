// Export checks, the browser half. The core tests (tests/export.rs) check the
// rest by reading files; this opens every exported page of the sample family in a
// real browser and checks:
//   E1  opens with no internet (the browser is offline; nothing fails to load)
//   E2  works with scripts off (JavaScript disabled for the whole run)
//   E5  catalog.json expands as JSON-LD with the local context.jsonld (pyld)
//   E12 no page is wider than a 390-pixel phone screen
// Usage: node export-checklist.mjs [an export folder]. With no folder it
// exports the sample family through ui_server first.
import { chromium } from 'playwright';
import fs from 'fs';
import path from 'path';
import { spawn, execFileSync } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
let folder = process.argv[2];
let server = null;
if (!folder) {
  server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8772'], { stdio: ['ignore', 'inherit', 'inherit'] });
  await new Promise((r) => setTimeout(r, 2500));
  const call = async (cmd, args = {}) => (await fetch(`http://127.0.0.1:8772/invoke/${cmd}`, { method: 'POST', body: JSON.stringify(args) })).json();
  await call('start_export');
  folder = (await call('export_status')).folder;
}
const results = { folder };
const problems = [];
const pages = [];
(function walk(d) {
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p);
    else if (p.endsWith('.html')) pages.push(p);
  }
})(folder);
results.pages = pages.length;

const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const context = await browser.newContext({ offline: true, javaScriptEnabled: false, viewport: { width: 390, height: 800 } });
const page = await context.newPage();
page.on('requestfailed', (r) => problems.push(`E1 failed to load ${r.url()} (${r.failure()?.errorText})`));
page.on('request', (r) => { if (!r.url().startsWith('file://')) problems.push(`E1/E3 asks the internet for ${r.url()}`); });
let widest = 0;
for (const p of pages) {
  await page.goto(`file://${p}`);
  const broken = await page.$$eval('img', (l) => l.filter((i) => !i.complete || i.naturalWidth === 0).map((i) => i.getAttribute('src')));
  for (const b of broken) problems.push(`E1 picture does not show on ${path.relative(folder, p)}: ${b}`);
  const media = await page.$$eval('audio source, video source, audio[src], video[src]', (l) => l.map((m) => m.getAttribute('src')));
  for (const m of media) if (!fs.existsSync(path.resolve(path.dirname(p), decodeURIComponent(m.split('#')[0])))) problems.push(`E1 missing media on ${path.relative(folder, p)}: ${m}`);
  const text = (await page.textContent('body')).trim();
  if (text.length < 20) problems.push(`E2 ${path.relative(folder, p)} has no readable text with scripts off`);
  const w = await page.evaluate(() => document.documentElement.scrollWidth);
  widest = Math.max(widest, w);
  if (w > 390) problems.push(`E12 ${path.relative(folder, p)} is ${w} px wide at 390 px`);
}
results.widest = widest;
await browser.close();

// E5: expand the catalog with pyld, loading only the local context.
const py = `
import json, sys, os
from pyld import jsonld
folder = sys.argv[1]
ctx = json.load(open(os.path.join(folder, 'context.jsonld'), encoding='utf-8'))
def loader(url, options=None):
    return {'contextUrl': None, 'documentUrl': url, 'document': ctx}
jsonld.set_document_loader(loader)
cat = json.load(open(os.path.join(folder, 'catalog.json'), encoding='utf-8'))
out = jsonld.expand(cat, {'base': 'file://' + os.path.join(folder, 'catalog.json')})
def ids(x):
    if isinstance(x, dict):
        return (1 if '@id' in x and len(x) > 1 else 0) + sum(ids(v) for v in x.values())
    if isinstance(x, list):
        return sum(ids(v) for v in x)
    return 0
print(json.dumps({'nodes': ids(out), 'formatVersion': cat.get('formatVersion') or cat.get('mdp:formatVersion')}))
`;
try {
  results.e5 = JSON.parse(execFileSync('python3', ['-I', '-c', py, folder], { encoding: 'utf8' }));
  if (!results.e5.nodes) problems.push('E5 the catalog expanded to nothing');
} catch (e) { problems.push(`E5 the catalog did not expand: ${String(e.stderr || e).slice(0, 400)}`); }

results.problems = problems;
console.log(JSON.stringify(results, null, 2));
server?.kill();
process.exit(problems.length ? 1 : 0);
