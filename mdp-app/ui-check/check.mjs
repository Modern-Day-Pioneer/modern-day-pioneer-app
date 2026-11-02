// Screen check for sprint 3: drives the real screens against the real core
// (cargo run --example ui_server) and runs axe in light and dark.
// Usage: npm i playwright axe-core; node check.mjs <folder for screenshots>
import { chromium } from 'playwright';
import fs from 'fs';
import { spawn } from 'child_process';
const repo = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const server = spawn(`${repo}/app/mdp-core/target/debug/examples/ui_server`, ['8765'], { stdio: ['ignore', 'inherit', 'inherit'] });
await new Promise((r) => setTimeout(r, 2500));
const out = process.argv[2] || '.';
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const errors = [];
const axeSrc = fs.readFileSync(new URL('./node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const stub = fs.readFileSync(`${repo}/app/mdp-app/ui-check/tauri-stub.js`, 'utf8');
const results = {};
async function axe(page, name) {
  for (const scheme of ['light', 'dark']) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.waitForTimeout(150);
    await page.evaluate(axeSrc);
    const r = await page.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'best-practice'] })).violations.map((v) => `${v.id}: ${v.nodes.length} (${v.nodes.slice(0, 2).map((n) => n.target.join(' ')).join(' | ')})`));
    results[`${name} ${scheme}`] = r;
  }
  await page.emulateMedia({ colorScheme: 'light' });
  await page.screenshot({ path: `${out}/${name}.png`, fullPage: true });
}
const page = await browser.newPage({ viewport: { width: 1280, height: 2000 } });
page.setDefaultTimeout(8000);
// Any failed step stops the check and saves fail.png.
process.on('unhandledRejection', async (e) => { console.log('FAILED', String(e).slice(0, 600)); console.log(JSON.stringify({ results, errors }, null, 2)); await page.screenshot({ path: `${out}/fail.png`, fullPage: true }).catch(() => {}); server.kill(); process.exit(1); });
page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(stub);
await page.goto('http://127.0.0.1:8765/');
await page.waitForSelector('#journey-title:not(:empty)');
// Home: "Add family members" opens People with the name box focused.
await page.click('#home-people');
await page.waitForSelector('#family-list .person');
results.homeButtonAsksForName = await page.evaluate(() => document.querySelector('#add-dialog').open && document.activeElement?.id === 'add-name');
await axe(page, '0-add-dialog');
await page.click('#add-cancel');
await axe(page, '1-people');
// Quick-add a person and fill in her details
// The button needs nothing typed first; it asks for the name.
await page.click('#add-person-btn');
await page.click('#add-dialog button[type=submit]');
results.emptyNameMessage = await page.textContent('#add-error');
await page.fill('#add-name', 'Ada Hale');
await page.click('#add-dialog button[type=submit]');
await page.waitForSelector('#h-person:text("Ada Hale")');
await page.getByLabel('Born').fill('about 1905');
await page.waitForSelector('text=Understood as about 1905.');
await page.getByLabel('Gender (used only to word relationships)').selectOption('female');
await page.click('#person-body form.form button[type=submit]');
await page.waitForSelector('text=Place of birth: not given yet');
// Relationship: Ada is the parent of June
await page.getByLabel('Relationship', { exact: true }).selectOption('parent');
await page.getByRole('combobox', { name: 'Who', exact: true }).fill('June');
await page.waitForSelector('#person-body [role=option]');
await page.keyboard.press('Enter');
await page.click('form[aria-label="Add a relationship"] button[type=submit]');
await page.waitForSelector('text=Mother of June Rowan');
// Add to a new family from the picker
await page.getByRole('combobox', { name: 'Add to a family' }).fill('The Hale family');
await page.waitForSelector('#person-body [role=option]:has-text("as a new family")');
await page.click('#person-body [role=option]:has-text("as a new family")');
await page.waitForSelector('#person-body .lines >> text=The Hale family');
await axe(page, '2-person');
// June's side reads the other way
await page.click('text=Mother of June Rowan');
await page.waitForSelector('#h-person:text("June Rowan")');
const juneRel = await page.textContent('#person-body');
results.juneSeesDaughter = juneRel.includes('Daughter of Ada Hale') || juneRel.includes('Child of Ada Hale');
// The pet
await page.goto('http://127.0.0.1:8765/');
await page.waitForSelector('#journey-title:not(:empty)');
await page.click('button.navbtn[data-go="people"]');
await page.click('button.person:has-text("Pepper")');
await page.waitForSelector('#h-person:text("Pepper")');
await axe(page, '3-pet');
// Family screen
await page.click('button.navbtn[data-go="people"]');
await page.click('#family-list h2 button:has-text("The Rowans")');
await page.click('#family-summary .btns .btn.primary'); // the name shows a summary first (sprint 12)
await page.waitForSelector('#h-family');
await axe(page, '4-family');
// Places and events
await page.click('button.navbtn[data-go="people"]');
await page.click('.pane[data-pane="people"] button[data-go="places"]');
await page.waitForSelector('#h-places');
await page.fill('#new-place', 'Ogden UT');
await page.click('form[aria-label="Add a place"] button[type=submit]');
await page.waitForSelector('#h-place:text("Ogden UT")');
await page.getByLabel('Coordinates (optional)').fill('41.22, -111.97');
await page.click('#place-body form.form button[type=submit]');
await page.waitForTimeout(300);
await axe(page, '5-place');
await page.click('.pane[data-pane="people"] button[data-go="places"]').catch(() => {});
await page.click('button.navbtn[data-go="people"]');
await page.click('.pane[data-pane="people"] button[data-go="places"]');
await page.click('#places-body button.link:has-text("Ogden, Utah")');
await page.waitForSelector('#h-place:text("Ogden, Utah")');
await page.getByRole('combobox', { name: 'Merge another place into this one' }).fill('Ogden UT');
await page.waitForSelector('#place-body [role=option]');
await page.keyboard.press('Enter');
await page.waitForTimeout(400);
results.mergeDialog = await page.evaluate(() => window.lastDialog);
await axe(page, '6-places-list-after-merge');
await page.click('button.navbtn[data-go="people"]');
await page.click('.pane[data-pane="people"] button[data-go="places"]');
results.placesAfterMerge = await page.$$eval('#places-body .section:first-child .lines li', (l) => l.map((x) => x.innerText.split('\n')[0]));
await axe(page, '7-places');
await page.click('#places-body button.link:has-text("Wedding of Walter and June Rowan")');
await page.waitForSelector('#h-event');
await axe(page, '8-event');
// Remove Ada with the warning
await page.fill('#q', 'Ada');
await page.waitForSelector('#results [role=option]');
await page.keyboard.press('Enter');
await page.waitForSelector('#h-person:text("Ada Hale")');
await page.click('#person-body .btn.danger');
await page.waitForTimeout(400);
results.removeDialog = await page.evaluate(() => window.lastDialog);
results.afterRemoveShowing = await page.evaluate(() => document.querySelector('.pane:not([hidden]) h1')?.textContent);
results.adaStillSearchable = (await page.evaluate(() => window.__TAURI__.core.invoke('search', { text: 'Ada' }))).length > 0;
// Search quick-add
await page.fill('#q', 'Uncle Bert');
await page.waitForSelector('#results [role=option]:has-text("as a new person")');
await page.click('#results [role=option]:has-text("as a new person")');
await page.waitForSelector('#h-person:text("Uncle Bert")');
results.quickAddFromSearch = true;
fs.writeFileSync(`${out}/results.json`, JSON.stringify({ results, errors }, null, 2));
console.log(JSON.stringify({ results, errors }, null, 2));
await browser.close();
server.kill();
