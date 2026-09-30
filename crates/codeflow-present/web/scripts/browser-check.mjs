import assert from "node:assert/strict";
import { createServer } from "node:http";
import { access, mkdtemp, readdir, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { chromium, firefox, webkit } from "playwright-core";
import { checkResolverRules, checkSelectionOccurrences } from "./selection-browser-check.mjs";
import { checkDocumentExcerpts, checkEntityCrops, checkCropBudget } from "./excerpt-browser-check.mjs";
import { checkSelectionLifecycle } from "./selection-lifecycle-browser-check.mjs";
import { checkIframeComments } from "./iframe-comment-browser-check.mjs";
import { assertNoPolicyViolations, recordPolicyViolations } from "./csp-violations.mjs";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const assetsRoot = resolve(webRoot, "../assets");
const repoRoot = resolve(webRoot, "../../..");
// A figure block the grammar draws with no rule failure (the portal's state specimen).
const figureDeclaration = await readFile(join(repoRoot, "docs-portal/tests/fixtures/figures/05-state.json"), "utf8");
const manifest = JSON.parse(await readFile(join(assetsRoot, "manifest.json"), "utf8"));
const assets = new Map(manifest.service.assets.map((asset) => [asset.request_path, asset]));
const appPath = manifest.service.entrypoints["present.app"];
const stylePath = manifest.service.entrypoints["present.style"];
const prepaint = manifest.service.inline["present.prepaint"].source;
const sourceStyles = await readFile(join(webRoot, "src/styles.css"), "utf8");
const exportFallback = await readFile(join(webRoot, "src/export-fallback.css"), "utf8");
const projectUtilityCss = ":root[data-cf-theme]{--cf-reading-measure:68ch;}";
const applicationCsp = `default-src 'none'; script-src 'self' '${manifest.service.inline["present.prepaint"].csp_sha256}'; style-src 'self'; style-src-elem 'self' 'unsafe-inline'; style-src-attr 'unsafe-inline'; font-src data:; img-src data: blob:; media-src data:; connect-src 'self'; frame-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`;
const reviewPosts = [];
const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    if (url.pathname === "/app") {
      response.writeHead(200, {
        "Content-Type": "text/html; charset=utf-8",
        "Cache-Control": "no-store",
        "Content-Security-Policy": applicationCsp,
      });
      const fixture = url.searchParams.get("case");
      // The submit race needs room for four short notes and a body limit a
      // crop does not fit in, so fitting re-encodes it.
      const limits = fixture === "race" ? { max_notes: 4, max_text_utf16: 200, max_selector_utf16: 64, max_payload_bytes: 1500 } : {};
      response.end(fixtureHtml(["prose", "selection", "iframe"].includes(fixture), fixture === "selection", fixture === "iframe", limits));
      return;
    }
    if (url.pathname === "/export") {
      response.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" });
      response.end(exportFixture(url.searchParams.get("mode") ?? "system"));
      return;
    }
    if (url.pathname === "/sandbox/1/fixture") {
      response.writeHead(200, {
        "Content-Type": "text/html; charset=utf-8",
        "Content-Security-Policy": "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; connect-src 'none'; form-action 'none'",
      });
      response.end('<p style="color:#123">Static sandbox content</p>');
      return;
    }
    if (url.pathname === "/sandbox/1/attack") {
      response.writeHead(200, {
        "Content-Type": "text/html; charset=utf-8",
        "Content-Security-Policy": "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; connect-src 'none'; form-action 'none'",
      });
      response.end(`<meta http-equiv="refresh" content="0;url=https://example.invalid/escape">
        <img src="https://example.invalid/pixel.png" alt="blocked">
        <svg><animate attributeName="href" to="https://example.invalid/animate"/></svg>
        <p>Attack sandbox stayed local</p>`);
      return;
    }
    const asset = assets.get(url.pathname);
    if (asset) {
      response.writeHead(200, {
        "Content-Type": asset.media_type,
        "Content-Encoding": "br",
        "Cache-Control": "public,max-age=31536000,immutable",
        ETag: asset.etag,
        Vary: "Accept-Encoding",
        "X-Content-Type-Options": "nosniff",
      });
      response.end(await readFile(join(assetsRoot, asset.stored_path)));
      return;
    }
    if (url.pathname === "/app/api/events/poll" && request.method === "POST") {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end('{"cursor":"end","kind":"session_closed","message":"Fixture session closed."}');
      return;
    }
    if (url.pathname === "/app/api/reviews" && request.method === "POST") {
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      reviewPosts.push(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end('{"event_id":"evt-browser-check","state":"received"}');
      return;
    }
    response.writeHead(404, { "Content-Type": "text/plain" });
    response.end("not found");
  } catch (error) {
    response.writeHead(500, { "Content-Type": "text/plain" });
    response.end(error instanceof Error ? error.message : "fixture error");
  }
});

await new Promise((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const address = server.address();
if (!address || typeof address === "string") throw new Error("Browser-check server did not bind TCP");
const origin = `http://127.0.0.1:${address.port}`;
// Every browser this check launches keeps its profile in a directory the
// check owns, never the operator's, and each close must empty its part
// (TSK-096 AC-4). Playwright makes the profiles under the temp directory.
const browserHome = await mkdtemp(join(tmpdir(), "cf-present-browsers-"));
process.env.TMPDIR = browserHome;
async function closeOwned(name, instance) {
  await instance.close();
  const left = (await readdir(browserHome)).filter((entry) => entry.startsWith(`playwright_${name}dev_profile`));
  if (left.length) throw new Error(`${name}: its profile outlived the browser: ${left.join(", ")}`);
  process.stdout.write(`${name}: teardown passed: headless browser closed, its own profile under ${browserHome} removed\n`);
}
const executablePath = await findBrowser();
const browser = await chromium.launch({ executablePath, headless: true });

try {
  for (const [name, engine] of [['chromium', chromium], ['firefox', firefox], ['webkit', webkit]]) {
    const chromeBrowser = name === 'chromium' ? browser : await engine.launch();
    try { await checkSavedAppearance(chromeBrowser, origin, name); }
    finally { if (chromeBrowser !== browser) await closeOwned(name, chromeBrowser); }
  }
  await checkSelectionOccurrences(browser);
  await checkResolverRules(browser);
  await checkDocumentExcerpts(browser);
  await checkEntityCrops(browser);
  await checkCropBudget(browser);
  await checkProseLazyPath(browser, origin);
  await checkSelectionLifecycle(browser, origin);
  await checkIframeComments(browser, origin);
  await checkSubmitRace(browser, origin, reviewPosts);
  await checkInteractiveSurface(browser, origin, reviewPosts);
  await checkPhoneSurface(browser, origin);
  // TSK-096: the same interactive cases, chip included, in other engines on
  // request (CF_PRESENT_CHIP_ENGINES=firefox,webkit), each in its own browser.
  for (const name of (process.env.CF_PRESENT_CHIP_ENGINES ?? "").split(",").filter(Boolean)) {
    const engine = { firefox, webkit }[name];
    if (!engine) throw new Error(`Unknown engine ${name}`);
    const other = await engine.launch();
    try {
      reviewPosts.length = 0;
      await checkInteractiveSurface(other, origin, reviewPosts);
      process.stdout.write(`${name}: interactive surface and comment chip cases passed\n`);
    } finally { await closeOwned(name, other); }
  }
  await checkStaticExportModes(browser, origin);
  process.stdout.write("cf-present browser checks passed: lazy paths, interactive and no-script modes, selection, figure blocks, zero CSP violations, axe, and 320 px reflow\n");
} finally {
  await closeOwned("chromium", browser);
  await rm(browserHome, { recursive: true, force: true });
  await new Promise((resolve) => server.close(resolve));
}

async function checkSavedAppearance(browser, origin, engine) {
  for (const [key, saved, skin, face] of [
    ...[['instrument', 'graphite'], ['editorial', 'slate'], ['ink', 'sage'], ['technical', 'graphite']].map(([old, skin]) => ['cf-present-theme', old, skin, 'plex']),
    ...[['instrument', 'archivo'], ['editorial', 'inter'], ['plex', 'plex']].map(([old, face]) => ['cf-present-typeface', old, 'sage', face]),
  ]) {
    const context = await browser.newContext();
    try {
      await context.addInitScript(({ key, saved }) => {
        localStorage.setItem('cf-present-theme', 'sage'); localStorage.setItem('cf-present-typeface', 'plex'); localStorage.setItem(key, saved);
        window.__appearancePaints = [];
        new MutationObserver(() => {
          const root = document.documentElement;
          if (root?.dataset.cfTheme && root.dataset.cfTypeface) window.__appearancePaints.push([root.dataset.cfTheme, root.dataset.cfTypeface]);
        }).observe(document, { subtree: true, childList: true, attributes: true });
      }, { key, saved });
      const page = await context.newPage();
      await page.goto(`${origin}/app?case=prose`);
      await page.getByTestId('settings-btn').click();
      const paints = await page.evaluate(() => window.__appearancePaints);
      assert.ok(paints.length && paints.every(p => p[0] === skin && p[1] === face), `${engine}: ${key}=${saved} first paint`);
      assert.equal(await page.locator(`[data-testid=skin-pills] [data-skin=${skin}]`).getAttribute('aria-pressed'), 'true');
      assert.equal(await page.getByTestId(`typeface-${face}`).getAttribute('aria-pressed'), 'true');
    } finally { await context.close(); }
  }
  process.stdout.write(`${engine}: saved appearance first-paint and selected-pill controls passed (7 values)\n`);
}

async function assertPrimary(button) {
  assert.deepEqual(await button.evaluate(el => {
    const s = getComputedStyle(el);
    const probe = document.createElement('span'); probe.style.fontFamily = 'var(--cf-font-sans)'; probe.style.color = 'var(--cf-on-accent)'; probe.style.backgroundColor = 'var(--cf-accent)'; document.body.append(probe);
    const p = getComputedStyle(probe);
    const result = [s.fontFamily === p.fontFamily, s.fontWeight, s.backgroundColor === p.backgroundColor, s.color === p.color]; probe.remove(); return result;
  }), [true, '600', true, true]);
}

/**
 * C071-1: a review is fixed from its first step. While its crops are fitted
 * (held open here), Submit, the notes, the instruction and the verdict take
 * no edit and a second press sends nothing; a note whose save was already
 * under way is not in the review, and it stays pending after the review is
 * received, with its draft, until the next submit sends it.
 */
async function checkSubmitRace(browser, origin, reviewPosts) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  try {
    const page = await context.newPage();
    // The page loads an image to make a drawing's crop (a blob URL) and to
    // re-encode a crop (decode); each waits while its gate is held.
    await page.addInitScript(() => {
      const gate = () => {
        const state = { held: false, waiting: 0 };
        state.open = new Promise((done) => { state.release = () => { state.held = false; done(); }; });
        return state;
      };
      const gates = { blob: gate(), decode: gate() };
      window.__gates = gates;
      const decode = HTMLImageElement.prototype.decode;
      HTMLImageElement.prototype.decode = async function held() {
        if (gates.decode.held) {
          gates.decode.waiting += 1;
          await gates.decode.open;
        }
        return decode.call(this);
      };
      const source = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, "src");
      Object.defineProperty(HTMLImageElement.prototype, "src", {
        configurable: true,
        get() { return source.get.call(this); },
        set(value) {
          if (gates.blob.held && String(value).startsWith("blob:")) {
            gates.blob.waiting += 1;
            void gates.blob.open.then(() => source.set.call(this, value));
            return;
          }
          source.set.call(this, value);
        },
      });
    });
    // This session stays open, so its draft must survive: end the poll
    // quietly instead of taking the fixture's session_closed reply.
    await page.route("**/app/api/events/poll", (route) =>
      route.fulfill({ status: 410, contentType: "application/json", body: "{}" }),
    );
    await page.goto(`${origin}/app?case=race`, { waitUntil: "networkidle" });
    await page.getByTestId("comment-btn").click();
    await page.locator("#cf-feedback-panel[data-open='true']").waitFor();
    // The first note: an element, whose crop is drawn at once.
    await page.evaluate(() => { document.querySelector("details.cf-tools").open = true; });
    await page.getByTestId("tool-pick-element").click();
    await page.locator("#cf-present-document[data-cf-capture-mode='element'] :focus").waitFor();
    await page.keyboard.press("Enter");
    await page.getByTestId("composer-text").fill("Sent note.");
    await page.getByTestId("composer-save").click();
    await page.getByTestId("composer").waitFor({ state: "detached" });
    await page.locator("#cf-feedback-panel textarea").fill("Sent instruction.");

    // The second note: an area on the stage drawing, whose crop loads an
    // image, so its save is still under way when Submit is pressed.
    await page.evaluate(() => { window.__gates.blob.held = true; window.__gates.decode.held = true; });
    const stage = page.locator("figure[aria-label='Stage fixture'] svg");
    await stage.scrollIntoViewIfNeeded();
    const box = await stage.boundingBox();
    await page.keyboard.down("Shift");
    await page.mouse.move(box.x + 10, box.y + 6);
    await page.mouse.down();
    await page.mouse.move(box.x + 200, box.y + 30, { steps: 10 });
    await page.mouse.up();
    await page.keyboard.up("Shift");
    await page.getByTestId("float-comment").click();
    await page.getByTestId("composer-text").fill("Held note.");
    await page.getByTestId("composer-save").click();
    await page.waitForFunction(() => window.__gates.blob.waiting === 1);
    assert.equal(await page.getByTestId("composer").count(), 1, "the held save finished early");

    reviewPosts.length = 0;
    await page.getByTestId("submit-all").evaluate((button) => button.click());
    await page.waitForFunction(() => window.__gates.decode.waiting === 1);
    // Fitting is under way: nothing that goes into the review takes an edit.
    for (const [name, locator] of [
      ["Submit", page.getByTestId("submit-all")],
      ["the instruction", page.locator("#cf-feedback-panel textarea")],
      ["the verdict", page.locator("#cf-review-verdict")],
      ["Remove", page.getByTestId("note-remove").first()],
      ["Save", page.getByTestId("composer-save")],
    ]) assert.equal(await locator.isDisabled(), true, `${name} took input while the review was prepared`);
    await page.getByTestId("submit-all").evaluate((button) => button.click());
    await page.getByTestId("note-remove").first().evaluate((button) => button.click());
    await page.getByTestId("note-row").first().evaluate((row) => row.click());
    assert.equal(await page.getByTestId("composer-text").inputValue(), "Held note.", "an edit opened while the review was prepared");
    // The save begun before Submit lands while the review is prepared.
    await page.evaluate(() => window.__gates.blob.release());
    await page.waitForFunction(() => document.querySelectorAll("[data-testid=note-row]").length === 2);
    await page.evaluate(() => window.__gates.decode.release());
    await page.getByTestId("toast").getByText(/Review received.* 1 note saved while it was sent is still pending\./u).waitFor({ timeout: 10_000 });
    assert.equal(reviewPosts.length, 1, "a second press sent the review again");
    assert.deepEqual(reviewPosts[0].notes.map((note) => note.body), ["Sent note."]);
    assert.equal(reviewPosts[0].instruction, "Sent instruction.");
    assert.deepEqual(await page.getByTestId("note-row").locator(".b").allInnerTexts(), ["Held note."], "the held note did not stay pending");
    assert.equal(await page.locator("#cf-feedback-panel textarea").inputValue(), "", "the sent instruction was kept");
    const draft = await page.evaluate(() => JSON.parse(sessionStorage.getItem("cf-present-draft:019f9b53-a341-7fa7-84c2-5f198ceea001") ?? "null"));
    assert.deepEqual(draft?.notes.map((note) => note.body), ["Held note."], "the held note left the draft");

    await page.getByTestId("submit-all").click();
    await page.waitForFunction(() => document.querySelectorAll("[data-testid=note-row]").length === 0);
    assert.equal(reviewPosts.length, 2);
    assert.deepEqual(reviewPosts[1].notes.map((note) => note.body), ["Held note."]);
    process.stdout.write("submit race passed: fitting holds the review fixed, a second press sends nothing, and a note saved meanwhile stays pending until it is sent\n");
  } finally {
    await context.close();
  }
}

async function checkStaticExportModes(browser, origin) {
  for (const { mode, preference, expected } of [
    // The retained export default resolves to Slate in either appearance.
    { mode: "system", preference: "dark", expected: "rgb(18, 23, 29)" },
    { mode: "system", preference: "light", expected: "rgb(234, 238, 243)" },
    { mode: "dark", preference: "light", expected: "rgb(18, 23, 29)" },
    { mode: "light", preference: "dark", expected: "rgb(234, 238, 243)" },
  ]) {
    const context = await browser.newContext({ colorScheme: preference, javaScriptEnabled: false });
    const page = await context.newPage();
    const network = await installNetworkAudit(context, page);
    await page.goto(`${origin}/export?mode=${mode}`, { waitUntil: "load" });
    const background = await page.locator("html").evaluate((element) => getComputedStyle(element).backgroundColor);
    if (background !== expected) {
      throw new Error(`${mode}/${preference} no-script export background was ${background}, expected ${expected}`);
    }
    await page.frameLocator("iframe[title='Static export sandbox']").getByText("Static export sandbox content").waitFor();
    const exportAttack = page.frameLocator("iframe[title='Static export attack']");
    await exportAttack.getByText("Static export attack stayed local").waitFor({ state: "attached" });
    const exportAttackHandle = await page.locator("iframe[title='Static export attack']").elementHandle();
    const exportAttackFrame = await exportAttackHandle?.contentFrame();
    const exportAttackUrl = exportAttackFrame?.url() ?? "missing";
    if (exportAttackUrl !== "" && !exportAttackUrl.startsWith("about:srcdoc")) {
      throw new Error(`Static export sandbox navigated to ${exportAttackUrl}`);
    }
    assertNetworkStayedLoopback(network);
    await context.close();
  }
}

async function checkProseLazyPath(browser, origin) {
  const context = await browser.newContext({ colorScheme: "dark" });
  const page = await context.newPage();
  await recordPolicyViolations(page);
  const requests = [];
  page.on("request", (request) => requests.push(new URL(request.url())));
  await page.goto(`${origin}/app?case=prose`, { waitUntil: "networkidle" });
  await page.locator(".cf-topbar").waitFor();
  const resolved = await page.locator("html").getAttribute("data-cf-mode-resolved");
  if (resolved !== "dark") throw new Error(`System dark mode resolved to ${resolved}`);
  const dynamicPaths = new Set(
    manifest.service.assets
      .find((asset) => asset.request_path === appPath)
      // The small offline font module is intentionally available on prose
      // pages; the syntax and figure renderers must remain lazy.
      .imports.filter((item) => item.kind === "dynamic-import" && !/\/chunk-fonts-[^/]+\.js$/u.test(item.request_path))
      .map((item) => item.request_path),
  );
  if (requests.some((request) => dynamicPaths.has(request.pathname))) {
    throw new Error("A prose-only page requested a syntax or figure entry path");
  }
  assertLoopbackOnly(requests);
  await assertNoPolicyViolations(page, "prose page");
  await context.close();
}

async function checkPhoneSurface(browser, origin) {
  const context = await browser.newContext({ viewport: { width: 375, height: 760 } });
  try {
    const page = await context.newPage();
    await page.goto(`${origin}/app`, { waitUntil: "networkidle" });
    await page.getByTestId("comment-btn").click();
    const dock = page.getByTestId("notes-dock");
    await dock.locator(".hd").waitFor();
    assert.equal(await dock.getAttribute("data-expanded"), "false", "375 px opens as a peek");
    assert.ok((await dock.boundingBox()).height <= 57, "the peek covers more than 56 px");
    const stage = await page.locator(".block--html .cf-stage-svg").evaluate((svg) => ({
      width: svg.getBoundingClientRect().width,
      floor: svg.viewBox.baseVal.width * 0.75,
      scroll: svg.closest(".cf-stage-host").scrollWidth,
      viewport: svg.closest(".cf-stage-host").clientWidth,
    }));
    assert.ok(stage.width >= stage.floor && stage.scroll > stage.viewport, `stage lost its scale floor or pan: ${JSON.stringify(stage)}`);
    assert.ok(!/block-(summary|flow|stage|code)/u.test(await page.locator(".cf-section-route").innerText()), "the rail exposes block ids");
    const label = page.locator("#gesture-target");
    await label.scrollIntoViewIfNeeded();
    await label.click();
    const chip = page.getByTestId("float-chip");
    await chip.waitFor();
    const overlap = await page.evaluate(() => {
      const a = document.querySelector("[data-testid=float-chip]").getBoundingClientRect();
      const b = document.querySelector("#gesture-target").getBoundingClientRect();
      return Math.max(0, Math.min(a.right, b.right) - Math.max(a.left, b.left)) * Math.max(0, Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top));
    });
    assert.equal(overlap, 0, "the selection chip covers its target");
    assert.equal(await dock.getAttribute("data-expanded"), "true", "a pin should expand notes");
    // On a fresh page, pulling the peek bar up expands the sheet, to no more
    // than the dock height, min(46vh, 440px).
    const fresh = await context.newPage();
    await fresh.goto(`${origin}/app`, { waitUntil: "networkidle" });
    await fresh.getByTestId("comment-btn").click();
    const peek = fresh.getByTestId("notes-dock");
    await peek.locator(".hd").waitFor();
    assert.equal(await peek.getAttribute("data-expanded"), "false", "375 px opens as a peek");
    await peek.locator(".hd").dispatchEvent("pointerdown", { clientY: 740 });
    await peek.locator(".hd").dispatchEvent("pointerup", { clientY: 700 });
    await fresh.locator("[data-testid=notes-dock][data-expanded='true']").waitFor();
    assert.ok((await peek.boundingBox()).height <= Math.min(760 * 0.46, 440) + 1, "the expanded sheet is taller than the dock");
    process.stdout.write("375 px surface passed: notes peek, pull to expand, html stage floor and pan, no block ids in the rail, chip clear of its target\n");
  } finally { await context.close(); }
}

async function checkInteractiveSurface(browser, origin, capturedReviews) {
  const context = await browser.newContext({ viewport: { width: 320, height: 760 }, colorScheme: "light" });
  const page = await context.newPage();
  const network = await installNetworkAudit(context, page);
  const consoleErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error" && !isExpectedAttackConsoleError(message, origin)) {
      consoleErrors.push(message.text());
    }
  });
  await recordPolicyViolations(page);
  await page.addInitScript({ content: axe.source });
  await page.goto(`${origin}/app`, { waitUntil: "networkidle" });
  await assertBundledFonts(page);
  // At phone width arming Comment opens the 56 px notes peek.
  await page.getByRole("button", { name: /Comment/ }).click();
  await page.locator(".cf-hint.on").waitFor();
  assert.equal(await page.locator("#cf-feedback-panel").getAttribute("data-open"), "true");
  assert.equal(await page.locator("#cf-feedback-panel").getAttribute("data-expanded"), "false");
  await page.getByRole("button", { name: /Comment/ }).click();
  await page.locator("#cf-feedback-panel[data-expanded='true']").waitFor();
  await page.getByTestId("feedback-history").waitFor();
  await page.getByTestId("feedback-history").locator("summary").click();
  await page.getByText("Matched uniquely in this revision.").waitFor();
  await page.getByText("Unpositioned: the referenced block is absent").waitFor();
  // Each earlier note names its block and its state in words, set apart (P2-1).
  const headings = await page.getByTestId("feedback-history").locator(".cf-note-heading").evaluateAll((nodes) =>
    nodes.map((node) => ({ parts: [...node.children].map((child) => child.textContent), gap: parseFloat(getComputedStyle(node).columnGap) || 0 })));
  // "moved" only when the anchor reports a change (round 3).
  assert.deepEqual(headings.map((heading) => heading.parts), [["Summary", "anchored"], ["Removed detail", "unpositioned"], ["Implementation", "moved"], ["Flow", "anchored"], ["Whole document", "anchored"]]);
  await page.getByText("The same element: its block is unchanged in this revision.").waitFor();
  await page.getByText("Still the whole document in this revision.").waitFor();
  assert.ok(headings.every((heading) => heading.gap > 0), `an earlier note heading runs its parts together: ${JSON.stringify(headings)}`);
  await page.getByRole("button", { name: "Close" }).click();
  const code = page.locator("code[data-cf-language='rust']");
  await code.scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("code[data-cf-language='rust']")?.getAttribute("data-cf-highlight") === "ready");
  const figure = page.locator("[data-cf-block-id='block-flow'] [data-cf-figure-block]");
  await figure.scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("[data-cf-figure-block]")?.getAttribute("data-cf-figure-block") !== "pending");
  if (await figure.getAttribute("data-cf-figure-block") !== "ready") {
    throw new Error(`Figure block did not draw: ${await figure.locator("[data-cf-figure-status]").textContent()}`);
  }
  const figureEvidence = await figure.evaluate((block) => ({
    // The grammar draws a wide and a narrow composition; one shows at a time.
    figures: [...block.querySelectorAll("figure.cf-fig svg.cf-fig-svg")].filter((svg) => svg.getBoundingClientRect().width > 0).length,
    states: [...new Set([...block.querySelectorAll("svg [data-state]")].map((node) => node.getAttribute("data-state")))].length,
    legend: block.querySelectorAll(".cf-legend li").length,
    styled: block.querySelectorAll("[style]").length,
    unsafe: block.querySelectorAll("script, foreignObject").length,
  }));
  if (figureEvidence.figures !== 1 || figureEvidence.states < 2 || figureEvidence.legend < 2 || figureEvidence.styled || figureEvidence.unsafe) {
    throw new Error(`Figure block drew an unexpected figure: ${JSON.stringify(figureEvidence)}`);
  }
  if (await page.locator("[data-cf-diagram], [data-cf-diagram-source]").count()) {
    throw new Error("The review surface still carries a diagram hook");
  }
  await page.frameLocator("iframe[title='Sandbox fixture']").getByText("Static sandbox content").waitFor();
  const attackFrame = page.frameLocator("iframe[title='Attack sandbox']");
  await attackFrame.getByText("Attack sandbox stayed local").waitFor({ state: "attached" });
  const attackHandle = await page.locator("iframe[title='Attack sandbox']").elementHandle();
  const attackContentFrame = await attackHandle?.contentFrame();
  const attackUrl = attackContentFrame?.url() ?? "missing";
  if (!attackUrl.endsWith("/sandbox/1/attack")) throw new Error(`Sandbox navigated to ${attackUrl}`);

  const overflow = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
    offenders: [...document.querySelectorAll("body *")]
      .map((element) => ({ tag: element.tagName, id: element.id, className: element.className, right: element.getBoundingClientRect().right, width: element.scrollWidth }))
      .filter((item) => item.right > document.documentElement.clientWidth + 1)
      .slice(0, 8),
    localScroll: (() => {
      const element = document.querySelector(".cf-local-scroll");
      if (!element) return null;
      const style = getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return { left: rect.left, right: rect.right, width: rect.width, scrollWidth: element.scrollWidth, overflow: style.overflow, marginInline: style.marginInline, inlineSize: style.inlineSize };
    })(),
    visibleOverflow: [...document.querySelectorAll("body *")]
      .map((element) => ({ tag: element.tagName, id: element.id, className: element.className, clientWidth: element.clientWidth, scrollWidth: element.scrollWidth, overflowX: getComputedStyle(element).overflowX }))
      .filter((item) => item.scrollWidth > item.clientWidth + 1 && item.overflowX === "visible")
      .slice(0, 8),
  }));
  if (overflow.scrollWidth > overflow.clientWidth) {
    throw new Error(`The review surface overflows at 320 CSS px: ${JSON.stringify(overflow)}`);
  }

  await page.evaluate(() => {
    const root = document.getElementById("cf-present-document");
    if (!root) throw new Error("Document root missing");
    globalThis.__cfDocumentRoot = root;
  });

  // Pass10 Comment SM: arm → pin → float → composer → rail → speech markers
  await page.getByRole("button", { name: /Comment/ }).click();
  await page.locator(".cf-hint.on").waitFor();
  // One instruction in the hint, the empty rail and the status line (P3-2).
  const instruction = "Select words, click any part, or drag a box; hold Shift to start a box on words.";
  assert.equal(await page.locator(".cf-hint.on [role=status]").innerText(), `${instruction} Esc leaves.`);
  await openSheet();
  await page.getByText("Nothing noted yet").waitFor();
  assert.equal(await page.getByTestId("notes-empty").locator(".h").innerText(), instruction);
  assert.equal(await page.locator(".cf-dock .cf-status").innerText(), instruction);

  // Drag starting on the prose wrapper (padding around the paragraph) must stay
  // Text. Missing that hit-test is how region marquees steal text selection.
  const glyphs = (from, to) => page.locator("#gesture-target").evaluate((el, [from, to]) => {
    const range = document.createRange();
    range.setStart(el.firstChild, from);
    range.setEnd(el.firstChild, to);
    const rect = range.getBoundingClientRect();
    return { left: rect.left, right: rect.right, y: rect.top + rect.height / 2 };
  }, [from, to]);
  // Each pin reopens the notes panel, which covers the prose at this width.
  async function proseDrag(label, { from, to, hold = 0, steps = 10, quote, pause }) {
    await revealDocumentForGestures();
    const end = await glyphs(0, to);
    const target = { x: end.right, y: end.y };
    const hits = await page.evaluate(({ from, target }) => ({
      start: document.elementFromPoint(from.x, from.y)?.id,
      end: document.elementFromPoint(target.x, target.y)?.id,
      startElement: document.elementFromPoint(from.x, from.y)?.outerHTML.slice(0, 200),
      endElement: document.elementFromPoint(target.x, target.y)?.outerHTML.slice(0, 200),
    }), { from, target });
    const expectedStart = from.onGlyph ? "gesture-target" : "gesture-root";
    if (hits.start !== expectedStart || hits.end !== "gesture-target") {
      throw new Error(`${label} is obscured or off-screen: ${JSON.stringify({ from, target, hits })}`);
    }
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    if (hold) await page.waitForTimeout(hold);
    if (pause) {
      // Hold the drag still partway, past the selection pin's settle delay:
      // a drag pins on release only, so no chip may open under it (TSK-160).
      const mid = await glyphs(0, pause.at);
      await page.mouse.move(mid.right, mid.y, { steps: 4 });
      const partial = await page.evaluate(() => String(getSelection()));
      if (partial !== pause.quote) throw new Error(`${label} held ${JSON.stringify(partial)}, expected the partial ${pause.quote}`);
      await page.waitForTimeout(pause.ms);
      const early = await page.getByTestId("float-chip").count();
      if (early !== 0) throw new Error(`${label} opened a chip while the drag was held`);
    }
    await page.mouse.move(target.x, target.y, { steps });
    const marquee = await page.locator(".cf-region-draft").count();
    await page.mouse.up();
    const selected = await page.evaluate(() => String(getSelection()));
    if (selected !== quote) throw new Error(`${label} selected ${JSON.stringify(selected)}, expected ${quote}`);
    await waitForTextChip(page, label);
    await page.waitForFunction(
      (quote) => document.querySelector("[data-testid=float-chip] .q")?.textContent === quote,
      quote,
      { timeout: 5000 },
    ).catch(() => {});
    const kind = (await page.getByTestId("float-chip").locator(".lab").innerText()).trim();
    if (marquee !== 0) throw new Error(`${label} drew a region marquee`);
    if (kind !== "Text") throw new Error(`${label} opened ${kind}, expected Text`);
    const pinned = await page.getByTestId("float-chip").locator(".q").innerText();
    if (pinned !== quote) throw new Error(`${label} pinned ${JSON.stringify(pinned)}, expected ${quote}`);
  }
  async function prepareProse() {
    await revealDocumentForGestures();
    const prose = page.locator("#gesture-root");
    await prose.evaluate((el) => el.scrollIntoView({ block: "center", behavior: "instant" }));
    const box = await prose.boundingBox();
    if (!box) throw new Error("Prose review-text-root has no box");
    const review = await glyphs(0, 6);
    // Inside the "e" of "Review": a press there lands on the captured highlight
    // and a drag from it starts a new selection at offset 1.
    const e = await glyphs(1, 2);
    return {
      padding: { x: box.x + 4, y: review.y },
      glyph: { x: e.left + 1, y: e.y, onGlyph: true },
    };
  }
  // The press under test must meet a live highlight with its chip showing.
  async function expectHighlighted(label) {
    await revealDocumentForGestures();
    const live = await page.evaluate(() => String(getSelection()));
    const chips = await page.getByTestId("float-chip").count();
    if (live !== "Review" || chips !== 1) {
      throw new Error(`${label} has no live capture: ${JSON.stringify({ live, chips })}`);
    }
  }
  // The rail names no pin once the pin is gone.
  async function expectNoPinnedStatus(label) {
    const status = await page.waitForFunction(
      () => {
        const text = document.querySelector(".cf-status")?.textContent?.trim() ?? "";
        return text.startsWith("Pinned:") ? false : text || "(empty)";
      },
      undefined,
      { timeout: 2000 },
    ).catch(async () => (await page.locator(".cf-status").first().textContent())?.trim());
    if (typeof status === "string") throw new Error(`${label} left the status naming a dropped pin: ${JSON.stringify(status)}`);
  }
  async function expectReleased(label) {
    const kept = await page.evaluate(() => String(getSelection()));
    if (kept) throw new Error(`${label} kept the discarded capture highlighted: ${JSON.stringify(kept)}`);
  }
  async function dismissChip() {
    await page.keyboard.press("Escape");
    await page.getByTestId("float-chip").waitFor({ state: "detached", timeout: 5000 });
  }
  for (const steps of [1, 10]) {
    const { padding } = await prepareProse();
    await proseDrag(`Prose drag (${steps} steps)`, { from: padding, steps, to: 6, quote: "Review" });
    await dismissChip();
  }
  // A reader who pauses mid-drag gets one Text pin of the final selection,
  // made on release (TSK-160).
  {
    const { padding } = await prepareProse();
    await proseDrag("Prose drag held for 400 ms partway", { from: padding, to: 6, quote: "Review", pause: { at: 3, ms: 400, quote: "Rev" } });
    await dismissChip();
  }
  // A one-character drag is a Text pin, made on release like any other.
  {
    const { padding } = await prepareProse();
    await proseDrag("One-character prose drag", { from: padding, to: 1, quote: "R" });
    await dismissChip();
  }

  // Dropping a text capture must release its highlight. Chromium turns a press
  // on a live highlight into a native text drag, not a new selection: at once
  // on Linux and Windows, after 150 ms on macOS. The 200 ms holds below take
  // that path on every platform. Presses start inside the "e" of the captured
  // "Review", and the new selection must read "eview me", which a leftover
  // "Review" cannot satisfy.
  {
    const { padding, glyph } = await prepareProse();
    const reselect = (label) => proseDrag(label, { from: padding, to: 6, quote: "Review" });

    // A held press on the highlight while its chip shows selects anew.
    await reselect("Prose drag before a held press on the chip's highlight");
    await expectHighlighted("Held press on the chip's highlight");
    await proseDrag("Held press on the chip's highlight", { from: glyph, to: 9, hold: 200, quote: "eview me" });
    await dismissChip();
    await expectReleased("Escape");

    // Escape releases the highlight, so a held press on those glyphs selects.
    await reselect("Prose drag before Escape");
    await dismissChip();
    await expectReleased("Escape");
    await expectNoPinnedStatus("Escape");
    await proseDrag("Held press after Escape", { from: glyph, to: 9, hold: 200, quote: "eview me" });
    await dismissChip();

    // A click on the highlight only dismisses its Text chip. It must not fall
    // through to an Element pin once the press has cleared the highlight.
    await reselect("Prose drag before a click on its highlight");
    await expectHighlighted("Click on the captured highlight");
    const clicked = await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.id, glyph);
    if (clicked !== "gesture-target") throw new Error(`The highlight click would land on ${clicked}`);
    await page.mouse.click(glyph.x, glyph.y);
    await page.waitForTimeout(400);
    const chip = page.getByTestId("float-chip");
    if (await chip.count()) {
      const kind = (await chip.locator(".lab").innerText()).trim();
      throw new Error(`A click on the captured highlight opened ${kind}, expected no chip`);
    }
    await expectNoPinnedStatus("A click on the captured highlight");

    // TSK-096 edge rule: the painted highlight decides. A click just past
    // its last glyph, in the same line box, is a click on the paragraph:
    // it drops the text pin and pins the element, as any other click does.
    await reselect("Prose drag before a click past its last glyph");
    await expectHighlighted("Click past the captured highlight");
    const edge = await glyphs(0, 6);
    const past = { x: edge.right + 2, y: edge.y };
    const pastTarget = await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.id, past);
    if (pastTarget !== "gesture-target") throw new Error(`The edge click would land on ${pastTarget}`);
    await page.mouse.click(past.x, past.y);
    await page.getByTestId("float-chip").waitFor({ timeout: 5000 });
    const edgeKind = (await page.getByTestId("float-chip").locator(".lab").innerText()).trim();
    if (edgeKind !== "Element") throw new Error(`A click past the highlight opened ${edgeKind}, expected Element`);
    await expectReleased("A click past the highlight");
    await dismissChip();

    // Composer Cancel leaves no highlight. Chromium already moves the
    // selection into the focused composer, so this locks the outcome only.
    await reselect("Prose drag before composer Cancel");
    await page.getByTestId("float-comment").click();
    await page.getByTestId("composer").waitFor();
    await page.getByTestId("composer-cancel").click();
    await page.getByTestId("composer").waitFor({ state: "detached", timeout: 5000 });
    await expectReleased("Composer Cancel");
    // Gesture listeners remount in an effect after the composer closes.
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => setTimeout(done, 50))));
    // The chip's esc button must release the highlight itself.
    await reselect("Prose drag before the chip esc button");
    await page.getByTestId("float-esc").click();
    await page.getByTestId("float-chip").waitFor({ state: "detached", timeout: 5000 });
    await expectReleased("The chip esc button");
    await expectNoPinnedStatus("The chip esc button");
  }

  // Words on an authored SVG stage must pin as Text (same as HTML prose).
  {
    await page.evaluate(() => {
      const node = document.getElementById("stage-label")?.firstChild;
      if (!node) throw new Error("Stage label text node missing");
      const range = document.createRange();
      range.selectNodeContents(node);
      const selection = getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
    });
    await waitForTextChip(page, "Stage label selection");
    const kind = (await page.getByTestId("float-chip").locator(".lab").innerText()).trim();
    if (kind !== "Text") throw new Error(`Stage label selection opened ${kind}, expected Text`);
    await page.keyboard.press("Escape");
    await page.getByTestId("float-chip").waitFor({ state: "detached", timeout: 5000 }).catch(() => {});
  }

  async function clickTool(testId) {
    await openSheet();
    await page.evaluate(() => {
      const tools = document.querySelector("details.cf-tools");
      if (tools) {
        tools.open = true;
        tools.scrollIntoView({ block: "end" });
      }
    });
    await page.getByTestId(testId).click();
  }
  async function saveComposerNote(body, { viaFloat = false } = {}) {
    if (viaFloat) {
      await page.getByTestId("float-chip").waitFor({ state: "attached", timeout: 10000 });
      await page.getByTestId("float-comment").click({ force: true });
    }
    const box = page.getByTestId("composer-text");
    await box.waitFor({ state: "attached", timeout: 10000 });
    if ((await box.getAttribute("maxlength")) !== "32") {
      throw new Error("Composer did not expose the server-provided length limit");
    }
    await box.fill(body, { force: true });
    await assertPrimary(page.getByTestId("composer-save"));
    // An unforced click waits until the button can take it: a forced one
    // lands wherever the button was measured, and a miss leaves the note unsaved.
    await page.getByTestId("composer-save").click();
    await page.getByTestId("composer").waitFor({ state: "detached", timeout: 10000 }).catch(async (error) => {
      // Name what the composer held when a save did not close it.
      const state = await page.evaluate(() => ({
        text: document.querySelector("[data-testid=composer-text]")?.value,
        saveDisabled: document.querySelector("[data-testid=composer-save]")?.disabled,
        status: [...document.querySelectorAll("[role=status], [aria-live]")].map((node) => node.textContent?.trim()).filter(Boolean),
        active: document.activeElement?.getAttribute("data-testid") ?? document.activeElement?.tagName,
      })).catch(() => "unreadable");
      throw new Error(`Saving "${body}" left the composer open: ${JSON.stringify(state)}\n${error.message}`);
    });
  }

  // Text note (tools open composer directly)
  await selectFixtureText(page);
  await clickTool("tool-add-text");
  await saveComposerNote("Keep this exact wording.");
  if ((await page.locator(".cf-note-row").count()) !== 1) throw new Error("Text note did not land in the rail");
  if ((await page.locator(".cf-marker").count()) !== 1) throw new Error("Speech marker missing for text note");

  const desktop = page.viewportSize();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.mouse.move(0, 0);
  await page.waitForFunction(() => [...document.querySelectorAll('.cf-marker')].every(el => el.getBoundingClientRect().right <= innerWidth - 8));
  assert.equal(await page.locator('.cf-marker').evaluateAll(nodes => nodes.every(el => el.getBoundingClientRect().left >= 0 && el.getBoundingClientRect().right <= innerWidth - 8)), true);
  await page.setViewportSize(desktop);

  // Limit
  await selectFixtureText(page);
  await clickTool("tool-add-text");
  await saveComposerNote("Second note body.");
  await selectFixtureText(page);
  await clickTool("tool-add-text");
  await page.getByText("A review can contain at most 2 notes.").waitFor({ timeout: 10000 });
  if ((await page.locator(".cf-note-row").count()) !== 2) {
    throw new Error("Review note limit was not enforced before creating an unsendable review");
  }
  async function removeAllNotes() {
    await page.evaluate(() => {
      document.querySelectorAll('[data-testid="note-remove"]').forEach((btn) => {
        if (btn instanceof HTMLElement) btn.click();
      });
    });
    // Second pass in case React re-render left one
    await page.waitForTimeout(50);
    await page.evaluate(() => {
      document.querySelectorAll('[data-testid="note-remove"]').forEach((btn) => {
        if (btn instanceof HTMLElement) btn.click();
      });
    });
    await page.waitForFunction(() => document.querySelectorAll(".cf-note-row").length === 0, null, { timeout: 10000 });
  }
  await removeAllNotes();

  // Element keyboard → composer
  await clickTool("tool-pick-element");
  await page.locator("#cf-present-document[data-cf-capture-mode='element']").waitFor();
  await page.locator("#cf-present-document[data-cf-capture-mode='element'] :focus").waitFor();
  await page.keyboard.press("Enter");
  await saveComposerNote("Element note body.");
  if ((await page.locator(".cf-marker").count()) !== 1) {
    throw new Error("Keyboard element feedback did not retain one visible speech marker");
  }
  await clickTool("tool-pick-element");
  await page.locator("#cf-present-document[data-cf-capture-mode='element']").waitFor();
  await page.keyboard.press("Escape");
  await page.locator("#cf-present-document:not([data-cf-capture-mode])").waitFor();
  if ((await page.locator(".cf-hint.on").count()) !== 1) {
    throw new Error("Esc should exit capture without leaving Comment mode");
  }
  await removeAllNotes();

  // Whole document → composer
  await clickTool("tool-whole-doc");
  await saveComposerNote("Whole document note.");
  await page.locator(".cf-note-row .a").filter({ hasText: "Whole document" }).waitFor();
  if ((await page.locator(".cf-marker").count()) !== 1) {
    throw new Error("Whole-document feedback did not retain one visible speech marker");
  }
  await removeAllNotes();

  // Harness excerpts: intercept the actual review POST, not the rail labels.
  async function submitCapturedReview() {
    capturedReviews.length = 0;
    await openSheet();
    await page.mouse.move(0, 0);
    await assertPrimary(page.getByTestId("submit-all"));
    await page.getByTestId("submit-all").click();
    await page.locator('.cf-chrome-frame[data-commenting="false"]').waitFor({ state: "attached" });
    await page.getByTestId("toast").getByText(/Review received/).waitFor({ timeout: 10000 });
    // The reviewer reads a plain receipt; the event id is the agent's (P3-4).
    assert.equal(await page.getByTestId("toast").innerText(), "Review received.");
    if (capturedReviews.length !== 1) {
      throw new Error(`Expected one review POST, got ${capturedReviews.length}`);
    }
    return capturedReviews[0];
  }
  async function armComment() {
    if ((await page.locator(".cf-chrome-frame").getAttribute("data-commenting")) === "true") return;
    await page.getByTestId("comment-btn").click();
    await page.waitForFunction(() =>
      document.querySelector(".cf-chrome-frame")?.getAttribute("data-commenting") === "true"
    );
  }
  async function assertPaintedJpeg(dataBase64, label) {
    if (typeof dataBase64 !== "string" || dataBase64.length < 80) {
      throw new Error(`${label} excerpt image missing or tiny (${dataBase64?.length ?? 0} b64 chars)`);
    }
    const result = await page.evaluate(async (data) => {
      const binary = atob(data);
      const bytes = Uint8Array.from(binary, (ch) => ch.charCodeAt(0));
      if (bytes[0] !== 0xff || bytes[1] !== 0xd8) return { ok: false, reason: "not jpeg" };
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/jpeg" }));
      const canvas = document.createElement("canvas");
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const ctx = canvas.getContext("2d");
      if (!ctx) return { ok: false, reason: "no canvas" };
      ctx.drawImage(bitmap, 0, 0);
      const sample = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      let painted = 0;
      let seen = 0;
      for (let i = 0; i < sample.length; i += 64) {
        seen += 1;
        if (sample[i] < 248 || sample[i + 1] < 248 || sample[i + 2] < 248) painted += 1;
      }
      return { ok: painted >= Math.max(4, seen * 0.01), painted, seen, width: bitmap.width, height: bitmap.height, bytes: bytes.length };
    }, dataBase64);
    if (!result.ok) {
      throw new Error(`${label} JPEG was blank or invalid: ${JSON.stringify(result)}`);
    }
  }

  await selectFixtureText(page);
  await clickTool("tool-add-text");
  await saveComposerNote("Text excerpt check.");
  const textReview = await submitCapturedReview();
  const textNote = textReview.notes?.[0];
  if (!textNote?.selector?.exact?.startsWith("Review this")) {
    throw new Error(`Text excerpt quote was ${JSON.stringify(textNote?.selector?.exact)}`);
  }
  if (textNote.excerpt?.text !== textNote.selector.exact) {
    throw new Error(`Text excerpt.text ${JSON.stringify(textNote.excerpt)} did not match the quote`);
  }
  if (textNote.excerpt?.image) {
    throw new Error("Text notes must not attach a crop image");
  }

  await armComment();
  await page.waitForFunction(() =>
    document.querySelector(".cf-chrome-frame")?.getAttribute("data-commenting") === "true"
    && !document.querySelector('[data-testid="composer"]')
    && !document.getElementById("cf-present-document")?.hasAttribute("data-cf-capture-mode")
  );
  // At phone width Comment, when armed, opens a closed sheet (as a peek while
  // it holds no note) and expands a peek.
  async function openSheet() {
    const panel = page.locator("#cf-feedback-panel");
    if ((await panel.getAttribute("data-open")) !== "true") {
      await page.locator("#cf-comment-toggle").click();
      await page.locator("#cf-feedback-panel[data-open='true']").waitFor();
    }
    if ((await panel.getAttribute("data-expanded")) !== "true") {
      await page.locator("#cf-comment-toggle").click();
      await page.locator("#cf-feedback-panel[data-expanded='true']").waitFor();
    }
  }
  async function revealDocumentForGestures() {
    if ((await page.locator("#cf-feedback-panel").getAttribute("data-open")) === "true") {
      await page.locator(".cf-feedback-close").click({ force: true });
      await page.waitForFunction(() =>
        document.getElementById("cf-feedback-panel")?.getAttribute("data-open") === "false"
      );
    }
  }
  async function shiftDragRegion(locator) {
    await revealDocumentForGestures();
    await locator.evaluate((el) => el.scrollIntoView({ block: "center", inline: "nearest" }));
    const box = await locator.boundingBox();
    if (!box) throw new Error("Shift+drag target has no box");
    await page.keyboard.down("Shift");
    await page.mouse.move(box.x + 10, box.y + Math.min(12, box.height / 2));
    await page.mouse.down();
    await page.mouse.move(box.x + Math.min(210, Math.max(80, box.width - 6)), box.y + Math.max(36, box.height - 4), { steps: 12 });
    const marquee = await page.locator(".cf-region-draft").count();
    await page.mouse.up();
    await page.keyboard.up("Shift");
    if (marquee === 0) {
      const hit = await page.evaluate(({ x, y }) => {
        const node = document.elementFromPoint(x, y);
        return node && { tag: node.tagName, id: node.id, className: String(node.className).slice(0, 80) };
      }, { x: box.x + 10, y: box.y + Math.min(12, box.height / 2) });
      throw new Error(`Shift+drag on prose drew no marquee; box=${JSON.stringify(box)} hit=${JSON.stringify(hit)}`);
    }
    await page.getByTestId("float-chip").waitFor({ state: "attached", timeout: 5000 });
    const kind = (await page.getByTestId("float-chip").locator(".lab").innerText()).trim();
    if (kind !== "Area") throw new Error(`Shift+drag on prose opened ${kind}, expected Area`);
    return kind;
  }

  // Shift+drag on prose is Region (plain drag on the same node is Text, tested above).
  await shiftDragRegion(page.locator("#selection-target"));
  await page.keyboard.press("Escape");
  await page.getByTestId("float-chip").waitFor({ state: "detached", timeout: 5000 }).catch(() => {});

  await clickTool("tool-pick-element");
  await page.locator("#cf-present-document[data-cf-capture-mode='element']").waitFor();
  await page.locator("#cf-present-document[data-cf-capture-mode='element'] :focus").waitFor();
  await page.evaluate(() => {
    window.__pickFocus = [];
    window.__pickFocusLog = (event) => window.__pickFocus.push(event.target?.getAttribute?.("data-testid") ?? event.target?.tagName);
    document.addEventListener("focusin", window.__pickFocusLog, true);
  });
  await page.keyboard.press("Enter");
  // The composer takes focus from the pick, and leaving the mode never hands
  // it back to the tool: text typed at once would otherwise be lost (TSK-096).
  await page.waitForFunction(() => document.activeElement?.getAttribute("data-testid") === "composer-text");
  const pickFocus = await page.evaluate(() => {
    document.removeEventListener("focusin", window.__pickFocusLog, true);
    return window.__pickFocus;
  });
  if (pickFocus.includes("tool-pick-element")) throw new Error(`An element pick handed focus back to its tool: ${pickFocus.join(" > ")}`);
  await saveComposerNote("Element excerpt body.");
  await page.waitForFunction(() =>
    !document.querySelector('[data-testid="composer"]')
    && !document.getElementById("cf-present-document")?.hasAttribute("data-cf-capture-mode")
  );

  await shiftDragRegion(page.locator("#selection-target"));
  await saveComposerNote("Region excerpt body.", { viaFloat: true });
  const mixedReview = await submitCapturedReview();
  const elementNote = mixedReview.notes.find((note) => note.element_selector);
  const regionNote = mixedReview.notes.find((note) => note.region_selector);
  if (!elementNote?.excerpt?.text) {
    throw new Error(`Element excerpt missing inner text: ${JSON.stringify(elementNote)}`);
  }
  if (!elementNote.excerpt.image || elementNote.excerpt.image.media_type !== "image/jpeg") {
    throw new Error("Element excerpt did not include a JPEG of the element");
  }
  await assertPaintedJpeg(elementNote.excerpt.image.data_base64, "element");
  if (!regionNote?.excerpt?.image || regionNote.excerpt.image.media_type !== "image/jpeg") {
    throw new Error(`Region excerpt missing JPEG: ${JSON.stringify(regionNote)}`);
  }
  await assertPaintedJpeg(regionNote.excerpt.image.data_base64, "region");
  if (!regionNote.excerpt.text) {
    throw new Error("Region excerpt missing intersecting visible text");
  }

  await armComment();

  // Leaving Comment mode hides the rail
  await page.keyboard.press("c");
  await page.waitForFunction(() =>
    document.querySelector(".cf-chrome-frame")?.getAttribute("data-commenting") === "false"
    && document.getElementById("cf-feedback-panel")?.getAttribute("data-open") === "false"
  );

  await page.getByTestId("settings-btn").click();
  await page.locator("[data-testid=skin-pills] [data-skin=graphite]").click();
  const identityPreserved = await page.evaluate(() => globalThis.__cfDocumentRoot === document.getElementById("cf-present-document"));
  if (!identityPreserved) throw new Error("Review chrome replaced the Rust-owned document root");

  await page.locator("[data-testid=scale-pills] [data-scale=large]").click();
  await page.getByTestId("typeface-plex").click();
  await page.waitForFunction(() =>
    document.documentElement.dataset.cfScale === "large"
    && document.documentElement.dataset.cfTypeface === "plex"
  );
  await page.locator("[data-testid=scale-pills] [data-scale=default]").click();
  await page.locator("[data-testid=typeface-pills] [data-typeface=archivo]").click();

  for (const theme of ["slate", "graphite", "sage"]) {
    await page.locator(`[data-testid=skin-pills] [data-skin=${theme}]`).click();
    for (const mode of ["Light", "Dark"]) {
      await page.getByRole("button", { name: mode, exact: true }).click();
      const axeResult = await page.evaluate(async () => globalThis.axe.run(document, {
        runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"] },
      }));
      if (axeResult.violations.length) {
        throw new Error(`${theme}/${mode} axe violations: ${axeResult.violations.map((item) => item.id).join(", ")}`);
      }
    }
  }
  if (consoleErrors.length) {
    const violations = await page.evaluate(() => globalThis.__cfPolicyViolations);
    throw new Error(`Browser console errors: ${consoleErrors.join(" | ")}; CSP: ${JSON.stringify(violations)}`);
  }
  await assertNoPolicyViolations(page, "interactive surface");
  assertNetworkStayedLoopback(network);
  await context.close();
}


async function assertBundledFonts(page) {
  const notices = page.locator("body > footer[data-cf-font-licenses]");
  await notices.waitFor({ state: "visible" });
  if (await page.locator("#cf-present-document footer[data-cf-font-licenses], #cf-present-chrome footer[data-cf-font-licenses]").count()) {
    throw new Error("Font notices entered an annotation or chrome root");
  }
  const text = await notices.textContent();
  for (const expected of ["Archivo", "Inter", "IBM Plex Sans", "SIL OPEN FONT LICENSE"]) {
    if (!text.includes(expected)) throw new Error(`Missing bundled font notice: ${expected}`);
  }
  for (const family of ["Archivo", "Inter", "IBM Plex Sans"]) {
    const loaded = await page.evaluate(async (name) => {
      const faces = await document.fonts.load(`400 16px "${name}"`);
      return faces.length > 0 && faces.every((face) => face.status === "loaded");
    }, family);
    if (!loaded) throw new Error(`Bundled font did not load: ${family}`);
  }
}

async function selectFixtureText(page) {
  await page.evaluate(() => {
    const text = document.getElementById("selection-target")?.firstChild;
    if (!text) throw new Error("Selection fixture missing");
    const range = document.createRange();
    range.setStart(text, 0);
    range.setEnd(text, 11);
    const selection = getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
  });
}

function isExpectedAttackConsoleError(message, origin) {
  const location = message.location().url;
  const text = message.text();
  // Firefox reports its own favicon request against the page CSP, and the
  // automation layer's messages to the sandboxed (null-origin) frames; the
  // page itself posts no messages and requests no favicon.
  if (text.includes("FaviconLoader.sys.mjs") || (text.includes("postMessage") && text.includes("recipient window’s origin (‘null’)"))) return true;
  // WebKit words the attack frame's refusals differently and reports them
  // without the frame's location.
  if (text.includes("https://example.invalid/") || text.includes("/sandbox/1/attack' because the document's frame is sandboxed")
    || text === "Unable to do meta refresh due to sandboxing" || text.includes("Recipient has origin null")) return true;
  return text.startsWith("Refused to execute the redirect specified via '<meta http-equiv='refresh'")
    || location.startsWith(`${origin}/sandbox/1/attack`)
    && (text.includes("example.invalid")
      || text.includes("frame is sandboxed")
      || text.includes("sandboxed and the 'allow-scripts' permission is not set"));
}

async function installNetworkAudit(context, page) {
  const responses = [];
  const externalRoutes = [];
  await context.route("https://example.invalid/**", async (route) => {
    externalRoutes.push(route.request().url());
    await route.abort("blockedbyclient");
  });
  page.on("response", (response) => responses.push(new URL(response.url())));
  return { responses, externalRoutes };
}

function assertNetworkStayedLoopback({ responses, externalRoutes }) {
  if (externalRoutes.length) {
    throw new Error(`Sandbox attempted external network routes: ${externalRoutes.join(", ")}`);
  }
  assertLoopbackOnly(responses);
}

function fixtureHtml(proseOnly, selectionOnly = false, iframeOnly = false, limits = {}) {
  const enhancements = proseOnly
    ? ""
    : `<section data-cf-block-id="block-code" data-cf-block-label="Implementation" data-cf-block-digest="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa">
        <h2 id="implementation">Implementation</h2>
        <div data-cf-review-text-root><p id="selection-target">Review this exact sentence before approval.</p>
        <figure role="img" aria-label="Stage fixture"><svg viewBox="0 0 280 36" width="280" height="36"><text id="stage-label" x="8" y="24">Stage words</text></svg></figure></div>
        <pre tabindex="0" role="region" aria-label="Rust example"><code data-cf-language="rust">fn main() { println!("safe"); }</code></pre>
      </section>
      <section class="block block--figure" data-cf-block-id="block-flow" data-cf-block-label="Flow" data-cf-block-digest="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb">
        <h2 id="flow">Flow</h2>
        <div data-cf-review-text-root data-cf-canonical-text="Figure">
          <div class="figure-block" data-cf-figure-block="pending" data-cf-figure-declaration="${escapeAttribute(figureDeclaration)}">
            <div data-cf-figure-output><p class="figure-block__title">Figure</p></div>
            <p data-cf-figure-status class="sr-only" role="status"></p>
          </div>
        </div>
      </section>
      <section class="block block--html" data-cf-block-id="block-stage" data-cf-block-label="Stage" data-cf-block-digest="${"f".repeat(64)}">
        <h2 id="stage">Stage</h2>
        <div class="cf-stage-host"><svg class="cf-stage-svg" viewBox="0 0 640 180" role="img" aria-label="Stage scale fixture"><rect x="0" y="0" width="640" height="180"/><text x="20" y="40">Stage scale fixture</text></svg></div>
      </section>`;
  const sandbox = iframeOnly
    ? `<section data-cf-block-id="frame-block" data-cf-block-label="Embedded view" data-cf-block-digest="${"e".repeat(64)}">
        <h2>Embedded view</h2>
        <div id="frame-scroll" style="height:240px;overflow:auto;transform:translateX(0)">
          <figure id="frame-figure" style="margin:0;height:420px">
            <iframe sandbox title="Sandbox fixture" src="/sandbox/1/fixture" style="height:400px;pointer-events:auto !important"></iframe>
          </figure>
        </div>
      </section>`
    : proseOnly ? "" : '<figure><iframe sandbox title="Sandbox fixture" src="/sandbox/1/fixture"></iframe></figure>';
  const attackSandbox = proseOnly ? "" : '<iframe sandbox title="Attack sandbox" src="/sandbox/1/attack" style="width:1px;height:1px;position:fixed;left:0;top:0;opacity:0"></iframe>';
  const config = JSON.stringify({
    schema_version: 1,
    session_id: "019f9b53-a341-7fa7-84c2-5f198ceea001",
    revision: 1,
    event_sequence: 0,
    response_sequence: 0,
    title: proseOnly ? "Plain-language review" : "Runtime review",
    shortcuts_enabled: true,
    review_limits: {
      max_notes: 2,
      max_visible_feedback: 256,
      max_text_utf16: 32,
      max_selector_utf16: 16,
      max_payload_bytes: 262144,
      ...limits,
    },
    identity: {
      src: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
      alt: "Fixture project",
    },
    feedback: proseOnly ? { items: [], omitted_older: 0 } : {
      omitted_older: 2,
      items: [
        {
          event_id: "019f9b53-a341-7fa7-84c2-5f198ceea010",
          source_revision: 1,
          event_version: 3,
          lifecycle: "addressed",
          verdict: "request_changes",
          instruction: "Keep the boundary explicit.",
          notes: [
            {
              id: "019f9b53-a341-7fa7-84c2-5f198ceea011",
              block_label: "Summary",
              kind: "comment",
              body: "This exact quote moved once.",
              quote: "bounded review runtime",
              anchor: { state: "reanchored", start_utf16: 2, end_utf16: 24 },
            },
            {
              id: "019f9b53-a341-7fa7-84c2-5f198ceea012",
              block_label: "Removed detail",
              kind: "question",
              body: "This remains visible without a fabricated location.",
              quote: "removed text",
              anchor: { state: "orphaned", reason: "the referenced block is absent" },
            },
          ],
        },
        {
          event_id: "019f9b53-a341-7fa7-84c2-5f198ceea015",
          source_revision: 1,
          event_version: 1,
          lifecycle: "delivered",
          verdict: "approve_with_notes",
          notes: [
            {
              id: "019f9b53-a341-7fa7-84c2-5f198ceea013",
              block_label: "Implementation",
              kind: "comment",
              body: "This quote changed in the new revision.",
              quote: "bounded runtime",
              anchor: { state: "reanchored", start_utf16: 0, end_utf16: 7, changed: true },
            },
            {
              id: "019f9b53-a341-7fa7-84c2-5f198ceea014",
              block_label: "Flow",
              kind: "comment",
              body: "This element's block did not change.",
              anchor: { state: "element_reanchored", element_path: "h2:nth-of-type(1)" },
            },
          ],
        },
        {
          event_id: "019f9b53-a341-7fa7-84c2-5f198ceea016",
          source_revision: 1,
          event_version: 1,
          lifecycle: "delivered",
          verdict: "approve_with_notes",
          notes: [
            {
              id: "019f9b53-a341-7fa7-84c2-5f198ceea017",
              block_label: "Whole document",
              kind: "comment",
              body: "This area covers the whole document.",
              anchor: { state: "region_reanchored", scope: "document", anchor_id: "document", x_ppm: 0, y_ppm: 0, width_ppm: 1000000, height_ppm: 1000000 },
            },
          ],
        },
      ],
    },
  }).replaceAll("<", "\\u003c");
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <meta name="color-scheme" content="light dark">
  <title>cf-present browser check</title>
  <script>${prepaint}</script>
  <link rel="stylesheet" href="${stylePath}">
  <style data-cf-project-utility-tokens>${projectUtilityCss}</style>
</head>
<body>
  <div id="cf-present-chrome" data-session-id="019f9b53-a341-7fa7-84c2-5f198ceea001"></div>
  <main id="cf-present-document">
    <header><p>Outcome</p><h1>${proseOnly ? "A focused explanation" : "A bounded review runtime"}</h1></header>
    <section data-cf-block-id="block-summary" data-cf-block-label="Summary" data-cf-block-digest="cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc">
      <h2 id="summary">Summary</h2>
      <div data-cf-review-text-root id="gesture-root" style="padding:12px"><p id="gesture-target" style="margin:0">Review me.</p></div>
      <div data-cf-review-text-root><p>The document remains readable without its review controls.</p></div>
    </section>
    ${selectionOnly ? `<section data-cf-block-id="selection-cases" data-cf-block-label="Selection cases" data-cf-block-digest="dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd">
      <h2>Selection cases</h2>
      <div data-cf-review-text-root><p id="selection-limit">0123456789abcdefZ</p></div>
      <div data-cf-review-text-root><p id="selection-repeat">First passed. Next passed. End.</p></div>
    </section>` : ""}
    ${enhancements}
    ${sandbox}
    ${attackSandbox}
  </main>
  <template id="cf-present-config">${config}</template>
  <script type="module" src="${appPath}"></script>
</body>
</html>`;
}

function escapeAttribute(value) {
  return value.replaceAll("&", "&amp;").replaceAll("\"", "&quot;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

function exportFixture(mode) {
  const resolved = mode === "dark" ? "dark" : "light";
  return `<!doctype html><html data-cf-theme="slate" data-cf-mode="${mode}" data-cf-mode-resolved="${resolved}"><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:; media-src data:; frame-src 'self' blob:; form-action 'none'"><style>${sourceStyles}\n${exportFallback}</style></head><body><main id="cf-present-document"><h1>Static export</h1><iframe sandbox title="Static export sandbox" srcdoc="&lt;p&gt;Static export sandbox content&lt;/p&gt;"></iframe><iframe sandbox title="Static export attack" style="width:1px;height:1px;position:fixed;left:0;top:0;opacity:0" srcdoc="&lt;meta http-equiv='refresh' content='0;url=https://example.invalid/export-escape'&gt;&lt;img src='https://example.invalid/export-pixel.png'&gt;&lt;p&gt;Static export attack stayed local&lt;/p&gt;"></iframe></main></body></html>`;
}

function assertLoopbackOnly(requests) {
  // WebKit lists the page's own blob: URLs (crops) as responses; a blob
  // belongs to the origin inside it.
  const host = (request) => (request.protocol === "blob:" ? new URL(request.pathname).hostname : request.hostname);
  const remote = requests.find((request) => host(request) !== "127.0.0.1");
  if (remote) throw new Error(`Non-loopback request observed: ${remote.href}`);
}

// A text selection opens the chip only after the page maps it into one review
// text root. Name the selection and the page's status when that never happens.
async function waitForTextChip(page, label) {
  try {
    await page.getByTestId("float-chip").waitFor({ state: "attached", timeout: 5000 });
  } catch (error) {
    const state = await page.evaluate(() => {
      const selection = getSelection();
      const range = selection?.rangeCount ? selection.getRangeAt(0) : null;
      const where = (node, offset) => {
        const element = node?.nodeType === Node.ELEMENT_NODE ? node : node?.parentElement;
        const textRoot = element?.closest("[data-cf-review-text-root]");
        return { node: node?.nodeName, id: element?.id || null, offset, textRoot: textRoot?.id ?? null };
      };
      return {
        quote: String(selection),
        ranges: selection?.rangeCount ?? 0,
        start: range && where(range.startContainer, range.startOffset),
        end: range && where(range.endContainer, range.endOffset),
        status: document.querySelector(".cf-status")?.textContent?.trim() ?? null,
        hint: document.querySelector("[data-testid=comment-hint]")?.textContent?.trim() ?? null,
      };
    });
    throw new Error(`${label} opened no comment chip; page state ${JSON.stringify(state)}`, { cause: error });
  }
}

async function findBrowser() {
  const candidates = [
    process.env.CF_PRESENT_BROWSER,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
  ].filter(Boolean);
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {
      // Continue to the next explicit executable candidate.
    }
  }
  throw new Error("No qualified browser executable found; set CF_PRESENT_BROWSER");
}
