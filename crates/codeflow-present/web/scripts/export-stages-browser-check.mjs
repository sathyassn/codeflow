// Run by the primary: this check starts the real session service.
// Optional --document PATH uses the design brief's status report for captures.
import assert from "node:assert/strict";
import { assertStageTextPainted } from "./export-paint.mjs";
import { prepareExportCapture } from "./export-capture.mjs";
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";
import { installedChrome } from "./browser-executables.mjs";
import { codeflowBinary } from "./codeflow-binary.mjs";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const binary = codeflowBinary(repo);
const root = await mkdtemp(join(tmpdir(), "tsk1101-captures-"));
const project = join(root, "project");
const env = { PATH: process.env.PATH, HOME: join(root, "home"), TMPDIR: join(root, "tmp"), XDG_STATE_HOME: join(root, "state"), LANG: "C.UTF-8", CF_PRESENT_OWNER_PID: String(process.pid) };
for (const folder of [project, env.HOME, env.TMPDIR, env.XDG_STATE_HOME]) await mkdir(folder, { recursive: true });
execFileSync("git", ["init", "--quiet"], { cwd: project, env });
const cli = (args) => execFileSync(binary, args, { cwd: project, env, encoding: "utf8", timeout: 60_000 });
const fixture = { schema_version: 2, title: "Export stage geometry", blocks: [
  { type: "html", id: "tall", title: "Nine lanes", caption: "All nine lanes remain visible.", html: `<div>${Array.from({ length: 9 }, (_, i) => `<div style="padding:32px 12px;border:1px solid">Lane ${i + 1}</div>`).join("")}</div>` },
  { type: "html", id: "short", title: "Short stage", caption: "One paragraph.", html: "<p>A short stage</p>" },
  { type: "html", id: "square", title: "Width-dependent stage", caption: "A wider square also grows taller.", html: '<div style="aspect-ratio:1;background:gray">A square</div>' },
  { type: "html", id: "wrapping", title: "Wrapping stage", caption: "Independent of the review rail.", html: `<p>${"Wrapping content must not inherit the review rail geometry. ".repeat(30)}</p>` },
] };
const argument = process.argv.indexOf("--document");
const documentPath = join(project, "stages.json");
await writeFile(documentPath, argument < 0 ? JSON.stringify(fixture) : await readFile(resolve(process.argv[argument + 1])));
// Replay the original stylesheet or renderer for fail-before evidence without
// building or mutating another worktree. Normal runs use the complete new build.
async function exportCapture(path, mode) {
  cli(["present", "export", id, "--out", path, "--theme", "slate", "--mode", mode]);
  let html = await readFile(path, "utf8");
  const from = (flag) => { const index = process.argv.indexOf(flag); return index < 0 ? null : process.argv[index + 1]; };
  const stylesFrom = from("--styles-from");
  if (stylesFrom) {
    const css = execFileSync("git", ["show", `${stylesFrom}:crates/codeflow-present/web/src/styles.css`], { cwd: repo, encoding: "utf8" });
    html = html.replace(/(<style data-cf-present-export-style="true">)[\s\S]*?(<\/style>)/u, (_, open, close) => `${open}${css}${close}`);
  }
  const rendererFrom = from("--renderer-from");
  if (rendererFrom) {
    const manifest = JSON.parse(execFileSync("git", ["show", `${rendererFrom}:crates/codeflow-present/assets/manifest.json`], { cwd: repo, encoding: "utf8" }));
    const asset = manifest.export["present.export"].stored_path;
    const bytes = execFileSync("git", ["show", `${rendererFrom}:crates/codeflow-present/assets/${asset}`], { cwd: repo });
    html = html.replace(/(<template id="cf-present-export-payload">)[^<]*(<\/template>)/u, (_, open, close) => `${open}${bytes.toString("base64")}${close}`);
  }
  await writeFile(path, html);
}
let id; let browser;
try {
  const opened = cli(["present", "open", documentPath, "--no-launch"]);
  id = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
  const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
  assert.ok(id && bootstrap, opened);
  browser = await chromium.launch({ executablePath: await installedChrome(), headless: true });
  const context = await browser.newContext({ viewport: { width: 1200, height: 900 } });
  const page = await context.newPage();
  const never = join(root, "never-opened.html");
  await exportCapture(never, "light");
  await page.goto(pathToFileURL(never).href);
  await page.waitForSelector("html[data-cf-ready='true']");
  const stageCount = await page.locator("iframe[data-cf-export-stage]").count();
  assert.ok(stageCount > 0);
  assert.equal(await page.locator("[data-cf-stage-notice]:visible").count(), stageCount);
  await page.screenshot({ path: join(root, "never-opened.png"), fullPage: true });
  const reports = [];
  page.on("request", (request) => { if (request.url().endsWith("/app/api/stages/measure")) reports.push(JSON.parse(request.postData())); });
  await page.goto(pathToFileURL(bootstrap).href, { waitUntil: "commit" });
  await page.waitForURL(/http:\/\/127\.0\.0\.1:\d+\/app\//u);
  await page.waitForSelector("html[data-cf-stages-measured='true']", { timeout: 120_000 });
  const closedRail = reports.splice(0);
  const manifest = JSON.parse(await readFile(join(repo, "crates/codeflow-present/assets/manifest.json"), "utf8"));
  const measuringAsset = manifest.service.assets.find((asset) => asset.request_path.includes("chunk-stage-measuring-")).request_path;
  await page.evaluate(async (asset) => {
    document.querySelector(".cf-chrome-frame").dataset.rail = "open";
    delete document.documentElement.dataset.cfStagesMeasured;
    await (await import(asset)).measureExportStages();
  }, measuringAsset);
  assert.deepEqual(reports, closedRail, "rail state must not change export measurements");
  for (const mode of ["light", "dark"]) {
    const exported = join(root, `measured-${mode}.html`);
    await exportCapture(exported, mode);
    const capture = await context.newPage();
    for (const [width, scrollbar] of [[1200, false], [400, false], [1366, false], [1200, true]]) {
      await capture.setViewportSize({ width, height: 900 });
      await capture.goto(pathToFileURL(exported).href);
      await capture.waitForSelector("html[data-cf-ready='true']");
      if (scrollbar) {
        // Model the classic scrollbar's 15px loss from the document column.
        await capture.evaluate(() => { document.getElementById("cf-present-document").style.inlineSize = "calc(100% - 15px)"; });
        await capture.waitForFunction(() => [...document.querySelectorAll("iframe[data-cf-export-stage]")].every((frame) => frame.dataset.cfStageMeasuredWidth === "768"));
      }
      assert.equal(await capture.locator("[data-cf-stage-notice]:visible").count(), 0);
      for (const handle of await capture.locator("iframe[data-cf-export-stage]").elementHandles()) {
        assert.equal(await handle.getAttribute("sandbox"), "");
        assert.equal(await handle.getAttribute("scrolling"), "no");
        if (width === 1366) assert.equal(await handle.getAttribute("data-cf-stage-measured-width"), "900");
        assert.equal(await handle.getAttribute("data-cf-stage-status"), "loaded");
        const frame = await handle.contentFrame();
        assert.ok(frame);
        assert.equal(await frame.locator("script").count(), 0);
        const geometry = await frame.evaluate(async () => {
          await document.fonts.ready;
          // The root's normal-flow bounds include margins collapsed through
          // body (for example a paragraph's 16px margin over body's 8px).
          // body.bottom + body.marginBottom undercounts that occupied space.
          const bottom = document.documentElement.getBoundingClientRect().bottom;
          return { height: innerHeight, scroll: document.documentElement.scrollHeight, bottom };
        });
        assert.ok(geometry.scroll <= geometry.height + 1, JSON.stringify(geometry));
        assert.ok(geometry.height <= Math.max(24, geometry.bottom) + 2, JSON.stringify(geometry));
      }
      if (!process.argv.includes("--without-capture-preparation")) await prepareExportCapture(capture);
      const screenshot = await capture.screenshot({ path: join(root, `${mode}-${width}${scrollbar ? "-scrollbar" : ""}.png`), fullPage: true });
      await assertStageTextPainted(capture, screenshot);
    }
    await capture.setViewportSize({ width: 350, height: 900 });
    await capture.goto(pathToFileURL(exported).href);
    await capture.waitForSelector("html[data-cf-ready='true']");
    assert.equal(await capture.locator("[data-cf-stage-notice]:visible").count(), stageCount);
    await capture.close();
  }
  const failed = await context.newPage();
  await failed.addInitScript(() => {
    const original = EventTarget.prototype.addEventListener;
    EventTarget.prototype.addEventListener = function(type, listener, options) {
      if (this instanceof HTMLIFrameElement && (type === "load" || type === "error")) return;
      return original.call(this, type, listener, options);
    };
  });
  await failed.goto(pathToFileURL(join(root, "measured-light.html")).href);
  await failed.waitForSelector("html[data-cf-ready='true']", { timeout: 15_000 });
  assert.equal(await failed.locator("iframe[data-cf-stage-status='failed']").count(), stageCount);
  assert.equal(await failed.getByText("Stage did not load; reopen this capture or open the session.", { exact: true }).count(), stageCount);
  await failed.screenshot({ path: join(root, "failed-frames.png"), fullPage: true });
  console.log(`PASS: export geometry, sandbox, rail independence, readiness and captures in ${root}`);
} finally {
  await browser?.close();
  if (id) cli(["present", "close", id]);
  console.log(`Evidence retained at ${root}`);
}
