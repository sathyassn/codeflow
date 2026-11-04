// TSK-443 AC-5: real pointer gestures, server anchors, revisions and offline export.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium } from 'playwright-core';
import { installedChrome } from './browser-executables.mjs';
import { codeflowBinary } from './codeflow-binary.mjs';
const repo = resolve(import.meta.dirname, '../../../..');
const root = process.env.CF_PRESENT_EVIDENCE ? resolve(process.env.CF_PRESENT_EVIDENCE) : await mkdtemp(join(repo, 'target/figure-text-'));
await mkdir(root, { recursive: true });
const project = join(root, 'project');
await mkdir(project, { recursive: true });
execFileSync('git', ['init', '--quiet'], { cwd: project });
const env = { ...process.env, XDG_STATE_HOME: join(root, 'state'), CF_PRESENT_OWNER_PID: String(process.pid) };
const run = args => execFileSync(codeflowBinary(repo), args, { cwd: project, env, encoding: 'utf8', timeout: 60_000 });
const declaration = JSON.parse(await readFile(join(repo, 'crates/codeflow-present/tests/fixtures/review-sequence.json')));
for (const variant of ['wide', 'narrow']) declaration.figure[variant].draw.push({ text: 'Review time 🧭', x: 20, y: 190, style: 'mute' });
const doc = { schema_version: 2, title: 'Figure words', blocks: [{ type: 'figure', id: 'sequence', declaration }] };
const input = join(root, 'document.json');
await writeFile(input, JSON.stringify(doc));
const failures = [];
const check = async (name, fn) => { try { await fn(); console.log(`PASS ${name}`); } catch (e) { failures.push(`${name}: ${e.message}`); console.error(`FAIL ${name}: ${e.message}`); } };
const browser = await chromium.launch({ executablePath: await installedChrome(), headless: true });
const context = await browser.newContext({ viewport: { width: 1600, height: 1000 } });
let session;
async function drag(page, locator, words) {
  await locator.scrollIntoViewIfNeeded();
  const box = await locator.evaluate((el, words) => {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
      const node = walker.currentNode;
      const at = node.data.indexOf(words);
      if (at < 0) continue;
      const range = document.createRange(); range.setStart(node, at); range.setEnd(node, at + words.length);
      const rect = range.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    }
    throw new Error(`no words ${words}`);
  }, words);
  await page.mouse.move(box.x + 0.5, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width - 0.5, box.y + box.height / 2, { steps: 10 });
  await page.mouse.up();
}
async function save(page, body) {
  await page.getByTestId('float-comment').click();
  await page.getByTestId('composer-text').fill(body);
  await page.getByTestId('composer-save').click();
  await page.getByTestId('composer').waitFor({ state: 'detached' });
}
const notes = () => run(['present', 'responses', 'list', session, '--kind', 'review']).trim().split('\n').filter(Boolean).flatMap(line => JSON.parse(line).notes);
try {
  const opened = run(['present', 'open', input, '--no-launch']);
  session = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
  const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
  const page = await context.newPage();
  await page.goto(pathToFileURL(bootstrap).href);
  await page.locator('[data-cf-figure-block="ready"]').waitFor();
  const expected = [];
  for (const theme of ['light', 'dark']) {
    await page.getByTestId('settings-btn').click();
    await page.getByTestId('settings-panel').getByRole('button', { name: theme === 'light' ? 'Light' : 'Dark', exact: true }).click();
    await page.keyboard.press('Escape');
    if (await page.locator('#cf-comment-toggle').getAttribute('aria-pressed') !== 'true') await page.locator('#cf-comment-toggle').click();
    for (const [name, selector, words] of [
      ['label', 'svg[data-cf-variant="wide"] text', 'Author'],
      ['axis', 'svg[data-cf-variant="wide"] text', 'time 🧭'],
      ['legend', '.cf-legend li', 'Participant'],
    ]) {
      const target = page.locator(selector).filter({ hasText: words }).first();
      if (name === 'label') {
        await target.hover();
        await check(`${theme} label hover glows one part`, async () => {
          await page.locator('[data-cf-figure-output] [data-cf-entity="author"].cf-hot').waitFor();
          const hot = page.locator('[data-cf-figure-output] .cf-hot');
          assert.equal(await hot.count(), 1);
          assert.equal(await hot.getAttribute('data-cf-entity'), 'author');
          assert.notEqual(await hot.evaluate(el => getComputedStyle(el).filter), 'none');
        });
        await page.screenshot({ path: join(root, `${theme}-label-hover.png`) });
      }
      await drag(page, target, words);
      const pinned = await page.getByTestId('float-chip').waitFor({ timeout: 5_000 }).then(() => true, () => false);
      await check(`${theme} ${name} drag creates a text pin`, async () => {
        assert.ok(pinned, 'the drag produced no pin');
        assert.equal((await page.getByTestId('float-chip').locator('.lab').innerText()).trim(), 'Text');
      });
      await page.screenshot({ path: join(root, `${theme}-${name}-selection.png`) });
      const body = `${theme} ${name} words`;
      if (pinned) await save(page, body);
      expected.push({ body, words });
    }
    const label = page.locator('svg[data-cf-variant="wide"] text').filter({ hasText: /^Author$/u });
    await label.click();
    await check(`${theme} click still pins the part`, async () => assert.equal((await page.getByTestId('float-chip').locator('.lab').innerText()).trim(), 'Element'));
    await save(page, `${theme} part click`);
    const drawing = page.locator('svg[data-cf-variant="wide"]');
    await drawing.scrollIntoViewIfNeeded();
    const box = await drawing.boundingBox();
    await page.mouse.move(box.x + box.width * 0.35, box.y + box.height * 0.72);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width * 0.65, box.y + box.height * 0.92, { steps: 10 });
    await page.mouse.up();
    await check(`${theme} empty-space drag creates a region`, async () => assert.equal((await page.getByTestId('float-chip').locator('.lab').innerText()).trim(), 'Area'));
    await page.screenshot({ path: join(root, `${theme}-region.png`) });
    await save(page, `${theme} empty region`);
  }
  await page.setViewportSize({ width: 375, height: 812 });
  if (await page.getByTestId('notes-expand').isVisible() && await page.getByTestId('notes-dock').getAttribute('data-expanded') === 'true') await page.getByTestId('notes-expand').click();
  const narrow = page.locator('svg[data-cf-variant="narrow"] text').filter({ hasText: /^Author$/u });
  await drag(page, narrow, 'Author');
  await page.getByTestId('float-chip').waitFor();
  await check('narrow variant selects its own text occurrence', async () => assert.equal((await page.getByTestId('float-chip').locator('.lab').innerText()).trim(), 'Text'));
  await page.screenshot({ path: join(root, 'narrow-selection.png') });
  await save(page, 'narrow label words');
  expected.push({ body: 'narrow label words', words: 'Author' });
  await page.getByLabel('Verdict').selectOption('approve_with_notes');
  await page.getByRole('button', { name: 'Submit review' }).click();
  await page.getByRole('status').getByText(/Review received/u).waitFor();
  for (const { body, words } of expected) await check(`${body} stored as exact text`, async () => {
    const note = notes().find(note => note.body === body);
    assert.equal(note?.selector?.exact, words);
    assert.equal(note.entity_selector, undefined);
  });
  const originalNotes = notes();
  for (const theme of ['light', 'dark']) {
    await check(`${theme} click stored as the same part`, async () => {
      const note = originalNotes.find(note => note.body === `${theme} part click`);
      assert.equal(note?.entity_selector?.entity_id, 'author');
      assert.equal(note.selector, undefined);
    });
    await check(`${theme} empty-space drag stored as a region`, async () => {
      const note = originalNotes.find(note => note.body === `${theme} empty region`);
      assert.ok(note?.region_selector);
      assert.equal(note.selector, undefined);
    });
  }
  await check('wide and narrow copies keep separate canonical positions', async () => {
    const wide = originalNotes.find(note => note.body === 'light label words')?.selector;
    const narrow = originalNotes.find(note => note.body === 'narrow label words')?.selector;
    assert.ok(wide && narrow && narrow.start_utf16 > wide.end_utf16);
  });
  for (const theme of ['light', 'dark']) {
    const out = join(root, `export-${theme}.html`);
    run(['present', 'export', session, '--out', out, '--mode', theme, '--with-notes']);
    const exported = await context.newPage();
    await exported.goto(pathToFileURL(out).href);
    await exported.locator('[data-cf-figure-block="ready"]').waitFor();
    await drag(exported, exported.locator('svg[data-cf-variant="wide"] text').filter({ hasText: /^Author$/u }), 'Author');
    await check(`${theme} export keeps selectable words and stored text notes`, async () => {
      assert.equal(await exported.evaluate(() => String(getSelection())), 'Author');
      const conversation = await exported.getByRole('region', { name: 'Conversation' }).innerText();
      assert.ok(conversation.includes('"exact": "Author"'));
      assert.equal(await exported.locator('#cf-comment-toggle').count(), 0);
    });
    await exported.screenshot({ path: join(root, `${theme}-export.png`) });
    await exported.close();
  }
  for (const variant of ['wide', 'narrow']) for (const item of doc.blocks[0].declaration.figure[variant].draw) if (item.text === 'Author') item.x += 5;
  await writeFile(input, JSON.stringify(doc));
  run(['present', 'update', session, input]);
  await check('revision keeps text anchors when only geometry moves', async () => {
    for (const { body } of expected) {
      const anchor = notes().find(note => note.body === body)?.anchor;
      const original = originalNotes.find(note => note.body === body)?.selector;
      assert.equal(anchor?.state, 'reanchored');
      assert.equal(anchor.start_utf16, original?.start_utf16);
      assert.equal(anchor.end_utf16, original?.end_utf16);
    }
  });
  doc.blocks = [{ type: 'narrative', id: 'replacement', markdown: 'The figure has been removed.' }];
  await writeFile(input, JSON.stringify(doc));
  run(['present', 'update', session, input]);
  await check('revision orphans text anchors when their figure is removed', async () => {
    for (const { body } of expected) assert.equal(notes().find(note => note.body === body)?.anchor?.state, 'orphaned');
  });
  await writeFile(join(root, 'notes.json'), JSON.stringify(notes(), null, 2));
  await page.close();
  run(['present', 'close', session]);
  session = null;
  for (const kind of ['coverage', 'extent']) {
    const layoutFigure = structuredClone(declaration);
    const f = layoutFigure.figure;
    f.family = kind;
    f.title = kind === 'coverage' ? 'Review coverage' : 'Review time';
    f.caption = kind === 'coverage' ? 'The review runs locally and remotely.' : 'The review takes 12.35 ms.';
    f.states = [{ name: 'covered', mark: kind === 'coverage' ? 'cov' : 'used', means: kind === 'coverage' ? 'Covered' : 'Time used' }];
    delete f.wide;
    f.narrow = { recompose: 'stack', drops: [], marks: 'same' };
    f.layout = kind === 'coverage'
      ? { kind, columns: ['Local', 'Remote'], rows: [{ label: 'Review', cells: ['covered', 'covered'] }] }
      : { kind, max: 80, unit: 'ms', rows: [{ label: 'Review', value: 12.345, state: 'covered' }] };
    await writeFile(input, JSON.stringify({ schema_version: 2, title: 'Layout words', blocks: [{ type: 'figure', id: 'layout', declaration: layoutFigure }] }));
    const opened = run(['present', 'open', input, '--no-launch']);
    session = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
    const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
    const layoutPage = await context.newPage();
    await layoutPage.goto(pathToFileURL(bootstrap).href);
    await layoutPage.locator('[data-cf-figure-block="ready"]').waitFor();
    await layoutPage.locator('#cf-comment-toggle').click();
    const words = kind === 'coverage' ? 'Local' : '12.35 ms';
    await drag(layoutPage, layoutPage.locator('svg[data-cf-variant="wide"] text').filter({ hasText: words }).first(), words);
    await layoutPage.getByTestId('float-chip').waitFor();
    await check(`${kind} layout selects generated words`, async () => assert.equal((await layoutPage.getByTestId('float-chip').locator('.lab').innerText()).trim(), 'Text'));
    await layoutPage.screenshot({ path: join(root, `${kind}-selection.png`) });
    await save(layoutPage, `${kind} words`);
    await layoutPage.getByLabel('Verdict').selectOption('approve_with_notes');
    await layoutPage.getByRole('button', { name: 'Submit review' }).click();
    await layoutPage.getByRole('status').getByText(/Review received/u).waitFor();
    await check(`${kind} generated words stored as text`, async () => assert.equal(notes().find(note => note.body === `${kind} words`)?.selector?.exact, words));
    await layoutPage.close();
    run(['present', 'close', session]);
    session = null;
  }
} finally {
  if (session) { try { run(['present', 'close', session]); } catch {} }
  await context.close(); await browser.close();
  await writeFile(join(root, 'results.json'), JSON.stringify({ failures }, null, 2));
  console.log(`Figure text evidence: ${root}`);
}
assert.deepEqual(failures, []);
