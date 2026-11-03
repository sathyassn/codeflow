// Primary-owned browser proof. Requires a current codeflow binary and a built
// docs-portal/dist. It drives real stored settings; it never opens the gear.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium } from 'playwright-core';
import { installedChrome } from './browser-executables.mjs';
import { codeflowBinary } from './codeflow-binary.mjs';
import { serveBuiltSite } from '../../../../docs-portal/scripts/site-server.mjs';
import { settings, floors } from '../../../../assets/base/agents/skills/cf-present/resources/design-system/tokens.js';
const repo = resolve(import.meta.dirname, '../../../..');
const evidence = join(repo, 'target/tsk-1162-browser');
await mkdir(evidence, { recursive: true });
const scratch = await mkdtemp(join(evidence, 'session-')), project = join(scratch, 'project');
await mkdir(project);
const env = { ...process.env, XDG_STATE_HOME: join(scratch, 'state'), CF_PRESENT_OWNER_PID: String(process.pid) };
const run = args => execFileSync(codeflowBinary(repo), args, { cwd: project, env, encoding: 'utf8', timeout: 60000 });
const source = JSON.parse(await readFile(join(repo, 'assets/base/agents/skills/cf-present/resources/design-system/source/colour.json'), 'utf8'));
const config = JSON.parse(await readFile(join(repo, 'docs-portal/portal.config.json'), 'utf8'));
execFileSync('git', ['init', '--quiet'], { cwd: project });
const input = join(project, 'tokens.json');
await writeFile(input, JSON.stringify({ schema_version: 2, title: 'Shared theme tokens', blocks: [{ type: 'narrative', id: 'tokens', markdown: '## One design\n\nMode, theme and accent remain independent viewer choices.' }] }));
let session, browser, portal, kit;
try {
    const opened = run(['present', 'open', input, '--no-launch']);
    session = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
    assert.ok(session, 'session created');
    const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
    assert.ok(bootstrap, 'private bootstrap located');
    portal = await serveBuiltSite({ directory: join(repo, 'docs-portal/dist'), base: config.base });
    kit = await serveBuiltSite({ directory: join(repo, 'assets/base/agents/skills/cf-present/resources/design-system') });
    browser = await chromium.launch({ executablePath: await installedChrome(), headless: true });
    for (const surface of ['present', 'portal']) {
        const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
        await page.goto(surface === 'present' ? pathToFileURL(bootstrap).href : portal.origin + config.base);
        await page.waitForSelector(surface === 'present' ? '#cf-present-document' : 'main');
        const keys = surface === 'present' ? ['cf-present-theme', 'cf-present-mode', 'cf-present-accent'] : ['cf-portal-skin', 'starlight-theme', 'cf-portal-accent'];
        for (const [theme, options] of Object.entries(settings.themes))
            for (const mode of ['light', 'dark']) {
                let previous;
                for (const accent of options) {
                    await page.evaluate(({ keys, theme, mode, accent }) => { localStorage.setItem(keys[0], theme); localStorage.setItem(keys[1], mode); localStorage.setItem(keys[2], accent); }, { keys, theme, mode, accent });
                    await page.reload();
                    await page.waitForFunction(value => getComputedStyle(document.documentElement).getPropertyValue('--cf-accent').trim().toLowerCase() === value, source.accents[accent][mode]);
                    const actual = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--cf-accent').trim());
                    if (previous)
                        assert.notEqual(actual, previous, `${surface}/${theme}/${mode} accent selection changes the role`);
                    previous = actual;
                }
            }
        for (const mode of ['light', 'dark']) {
            await page.evaluate(({ keys, mode }) => { localStorage.setItem(keys[0], 'graphite'); localStorage.setItem(keys[1], mode); localStorage.setItem(keys[2], 'green'); }, { keys, mode });
            await page.reload();
            await page.screenshot({ path: join(evidence, `${surface}-graphite-${mode}.png`), fullPage: true });
        }
        for (const storedMode of surface === 'portal' ? ['system', 'auto'] : ['system']) {
            await page.evaluate(({ keys, storedMode }) => localStorage.setItem(keys[1], storedMode), { keys, storedMode });
            await page.emulateMedia({ colorScheme: 'light' });
            await page.reload();
            for (const colorScheme of ['light', 'dark']) {
                await page.emulateMedia({ colorScheme });
                await page.waitForFunction(value => getComputedStyle(document.documentElement).getPropertyValue('--cf-accent').trim().toLowerCase() === value, source.accents.green[colorScheme]);
            }
        }
        await page.close();
    }
    for (const theme of Object.keys(settings.themes)) {
        const exported = join(scratch, `${theme}-system.html`);
        run(['present', 'export', session, '--out', exported, '--theme', theme, '--mode', 'system']);
        for (const javaScriptEnabled of [false, true]) {
            const exportedPage = await browser.newPage({ javaScriptEnabled });
            await exportedPage.emulateMedia({ colorScheme: 'light' });
            await exportedPage.goto(pathToFileURL(exported).href);
            for (const colorScheme of ['light', 'dark']) {
                await exportedPage.emulateMedia({ colorScheme });
                const accent = await exportedPage.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--cf-accent').trim().toLowerCase());
                assert.equal(accent, source.accents.green[colorScheme], `${theme}/${colorScheme} system export, JavaScript ${javaScriptEnabled}`);
                if (theme === 'slate' && javaScriptEnabled) await exportedPage.screenshot({ path: join(evidence, `export-slate-${colorScheme}.png`), fullPage: true });
            }
            await exportedPage.close();
        }
    }
    const page = await browser.newPage();
    for (const width of [1280, 390]) {
        await page.setViewportSize({ width, height: 900 });
        await page.goto(kit.origin + '/counts.reference.html');
        await page.waitForSelector('#ring path[data-count="1"]');
        const bar = await page.locator('#bar rect[data-count="1"]').boundingBox();
        assert.ok(bar.width >= floors.minimumSegmentPx - .01);
        const arc = Number(await page.locator('#ring path[data-count="1"]').getAttribute('data-extent'));
        assert.ok(arc >= floors.minimumArcDegrees);
        assert.match(await page.locator('#ring title').textContent(), /review: 1/);
        await page.screenshot({ path: join(evidence, `positive-counts-${width}.png`), fullPage: true });
    }
    console.log('PASS: all theme/mode/accent stored settings, system and legacy modes, all-theme system exports with and without JavaScript, four default captures, positive bar/ring at 1280 and 390 px');
}
finally {
    await browser?.close();
    await portal?.close();
    await kit?.close();
    if (session)
        run(['present', 'close', session]);
    await rm(scratch, { recursive: true, force: true });
}
