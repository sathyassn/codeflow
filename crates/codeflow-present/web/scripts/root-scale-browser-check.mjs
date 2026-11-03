// TSK-1109: real runtime, authored stages, figure labels and chrome at each Size.
// --styles-from <revision> installs that revision's sheets as a fail-before probe.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium } from 'playwright-core';
import { installedChrome } from './browser-executables.mjs';
import { codeflowBinary } from './codeflow-binary.mjs';
const repo = resolve(import.meta.dirname, '../../../..');
const root = process.env.CF_PRESENT_EVIDENCE ? resolve(process.env.CF_PRESENT_EVIDENCE) : await mkdtemp(join(repo, 'target/root-scale-'));
await mkdir(root, { recursive: true });
const project = join(root, 'project');
await mkdir(project, { recursive: true });
execFileSync('git', ['init', '--quiet'], { cwd: project });
const env = { ...process.env, XDG_STATE_HOME: join(root, 'state'), CF_PRESENT_OWNER_PID: String(process.pid) };
const run = args => execFileSync(codeflowBinary(repo), args, { cwd: project, env, encoding: 'utf8', timeout: 60_000 });
const declaration = JSON.parse(await readFile(join(repo, 'crates/codeflow-present/tests/fixtures/review-sequence.json')));
const input = join(root, 'document.json');
await writeFile(input, JSON.stringify({ schema_version: 2, title: 'Text follows the reader', blocks: [
  { type: 'narrative', id: 'intro', markdown: '## A section\n\nText that scales with the reader.' },
  { type: 'html', id: 'stage', title: 'A stage', caption: 'Both authored and inherited text follow Size.', html: '<style>.scale-line { font-size: 0.88rem; }</style><p class="scale-line">Authored stage line</p><p class="inherited-line">Inherited stage line</p>' },
  { type: 'figure', id: 'figure', declaration },
] }));
const stylesIndex = process.argv.indexOf('--styles-from');
const replacement = stylesIndex < 0 ? null : ['styles.css', 'figure.css'].map(name => execFileSync('git', ['show', `${process.argv[stylesIndex + 1]}:crates/codeflow-present/web/src/${name}`], { cwd: repo, encoding: 'utf8' })).join('\n');
const close = (got, expected, label) => assert.ok(Math.abs(got - expected) < 0.03, `${label}: ${got}, expected ${expected}`);
const fontSize = locator => locator.evaluate(el => parseFloat(getComputedStyle(el).fontSize));
const failures = [];
const check = async (name, fn) => { try { await fn(); console.log(`PASS ${name}`); } catch (error) { failures.push(`${name}: ${error.message}`); console.error(`FAIL ${name}: ${error.message}`); } };
const floorViolations = () => {
  const floor = parseFloat(getComputedStyle(document.documentElement).fontSize) * 0.76;
  const found = [];
  const surfaces = document.querySelectorAll('.cf-topbar, .cf-dock, .cf-settings-panel');
  for (const surface of surfaces) {
    const walker = document.createTreeWalker(surface, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
      const text = walker.currentNode;
      if (!text.textContent.trim()) continue;
      const el = text.parentElement;
      if (!el || !el.checkVisibility()) continue;
      const size = parseFloat(getComputedStyle(el).fontSize);
      if (size + 0.02 < floor) found.push(`${el.className}: ${text.textContent.trim().slice(0, 40)} at ${size} < ${floor}`);
    }
  }
  return [...new Set(found)];
};
const browser = await chromium.launch({ executablePath: await installedChrome(), headless: true });
let session;
try {
  const opened = run(['present', 'open', input, '--no-launch']);
  session = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
  const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
  assert.ok(session && bootstrap, opened);
  for (const width of [1200, 400]) {
    const page = await browser.newPage({ viewport: { width, height: 1000 } });
    await page.goto(pathToFileURL(bootstrap).href);
    await page.locator('[data-cf-figure-block="ready"]').waitFor();
    if (replacement) await page.addStyleTag({ content: replacement });
    await page.getByTestId('comment-btn').click();
    if (await page.getByTestId('notes-dock').getAttribute('data-expanded') !== 'true' && await page.getByTestId('notes-expand').isVisible()) await page.getByTestId('notes-expand').click();
    await page.getByTestId('tool-whole-doc').click();
    await page.getByTestId('composer-text').fill('A readable note in the comment rail.');
    await page.getByTestId('composer-save').click();
    await page.getByTestId('composer').waitFor({ state: 'detached' });
    for (const mode of ['light', 'dark']) {
      if (await page.getByTestId('comment-btn').getAttribute('aria-pressed') !== 'true') await page.getByTestId('comment-btn').click();
      if (await page.getByTestId('notes-dock').getAttribute('data-expanded') !== 'true' && await page.getByTestId('notes-expand').isVisible()) await page.getByTestId('notes-expand').click();
      await page.getByTestId('settings-btn').click();
      const panel = page.getByTestId('settings-panel');
      await panel.getByRole('button', { name: mode === 'light' ? 'Light' : 'Dark', exact: true }).click();
      for (const [scale, px] of [['compact', 15], ['default', 17], ['large', 19]]) {
        await panel.locator(`[data-scale="${scale}"]`).click();
        await page.waitForFunction(step => document.documentElement.dataset.cfScale === step, scale);
        await check(`${width} ${mode} ${scale} root, stage, figure and preview`, async () => {
          close(await fontSize(page.locator('html')), px, 'root');
          close(await fontSize(page.locator('.scale-line')), px * 0.88, 'authored line');
          close(await fontSize(page.locator('.inherited-line')), px * 0.88, 'inherited line');
          close(await fontSize(page.locator('.cf-fig-number').first()), px * 0.76, 'figure label');
          close(await fontSize(page.getByTestId('preview-stage-line')), px * 0.88, 'preview stage');
          close(await fontSize(page.getByTestId('preview-figure-label')), px * 0.76, 'preview figure');
        });
        await check(`${width} ${mode} ${scale} chrome floor`, async () => {
          assert.ok(await page.getByTestId('notes-dock').isVisible());
          assert.ok(await page.getByTestId('note-row').first().isVisible());
          assert.deepEqual(await page.evaluate(floorViolations), []);
        });
        await check(`${width} ${mode} ${scale} floor catches a planted small note`, async () => {
          const style = await page.addStyleTag({ content: '.cf-note-row .b { font-size: 0.5rem !important; }' });
          try { assert.ok((await page.evaluate(floorViolations)).some(text => text.includes('readable note'))); }
          finally { await style.evaluate(el => el.remove()); }
        });
        await page.screenshot({ path: join(root, `${width}-${mode}-${scale}-display.png`) });
      }
      await check(`${width} ${mode} heading choice persists and loads an existing face`, async () => {
        await panel.locator('[data-headings="archivo"]').click();
        await page.waitForFunction(() => document.documentElement.dataset.cfHeadings === 'archivo');
        await page.locator('h1').evaluate(async el => { await document.fonts.load(`600 20px Archivo`); assertFont(el); function assertFont(el) { if (getComputedStyle(el).fontFamily.split(',')[0].replaceAll('\"', '').trim() !== 'Archivo') throw new Error('heading is not Archivo'); } });
        await page.reload();
        await page.locator('[data-cf-figure-block="ready"]').waitFor();
        assert.equal(await page.locator('html').getAttribute('data-cf-headings'), 'archivo');
        if (replacement) await page.addStyleTag({ content: replacement });
        await page.getByTestId('settings-btn').click();
        await page.getByTestId('settings-panel').locator('[data-headings="body"]').click();
      });
      await page.keyboard.press('Escape');
    }
    // Assert the relative root contract here. A real changed browser font
    // preference remains a separate host journey; this does not simulate it.
    await page.evaluate(() => { document.documentElement.dataset.cfScale = 'large'; });
    await check(`${width} reader default remains effective`, async () => {
      const scale = await page.locator('html').evaluate(el => getComputedStyle(el).getPropertyValue('--cf-root-size').trim());
      assert.equal(scale, '118.75%');
    });
    await page.close();
  }
  for (const mode of ['light', 'dark']) {
    const output = join(root, `export-${mode}.html`);
    run(['present', 'export', session, '--out', output, '--mode', mode]);
    const page = await browser.newPage();
    await page.goto(pathToFileURL(output).href);
    if (replacement) await page.addStyleTag({ content: replacement });
    for (const [scale, px] of [['compact', 15], ['default', 17], ['large', 19]]) {
      await page.evaluate(step => { document.documentElement.dataset.cfScale = step; }, scale);
      const line = page.frameLocator('iframe[data-cf-export-stage]').locator('.scale-line');
      await check(`export ${mode} ${scale} frame inherits root size`, async () => {
        await page.waitForTimeout(100);
        close(await fontSize(line), px * 0.88, 'frame line');
      });
    }
    await page.close();
  }
} finally {
  await browser.close();
  if (session) run(['present', 'close', session]);
}
if (failures.length) throw new Error(failures.join('\n'));
console.log(`Root scale and chrome floor checks passed. Evidence: ${root}`);
