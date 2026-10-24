// TSK-443: real CLI, export and live drawing. Chrome runs on wall time.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium } from 'playwright-core';
import { installedChrome } from './browser-executables.mjs';
import { codeflowBinary } from './codeflow-binary.mjs';

const repo = resolve(import.meta.dirname, '../../../..');
await mkdir(join(repo, 'target'), { recursive: true });
const root = process.env.CF_PRESENT_EVIDENCE ? resolve(process.env.CF_PRESENT_EVIDENCE) : await mkdtemp(join(repo, 'target/tsk-443-surface-'));
await mkdir(root, { recursive: true });
const project = join(root, 'project');
await mkdir(project, { recursive: true });
const env = { ...process.env, XDG_STATE_HOME: join(root, 'state'), CF_PRESENT_OWNER_PID: String(process.pid) };
const run = (args) => execFileSync(codeflowBinary(repo), args, { cwd: project, env, encoding: 'utf8', timeout: 60_000 });
execFileSync('git', ['init', '--quiet'], { cwd: project });
const figure = { type: 'figure', id: 'sequence', declaration: JSON.parse(await readFile(join(repo, 'crates/codeflow-present/tests/fixtures/review-sequence.json'))) };
const stage = { type: 'html', id: 'stage', title: 'Review stage', caption: 'Review precedes landing.', html: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 360 120" fill="none"><title>Review stage</title><circle data-cf-target="review" data-cf-label="Review" cx="40" cy="60" r="15" fill="none" stroke="var(--cf-diagram-line)"/><text x="70" y="65" fill="var(--cf-text)">Review before landing</text></svg>' };
const doc = { schema_version: 2, title: 'Review surface regression', blocks: [
  { type: 'narrative', id: 'hierarchy', markdown: '## Section\n### Subsection\n#### Detail\n##### Smaller detail\n###### Smallest detail\nSelectable review prose.' },
  { type: 'tabs', id: 'versions', tabs: [ { label: 'After', blocks: [{ type: 'narrative', id: 'after', markdown: 'The corrected result.' }] }, { label: 'Before', blocks: [{ type: 'narrative', id: 'before', markdown: 'The previous result.' }] } ] },
  { type: 'comparison', id: 'comparison', columns: [{ title: 'Block title', markdown: 'A peer view.' }, { title: 'Another block title', markdown: 'A second peer view.' }] },
  figure, stage,
] };
// Old inputs remain valid; the titled control is exercised after the baseline.
if (!process.env.CF_PRESENT_BASELINE) doc.blocks[1].title = 'Versions';
const input = join(root, 'document.json');
await writeFile(input, JSON.stringify(doc));
const failures = [];
const check = async (name, fn) => { try { await fn(); console.log(`PASS ${name}`); } catch (e) { failures.push(`${name}: ${e.message}`); console.error(`FAIL ${name}: ${e.message}`); } };
async function setComment(page, armed) {
  const button = page.locator('#cf-comment-toggle');
  if ((await button.getAttribute('aria-pressed') === 'true') !== armed) {
    if (armed) await button.click();
    else await page.getByTestId('comment-leave').click();
  }
  await page.waitForFunction(value => document.querySelector('#cf-present-document')?.getAttribute('data-cf-commenting') === String(value), armed);
  // Let the armed rail finish moving before measuring pointer coordinates.
  await page.waitForTimeout(250);
}
const browser = await chromium.launch({ executablePath: await installedChrome(), headless: true });
const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
let session;
let probe;
try {
  await check('present check accepts the journey document', async () => {
    const output = run(['present', 'check', '--file', input]);
    assert.ok(output.includes('"valid":true'), output);
  });
  for (const defect of ['json-fact', 'wide']) {
    await check(`present check refuses ${defect}`, async () => {
      const invalid = structuredClone(figure);
      if (defect === 'json-fact') {
        invalid.declaration.figure.facts[0].check = { kind: 'contains', text: 'review' };
        invalid.declaration.figure.facts[0].value = true;
      } else {
        invalid.declaration.figure.family = 'coverage';
        invalid.declaration.figure.wide.width = 760;
      }
      const path = join(root, `${defect}.json`);
      await writeFile(path, JSON.stringify({ schema_version: 2, title: 'Undrawable figure', blocks: [invalid] }));
      let status = 0;
      let output = '';
      try { output = run(['present', 'check', '--file', path]); } catch (error) { status = error.status; output = String(error.stdout); }
      await writeFile(join(root, `${defect}-check.jsonl`), output);
      assert.equal(status, 9, output);
    });
  }
  const opened = run(['present', 'open', input, '--no-launch']);
  session = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
  assert.ok(session, opened);
  const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
  const page = await context.newPage();
  await page.goto(pathToFileURL(bootstrap).href);
  await page.locator('#cf-comment-toggle').waitFor();
  await check('prose remains selectable in Comment mode', async () => {
    await setComment(page, true);
    const box = await page.locator('#hierarchy p').evaluate(el => {
      const range = document.createRange();
      range.selectNodeContents(el);
      const rect = range.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    });
    await page.mouse.move(box.x + 1, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width - 1, box.y + box.height / 2, { steps: 8 });
    await page.mouse.up();
    assert.equal((await page.getByTestId('float-chip').locator('.lab').innerText()).trim(), 'Text');
  });
  await page.keyboard.press('Escape');
  await setComment(page, false);
  await check('tabs switch with pointer and keyboard', async () => {
    const tab = page.getByRole('tab', { name: 'Before', exact: true });
    await tab.click({ timeout: 3000 });
    assert.equal(await page.locator('#before').isVisible(), true);
    assert.equal(await page.locator('#after').isVisible(), false);
    await page.keyboard.press('ArrowLeft');
    assert.equal(await page.getByRole('tab', { name: 'After', exact: true }).getAttribute('aria-selected'), 'true');
    assert.equal(await page.locator('#after').isVisible(), true);
    if (!process.env.CF_PRESENT_BASELINE) {
      const nav = page.getByRole('navigation', { name: 'Document sections' });
      assert.equal(await nav.getByRole('link', { name: 'Versions', exact: true }).count(), 1);
      assert.equal(await nav.locator('[href="#before"], [href="#after"]').count(), 0);
    }
  });
  await check('Comment pins a tab label without changing panels', async () => {
    await setComment(page, true);
    await page.getByRole('tab', { name: 'Before', exact: true }).click({ timeout: 3000 });
    assert.ok((await page.getByTestId('float-chip').innerText()).includes('Before'));
    assert.equal(await page.locator('#after').isVisible(), true);
    assert.equal(await page.locator('#before').isVisible(), false);
  });
  await page.keyboard.press('Escape');
  await setComment(page, false);
  await check('heading hierarchy', async () => {
    const sizes = await page.locator('#hierarchy').evaluate(el => [...el.querySelectorAll('h2,h3,h4,h5,h6')].map(h => parseFloat(getComputedStyle(h).fontSize)));
    assert.ok(sizes.every((size, i) => !i || size <= sizes[i - 1]), JSON.stringify(sizes));
    const blockSize = await page.locator('#comparison h2').first().evaluate(el => parseFloat(getComputedStyle(el).fontSize));
    assert.ok(blockSize < sizes[1], `block title ${blockSize} >= subsection ${sizes[1]}`);
  });
  await page.screenshot({ path: join(root, 'live-top.png') });
  await page.locator('#comparison').screenshot({ path: join(root, 'block-titles.png') });
  const out = join(root, 'export.html');
  run(['present', 'export', session, '--out', out, '--mode', 'light']);
  const exported = await context.newPage();
  const exportErrors = [];
  exported.on('console', message => { if (message.type() === 'error') exportErrors.push(message.text()); });
  await exported.goto(pathToFileURL(out).href);
  await exported.waitForFunction(() => document.documentElement.dataset.cfExport === 'true');
  await check('export tabs switch', async () => {
    await exported.getByRole('tab', { name: 'Before', exact: true }).click({ timeout: 3000 });
    assert.equal(await exported.locator('#before').isVisible(), true);
    assert.equal(await exported.locator('#after').isVisible(), false);
  });
  await check('export stage draws visible ink', async () => {
    // Chrome can suspend an offscreen frame's animation-frame polling.
    await exported.locator('#stage').scrollIntoViewIfNeeded();
    const inline = exported.locator('#stage .cf-stage-host svg');
    if (await inline.count()) {
      assert.equal(await inline.isVisible(), true);
      assert.equal(await inline.locator('text').textContent(), 'Review before landing');
    } else {
      const frame = await exported.locator('#stage iframe').elementHandle();
      const content = await frame.contentFrame();
      await content.locator('svg').waitFor({ timeout: 3000 });
      assert.equal(await content.locator('text').textContent(), 'Review before landing');
      await content.waitForFunction(() => { const text = document.querySelector('text'); return text && getComputedStyle(text).fill !== 'none'; }, null, { timeout: 3000 });
      assert.notEqual(await content.locator('text').evaluate(el => getComputedStyle(el).fill), 'none');
      assert.notEqual(await content.locator('circle').evaluate(el => getComputedStyle(el).stroke), 'none');
      assert.equal(await exported.locator('#stage iframe').getAttribute('sandbox'), '');
    }
  });
  await writeFile(join(root, 'export-console.json'), JSON.stringify(exportErrors, null, 2));
  await exported.locator('#stage').scrollIntoViewIfNeeded();
  await exported.screenshot({ path: join(root, 'export-stage.png') });
  await check('export stage follows system appearance', async () => {
    const systemOut = join(root, 'export-system.html');
    run(['present', 'export', session, '--out', systemOut, '--mode', 'system']);
    const themed = await context.newPage();
    await themed.emulateMedia({ colorScheme: 'dark' });
    await themed.goto(pathToFileURL(systemOut).href);
    await themed.waitForFunction(() => document.documentElement.dataset.cfExport === 'true');
    const handle = await themed.locator('#stage iframe').elementHandle();
    const frame = await handle.contentFrame();
    for (const scheme of ['dark', 'light']) {
      await themed.emulateMedia({ colorScheme: scheme });
      await themed.waitForFunction(mode => document.documentElement.dataset.cfModeResolved === mode, scheme);
      await themed.locator('#stage').scrollIntoViewIfNeeded();
      const expected = await themed.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--cf-text').trim());
      await frame.waitForFunction(color => {
        const text = document.querySelector('text');
        if (!text) return false;
        const probe = document.createElement('span');
        probe.style.color = color;
        document.body.append(probe);
        const resolved = getComputedStyle(probe).color;
        probe.remove();
        return getComputedStyle(text).fill === resolved;
      }, expected, { timeout: 3000 });
      await frame.locator('text').waitFor({ state: 'visible' });
      await themed.screenshot({ path: join(root, `export-stage-${scheme}.png`) });
    }
    await themed.close();
  });
  await check('export sequence draws', async () => {
    assert.equal(await exported.locator('[data-cf-figure-block]').getAttribute('data-cf-figure-block'), 'ready');
  });
  // A lower drawing gets its own real session, avoiding lazy-load timing guesses.
  const probeFile = join(root, 'probe.json');
  await writeFile(probeFile, JSON.stringify({ schema_version: 2, title: 'Figure probe', blocks: [figure] }));
  const probeOpen = run(['present', 'open', probeFile, '--no-launch']);
  probe = probeOpen.match(/session ([0-9a-f-]+) ready/u)?.[1];
  const probeBootstrap = probeOpen.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
  const drawing = await context.newPage();
  await drawing.goto(pathToFileURL(probeBootstrap).href);
  await drawing.locator('[data-cf-figure-block="ready"]').waitFor();
  await setComment(drawing, true);
  await check('empty SVG hover outlines without a whole-drawing glow', async () => {
    const svg = drawing.locator('svg.cf-fig-svg:visible');
    const box = await svg.boundingBox();
    await drawing.mouse.move(box.x + box.width - 5, box.y + box.height - 5);
    await drawing.waitForTimeout(150);
    await drawing.screenshot({ path: join(root, 'figure-root.png') });
    const style = await svg.evaluate(el => ({ hot: el.classList.contains('cf-hot'), filter: getComputedStyle(el).filter, outline: getComputedStyle(el).outlineStyle }));
    assert.equal(style.hot, true, JSON.stringify(style));
    assert.equal(style.filter, 'none');
    assert.equal(style.outline, 'solid');
  });
  await check('SVG root pin uses its figure title', async () => {
    const box = await drawing.locator('svg.cf-fig-svg:visible').boundingBox();
    await drawing.mouse.click(box.x + box.width - 5, box.y + box.height - 5);
    assert.ok((await drawing.getByTestId('float-chip').innerText()).includes(figure.declaration.figure.title));
  });
  await drawing.keyboard.press('Escape');
  await setComment(drawing, true);
  await check('part hover glows one part', async () => {
    const part = drawing.locator('svg.cf-fig-svg:visible [data-cf-entity="request"]');
    const point = await part.evaluate(el => { const box = el.getBoundingClientRect(); return { x: box.x + box.width / 2, y: box.y + box.height / 2 }; });
    await drawing.mouse.move(point.x, point.y);
    await drawing.waitForTimeout(150);
    assert.equal(await drawing.locator('svg .cf-hot').count(), 1);
    assert.notEqual(await drawing.locator('svg .cf-hot').evaluate(el => getComputedStyle(el).filter), 'none');
  });
  await drawing.screenshot({ path: join(root, 'figure-part.png') });
  await check('live HTML stage draws', async () => {
    await page.locator('#stage').scrollIntoViewIfNeeded();
    assert.equal(await page.locator('#stage svg text').textContent(), 'Review before landing');
    assert.equal(await page.locator('#stage svg').isVisible(), true);
  });
  await page.screenshot({ path: join(root, 'live-stage.png') });
} finally {
  await context.close();
  await browser.close();
  for (const id of [session, probe].filter(Boolean)) run(['present', 'close', id]);
}
await writeFile(join(root, 'results.json'), JSON.stringify({ failures }, null, 2));
console.log(`Review surface evidence: ${root}`);
assert.deepEqual(failures, []);
