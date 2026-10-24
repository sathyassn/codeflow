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
    const context = await browser.newContext({ viewport: { width, height: width === 375 ? 812 : 900 }, colorScheme: theme, hasTouch: width === 375 });
    const opened = run(['present', 'open', input, '--no-launch']);
    const session = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
    const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
    const page = await context.newPage();
    page.setDefaultTimeout(8000);
    let step = 0;
    const shot = async name => page.screenshot({ path: join(root, `${label}-${String(++step).padStart(2, '0')}-${name}.png`) });
    const verify = (name, fn) => check(`${label} ${name}`, fn);
    const hideNotes = async () => { if (width === 375 && await page.getByTestId('notes-dock').getAttribute('data-expanded') === 'true') await page.getByTestId('notes-expand').click(); };
    const showNotes = async () => {
      if (await page.getByTestId('notes-dock').getAttribute('data-open') !== 'true') await page.getByTestId('comment-btn').click();
      if (width === 375 && await page.getByTestId('notes-dock').getAttribute('data-expanded') !== 'true') {
        if (await page.getByTestId('notes-expand').count()) await page.getByTestId('notes-expand').click();
        else await page.getByTestId('comment-btn').click();
      }
    };
    let capturedWords = null;
    const save = async body => {
      if (!(await page.getByTestId('composer').count())) await page.getByTestId('float-comment').click();
      await page.keyboard.type(body);
      await verify('first keystroke reaches the focused editor', async () => assert.equal(await page.getByTestId('composer-text').inputValue(), body));
      await page.keyboard.press('Escape');
      await verify('Escape keeps typed words until discard is confirmed', async () => {
        assert.ok(await page.getByTestId('discard-prompt').isVisible());
        assert.equal(await page.getByTestId('composer-text').inputValue(), body);
      });
      await verify('discard confirmation and save stay on screen', async () => {
        const b = await page.getByTestId('composer').boundingBox(); assert.ok(b.y >= 0 && b.y + b.height <= (width === 375 ? 812 : 900));
      });
      await shot('confirm-discard');
      await page.getByRole('button', { name: 'Keep writing', exact: true }).click();
      await verify('editor leaves the captured words visible', async () => {
        const overlap = await page.getByTestId('composer').evaluate((el, target) => {
          const editor = el.getBoundingClientRect();
          if (!target) return false;
          return editor.left < target.right && target.left < editor.right && editor.top < target.bottom && target.top < editor.bottom;
        }, capturedWords);
        assert.equal(overlap, false);
      });
      await verify('new note editor is titled as a note', async () => assert.equal(await page.getByTestId('composer').locator('.title').innerText(), 'NEW NOTE'));
      if (capturedWords) await verify('captured words stay marked while writing', async () => {
        const marks = await page.getByTestId('pending-text').evaluateAll(els => els.map(el => { const b = el.getBoundingClientRect(); return { left: b.left, right: b.right, top: b.top, bottom: b.bottom }; }));
        assert.ok(marks.length > 0);
        assert.ok(marks.some(m => m.left < capturedWords.right && capturedWords.left < m.right && m.top < capturedWords.bottom && capturedWords.top < m.bottom));
      });
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
      await verify('verdict requires an explicit choice', async () => assert.equal(await page.locator('#cf-review-verdict').inputValue(), ''));
      await showNotes();
      await verify('whole document is directly reachable', async () => assert.ok(await page.getByTestId('tool-whole-doc').isVisible()));
      await verify('header is opaque and title stays within its column', async () => assert.ok(await page.locator('.cf-topbar').evaluate(el => {
        const title = el.querySelector('.cf-title-group').getBoundingClientRect();
        const mark = el.querySelector('.cf-brand-mark').getBoundingClientRect();
        const bg = getComputedStyle(el).backgroundColor;
        return title.left >= mark.right && !bg.includes(' / ') && !bg.startsWith('rgba');
      })));

      await verify('mode and exit are explicit', async () => {
        assert.equal(await page.getByTestId('comment-btn').getAttribute('aria-pressed'), 'true');
        assert.equal(await page.getByTestId('comment-leave').innerText(), 'Done commenting');
        if (width === 375) assert.ok(await page.getByTestId('notes-expand').isVisible(), 'collapsed notes need an explicit expand button');
      });
      await page.getByTestId('comment-leave').click();
      await verify('exit returns to reading', async () => assert.equal(await page.getByTestId('comment-btn').getAttribute('aria-pressed'), 'false'));
      await shot('leave-comment');
      await page.getByTestId('comment-btn').click();
      await hideNotes();
      await drag(page, page.locator('#intro p'), 'Review the h');
      await page.getByTestId('float-chip').waitFor();
      await verify('partial words are visibly abbreviated', async () => assert.match(await page.getByTestId('float-comment').innerText(), /h…/));
      await shot('partial-word');
      await page.getByTestId('float-esc').click();
      await showNotes();
      await page.getByTestId('tool-select-area').click();
      await verify('area mode is visible after the phone sheet closes', async () => {
        assert.match(await page.getByTestId('comment-hint').innerText(), /Select area is on/);
        if (width === 375) { assert.doesNotMatch(await page.getByTestId('comment-hint').innerText(), /Shift/); assert.equal(await page.getByTestId('notes-dock').getAttribute('data-expanded'), 'false'); }
      });
      await shot('armed-area');
      await page.getByTestId('capture-cancel').click();
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
          if (!(await page.getByTestId('tool-whole-doc').isVisible())) await page.locator('.cf-tools summary').click();
          const control = await page.getByTestId('tool-whole-doc').boundingBox();
          await page.getByTestId('tool-whole-doc').click();
          await page.getByTestId('composer').waitFor();
          await verify('whole-document editor opens beside its button', async () => {
            const editor = await page.getByTestId('composer').boundingBox();
            const gap = Math.max(editor.y - (control.y + control.height), control.y - (editor.y + editor.height));
            assert.ok(gap < 40, `gap ${gap}`);
            assert.equal(await page.getByTestId('composer').locator('.q').innerText(), 'Whole document');
          });
        }
        await (kind === 'document' ? page.getByTestId('composer') : page.getByTestId('float-chip')).waitFor();
        capturedWords = kind === 'prose' || kind === 'figure-words' ? await page.evaluate(() => {
          const b = getSelection().getRangeAt(0).getBoundingClientRect(); return { left: b.left, right: b.right, top: b.top, bottom: b.bottom };
        }) : null;
        await verify(`${kind} shows the captured target before writing`, async () => {
          const preview = kind === 'document' ? page.getByTestId('composer').locator('.q') : page.getByTestId('float-comment');
          const text = await preview.innerText();
          assert.match(text, kind === 'prose' ? /Text.*Review the handoff/s : kind === 'figure-words' ? /Text.*Author/s : kind === 'part' ? /Element.*Author/s : kind === 'region' ? /Area/s : /document/i);
          assert.equal(await page.getByTestId('composer-text').count() ? await page.getByTestId('composer-text').inputValue() : '', '');
        });
        await verify(`${kind} capture leaves phone document visible`, async () => {
          if (width === 375 && kind !== 'document') assert.equal(await page.getByTestId('notes-dock').getAttribute('data-expanded'), 'false');
          if (kind === 'document') assert.doesNotMatch(await page.getByTestId('composer').locator('.q').innerText(), /Area/);
        });
        await verify('capture chip leaves selected words visible', async () => {
          if (!capturedWords) return;
          const b = await page.getByTestId('float-chip').boundingBox();
          assert.ok(!(b.x < capturedWords.right && capturedWords.left < b.x + b.width && b.y < capturedWords.bottom && capturedWords.top < b.y + b.height));
        });
        await shot(`capture-${kind}`);
        await save(`${kind} note`);
        await hideNotes();
        await shot(`saved-target-${kind}`);
        if (kind === 'prose') await verify('note number leaves the complete prose line readable', async () => {
          const marker = await page.getByTestId('note-marker').first().boundingBox();
          const line = await page.locator('#intro p').boundingBox();
          assert.ok(!(marker.x < line.x + line.width && line.x < marker.x + marker.width && marker.y < line.y + line.height && line.y < marker.y + marker.height));
        });
        if (kind === 'part') await verify('saved part stays marked', async () => assert.ok(await page.getByTestId('saved-part').count() > 0));
        await showNotes();
        if (kind === 'prose' || kind === 'figure-words') await verify(`${kind} saved words stay marked`, async () => assert.ok(await page.getByTestId('saved-text').count() > 0));
        await verify('marker is explicitly a note number', async () => assert.match(await page.getByTestId('note-marker').last().innerText(), /#/));
      }
      await verify('queue has a count and explicit edit affordances', async () => {
        assert.equal(await page.getByTestId('note-row').count(), 5);
        assert.match(await page.locator('#cf-feedback-title').innerText(), /Draft notes/);
        assert.ok(await page.locator('#cf-feedback-title').evaluate(el => { const b = el.getBoundingClientRect(); return document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2) === el; }), 'queue heading must not be covered by the mode strip');
        assert.equal(await page.getByTestId('note-edit').count(), 5);
        assert.match(await page.getByTestId('comment-btn').innerText(), /5/);
      });
      await verify('note rows name the target once, without a repeated kind', async () => {
        assert.equal(await page.getByTestId('note-row').filter({ hasText: 'prose note' }).locator('.a').innerText(), 'Review the handoff');
        assert.equal(await page.getByTestId('note-row').filter({ hasText: 'document note' }).locator('.a').innerText(), 'Whole document');
      });
      await verify('a list taller than its panel says more notes are below', async () => {
        const list = page.getByTestId('notes-list');
        const overflow = await list.evaluate(el => el.scrollHeight > el.clientHeight + 2);
        assert.equal(await list.getAttribute('data-more-below'), overflow ? 'true' : 'false');
        if (overflow) {
          await shot('more-notes-below');
          await list.evaluate(el => { el.scrollTop = el.scrollHeight; });
          await page.waitForFunction(() => document.querySelector('[data-testid=notes-list]').dataset.moreBelow === 'false');
          await list.evaluate(el => { el.scrollTop = 0; });
        }
      });
      if (width === 375) await verify('phone sheet controls have distinct plain labels', async () => {
        assert.equal(await page.getByTestId('notes-expand').innerText(), 'Back to document');
        assert.equal(await page.getByText('Hide notes', { exact: true }).count(), 0);
        await page.getByTestId('notes-expand').click();
        assert.equal(await page.getByTestId('notes-expand').innerText(), 'Show notes');
        assert.equal(await page.getByTestId('comment-leave').innerText(), 'Done commenting');
        await page.getByTestId('notes-expand').click();
      });
      const row = page.getByTestId('note-row').filter({ hasText: 'prose note' });
      if (await row.getByTestId('note-edit').count()) await row.getByTestId('note-edit').click();
      else await row.click();
      await verify('editing preserves the target', async () => assert.match(await page.getByTestId('composer').locator('.q').innerText(), /Review the handoff/));
      await page.getByTestId('composer-text').fill('Edited prose note');
      await shot('edit-note');
      await page.getByTestId('composer-save').click();
      await page.getByTestId('composer').waitFor({ state: 'detached' });
      await verify('cancelling an unchanged edit closes without a discard question', async () => {
        await page.getByTestId('note-row').filter({ hasText: 'Edited prose note' }).getByTestId('note-edit').click();
        await page.getByTestId('composer-cancel').click();
        await page.getByTestId('composer').waitFor({ state: 'detached', timeout: 5000 });
      });
      const remove = page.getByTestId('note-row').filter({ hasText: 'part note' }).getByTestId('note-remove');
      await remove.focus();
      await page.keyboard.press('Enter');
      await verify('remove updates the queue', async () => assert.equal(await page.getByTestId('note-row').count(), 4));
      await shot('remove-note');
      await verify('removed note can be restored', async () => {
        await page.getByTestId('undo-remove').click();
        assert.equal(await page.getByTestId('note-row').count(), 5);
        await shot('undo-remove');
        await page.getByTestId('note-row').filter({ hasText: 'part note' }).getByTestId('note-remove').click();
      });
      await page.locator('#cf-review-verdict').selectOption('request_changes');
      await page.locator('.cf-verdict textarea').fill('Clarify the handoff.');
      await verify('verdict and submit remain reachable without scrolling the sheet', async () => {
        for (const selector of ['#cf-review-verdict', '#submitAllBtn']) assert.ok(await page.locator(selector).evaluate(el => { const b = el.getBoundingClientRect(); return b.top >= 0 && b.bottom <= innerHeight; }));
      });
      await shot('send-verdict');
      const scrollBeforeSend = await page.evaluate(() => scrollY);
      await page.getByTestId('submit-all').click();
      await page.getByTestId('review-sent').waitFor();
      await verify('sent confirmation remains visible outside the closed queue', async () => {
        await page.getByTestId('review-sent').waitFor();
        assert.ok(await page.getByTestId('review-sent').isVisible());
        assert.match(await page.getByTestId('review-sent').innerText(), /4 notes.*Request changes/s);
      });
      await verify('sent review opens history and resets the verdict', async () => {
        assert.equal(await page.locator('#cf-review-verdict').inputValue(), '');
        assert.ok(await page.getByTestId('feedback-history').evaluate(el => el.open));
        assert.ok(Math.abs(await page.evaluate(() => scrollY) - scrollBeforeSend) < 2, 'sending must not scroll the document');
        assert.match(await page.getByTestId('review-sent').innerText(), /Earlier feedback/);
      });
      await verify('sent state sits in the notes panel above Earlier feedback', async () => {
        assert.equal(await page.getByTestId('notes-dock').getByTestId('review-sent').count(), 1);
        const sent = await page.getByTestId('review-sent').boundingBox();
        const history = await page.getByTestId('feedback-history').boundingBox();
        assert.ok(sent.y + sent.height <= history.y + 1);
        assert.match(await page.getByTestId('notes-dock').locator('#dockCount').innerText(), /^0$/u);
        const shown = await page.getByTestId('feedback-history').evaluate(el => el.getBoundingClientRect().height);
        assert.ok(shown >= 120, `Earlier feedback shows ${shown} px`);
        assert.equal(await page.getByTestId('comment-btn').getAttribute('aria-pressed'), 'true');
      });
      await shot('sent');
      await page.getByTestId('comment-leave').click();
      await verify('sent state stays visible after leaving Comment mode', async () => assert.match(await page.getByTestId('review-sent').innerText(), /Review sent: 4 notes/));
      await page.evaluate(() => scrollTo(0, 260));
      await verify('scrolled content never shows through the header', async () => assert.ok(await page.locator('.cf-topbar').evaluate(el => {
        const bg = getComputedStyle(el).backgroundColor;
        return !bg.startsWith('rgba') && getComputedStyle(el).backdropFilter === 'none';
      })));
      await shot('sent-reading');
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
