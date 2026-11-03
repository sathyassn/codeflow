// Capture the installed report journey for the design seat. Takes its JSON path.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium } from 'playwright-core';
import { installedChrome } from './browser-executables.mjs';
import { codeflowBinary } from './codeflow-binary.mjs';
const repo = resolve(import.meta.dirname, '../../../..');
const documentPath = process.argv[2];
assert.ok(documentPath, 'usage: node scripts/report-scale-browser-check.mjs <report.json>');
const root = process.env.CF_PRESENT_EVIDENCE ? resolve(process.env.CF_PRESENT_EVIDENCE) : await mkdtemp(join(repo, 'target/report-scale-'));
await mkdir(root, { recursive: true });
const project = join(root, 'project');
await mkdir(project, { recursive: true });
execFileSync('git', ['init', '--quiet'], { cwd: project });
const env = { ...process.env, XDG_STATE_HOME: join(root, 'state'), CF_PRESENT_OWNER_PID: String(process.pid) };
const run = args => execFileSync(codeflowBinary(repo), args, { cwd: project, env, encoding: 'utf8', timeout: 60_000 });
const browser = await chromium.launch({ executablePath: await installedChrome(), headless: true });
let session;
const measurements = [];
try {
  const opened = run(['present', 'open', resolve(documentPath), '--no-launch']);
  session = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
  const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
  assert.ok(session && bootstrap, opened);
  for (const width of [1200, 400]) {
    const page = await browser.newPage({ viewport: { width, height: 1000 } });
    await page.goto(pathToFileURL(bootstrap).href);
    await page.locator('#cf-present-document').waitFor();
    for (const mode of ['light', 'dark']) for (const scale of ['default', 'large']) {
      await page.getByTestId('settings-btn').click();
      const panel = page.getByTestId('settings-panel');
      await panel.getByRole('button', { name: mode === 'light' ? 'Light' : 'Dark', exact: true }).click();
      await panel.locator(`[data-scale="${scale}"]`).click();
      await page.keyboard.press('Escape');
      await page.evaluate(async () => { await document.fonts.ready; });
      measurements.push({ width, mode, scale, text: await page.locator('.cf-stage-host').evaluateAll(stages => stages.flatMap(stage => [...stage.querySelectorAll('*')].filter(el => [...el.childNodes].some(n => n.nodeType === Node.TEXT_NODE && n.textContent.trim())).map(el => ({ text: el.textContent.trim().slice(0, 80), px: parseFloat(getComputedStyle(el).fontSize) })))) });
      await page.screenshot({ path: join(root, `${width}-${mode}-${scale}.png`), fullPage: true });
    }
    await page.close();
  }
  await writeFile(join(root, 'stage-type-measurements.json'), JSON.stringify(measurements, null, 2) + '\n');
  console.log(`Report screenshots and stage measurements: ${root}. Design verdict still required.`);
} finally {
  await browser.close();
  if (session) run(['present', 'close', session]);
}
