// TSK-443: every capture and review step in installed Chrome, including phone width.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium } from 'playwright-core';
import { installedChrome } from './browser-executables.mjs';
import { codeflowBinary } from './codeflow-binary.mjs';
const repo = resolve(import.meta.dirname, '../../../..');
const root = process.env.CF_PRESENT_EVIDENCE ? resolve(process.env.CF_PRESENT_EVIDENCE) : await mkdtemp(join(repo, 'target/comment-journey-'));
await mkdir(root, { recursive: true });
const project = join(root, 'project');
await mkdir(project, { recursive: true });
execFileSync('git', ['init', '--quiet'], { cwd: project });
const env = { ...process.env, XDG_STATE_HOME: join(root, 'state'), CF_PRESENT_OWNER_PID: String(process.pid) };
const run = args => execFileSync(codeflowBinary(repo), args, { cwd: project, env, encoding: 'utf8', timeout: 60_000 });
const declaration = JSON.parse(await readFile(join(repo, 'crates/codeflow-present/tests/fixtures/review-sequence.json')));
const input = join(root, 'document.json');
await writeFile(input, JSON.stringify({ schema_version: 2, title: 'Review the handoff', blocks: [
  { type: 'narrative', id: 'intro', markdown: '## Handoff\n\nReview the handoff before sending it.' },
  { type: 'figure', id: 'sequence', declaration },
] }));
const failures = [];
let passes = 0;
const check = async (name, fn) => { try { await fn(); passes++; console.log(`PASS ${name}`); } catch (e) { failures.push(`${name}: ${e.message}`); console.error(`FAIL ${name}: ${e.message}`); } };
const browser = await chromium.launch({ executablePath: await installedChrome(), headless: true });
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
try {
  for (const width of [1600, 375]) for (const theme of ['light', 'dark']) {
    const label = `${theme}-${width}`;
    const context = await browser.newContext({ viewport: { width, height: 1000 }, colorScheme: theme });
    const opened = run(['present', 'open', input, '--no-launch']);
    const session = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
    const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
    const page = await context.newPage();
    page.setDefaultTimeout(8000);
    let step = 0;
    const shot = async name => page.screenshot({ path: join(root, `${label}-${String(++step).padStart(2, '0')}-${name}.png`) });
    const verify = (name, fn) => check(`${label} ${name}`, fn);
    const hideNotes = async () => { if (await page.locator('.cf-feedback-close').isVisible()) await page.locator('.cf-feedback-close').click(); };
    const showNotes = async () => {
      if (await page.getByTestId('notes-dock').getAttribute('data-open') !== 'true') await page.getByTestId('comment-btn').click();
      if (width === 375 && await page.getByTestId('notes-dock').getAttribute('data-expanded') !== 'true') {
        if (await page.getByTestId('notes-expand').count()) await page.getByTestId('notes-expand').click();
        else await page.getByTestId('comment-btn').click();
      }
    };
    const save = async body => {
      if (!(await page.getByTestId('composer').count())) await page.getByTestId('float-comment').click();
      await page.getByTestId('composer-text').fill(body);
      await shot('write-note');
      await page.getByTestId('composer-save').click();
      await page.getByTestId('composer').waitFor({ state: 'detached' });
      await showNotes();
      await verify(`queued ${body}`, async () => assert.ok((await page.getByTestId('notes-list').innerText()).includes(body)));
      await shot('queued-note');
    };
    try {
      await page.goto(pathToFileURL(bootstrap).href);
      await page.locator('[data-cf-figure-block="ready"]').waitFor();
      await shot('read');
      await verify('Comment has a visible label at every width', async () => assert.ok(await page.getByTestId('comment-btn').locator('span').filter({ hasText: /^Comment$/u }).isVisible()));
      await page.getByTestId('comment-btn').click();
      await shot('enter-comment');
      await verify('mode and exit are explicit', async () => {
        assert.equal(await page.getByTestId('comment-btn').getAttribute('aria-pressed'), 'true');
        assert.equal(await page.getByTestId('comment-leave').innerText(), 'Exit Comment');
        if (width === 375) assert.ok(await page.getByTestId('notes-expand').isVisible(), 'collapsed notes need an explicit expand button');
      });
      await page.getByTestId('comment-leave').click();
      await verify('exit returns to reading', async () => assert.equal(await page.getByTestId('comment-btn').getAttribute('aria-pressed'), 'false'));
      await shot('leave-comment');
      await page.getByTestId('comment-btn').click();
      for (const kind of ['prose', 'figure-words', 'part', 'region', 'document']) {
        await hideNotes();
        const variant = width === 375 ? 'narrow' : 'wide';
        const drawing = page.locator(`svg[data-cf-variant="${variant}"]`);
        const author = drawing.locator('text').filter({ hasText: /^Author$/u });
        if (kind === 'prose') await drag(page, page.locator('#intro p'), 'Review the handoff');
        else if (kind === 'figure-words') await drag(page, author, 'Author');
        else if (kind === 'part') await author.click();
        else if (kind === 'region') {
          await drawing.scrollIntoViewIfNeeded();
          const b = await drawing.boundingBox();
          await page.mouse.move(b.x + b.width * .35, b.y + b.height * .72);
          await page.mouse.down();
          await page.mouse.move(b.x + b.width * .65, b.y + b.height * .92, { steps: 10 });
          await page.mouse.up();
        } else {
          await showNotes();
          await page.locator('.cf-tools summary').click();
          await page.getByTestId('tool-whole-doc').click();
        }
        await (kind === 'document' ? page.getByTestId('composer') : page.getByTestId('float-chip')).waitFor();
        await verify(`${kind} shows the captured target before writing`, async () => {
          const preview = kind === 'document' ? page.getByTestId('composer').locator('.q') : page.getByTestId('float-comment');
          const text = await preview.innerText();
          assert.match(text, kind === 'prose' ? /Text.*Review the handoff/s : kind === 'figure-words' ? /Text.*Author/s : kind === 'part' ? /Element.*Author/s : kind === 'region' ? /Area/s : /document/i);
          assert.equal(await page.getByTestId('composer-text').count() ? await page.getByTestId('composer-text').inputValue() : '', '');
        });
        await shot(`capture-${kind}`);
        await save(`${kind} note`);
      }
      await verify('queue has a count and explicit edit affordances', async () => {
        assert.equal(await page.getByTestId('note-row').count(), 5);
        assert.match(await page.locator('#cf-feedback-title').innerText(), /Queued notes/);
        assert.ok(await page.locator('#cf-feedback-title').evaluate(el => { const b = el.getBoundingClientRect(); return document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2) === el; }), 'queue heading must not be covered by the mode strip');
        assert.equal(await page.getByTestId('note-edit').count(), 5);
        assert.match(await page.getByTestId('comment-btn').innerText(), /5/);
      });
      const row = page.getByTestId('note-row').filter({ hasText: 'prose note' });
      if (await row.getByTestId('note-edit').count()) await row.getByTestId('note-edit').click();
      else await row.click();
      await verify('editing preserves the target', async () => assert.match(await page.getByTestId('composer').locator('.q').innerText(), /Review the handoff/));
      await page.getByTestId('composer-text').fill('Edited prose note');
      await shot('edit-note');
      await page.getByTestId('composer-save').click();
      await page.getByTestId('composer').waitFor({ state: 'detached' });
      const remove = page.getByTestId('note-row').filter({ hasText: 'part note' }).getByTestId('note-remove');
      await remove.focus();
      await page.keyboard.press('Enter');
      await verify('remove updates the queue', async () => assert.equal(await page.getByTestId('note-row').count(), 4));
      await shot('remove-note');
      await page.locator('#cf-review-verdict').selectOption('request_changes');
      await page.locator('.cf-verdict textarea').fill('Clarify the handoff.');
      await page.getByTestId('submit-all').scrollIntoViewIfNeeded();
      await shot('send-verdict');
      await page.getByTestId('submit-all').click();
      await page.getByTestId('toast').filter({ hasText: /Review received/ }).waitFor();
      await verify('sent confirmation remains visible outside the closed queue', async () => {
        await page.getByTestId('review-sent').waitFor();
        assert.ok(await page.getByTestId('review-sent').isVisible());
        assert.match(await page.getByTestId('review-sent').innerText(), /4 notes.*Request changes/s);
      });
      await shot('sent');
      const reviews = run(['present', 'responses', 'list', session, '--kind', 'review']).trim().split('\n').map(JSON.parse);
      await verify('service received the chosen verdict and edited queue', async () => {
        assert.equal(reviews.length, 1);
        assert.equal(reviews[0].verdict, 'request_changes');
        assert.equal(reviews[0].instruction, 'Clarify the handoff.');
        assert.equal(reviews[0].notes.length, 4);
        assert.ok(reviews[0].notes.some(n => n.body === 'Edited prose note' && n.selector.exact === 'Review the handoff'));
        assert.ok(reviews[0].notes.some(n => n.body === 'figure-words note' && n.selector.exact === 'Author'));
        assert.ok(reviews[0].notes.some(n => n.body === 'region note' && n.region_selector));
        assert.ok(!reviews[0].notes.some(n => n.body === 'part note'));
      });
    } catch (error) { failures.push(`${label} journey: ${error.message}`); await shot('failure'); }
    finally { await context.close(); run(['present', 'close', session]); }
  }
} finally {
  await browser.close();
  await writeFile(join(root, 'results.json'), JSON.stringify({ passes, failures }, null, 2));
  console.log(`Comment journey evidence: ${root}`);
}
assert.deepEqual(failures, []);
