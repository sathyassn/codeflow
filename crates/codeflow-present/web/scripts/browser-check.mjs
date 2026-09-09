import { createServer } from "node:http";
import { access, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { chromium } from "playwright-core";
import { checkSelectionOccurrences } from "./selection-browser-check.mjs";
import { checkDocumentExcerpts } from "./excerpt-browser-check.mjs";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const assetsRoot = resolve(webRoot, "../assets");
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
      response.end(fixtureHtml(url.searchParams.get("case") === "prose"));
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
const executablePath = await findBrowser();
const browser = await chromium.launch({ executablePath, headless: true });

try {
  await checkSelectionOccurrences(browser);
  await checkDocumentExcerpts(browser);
  await checkProseLazyPath(browser, origin);
  await checkInteractiveSurface(browser, origin, reviewPosts);
  await checkStaticExportModes(browser, origin);
  process.stdout.write("cf-present browser checks passed: lazy paths, interactive and no-script modes, selection, diagrams, axe, and 320 px reflow\n");
} finally {
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
}

async function checkStaticExportModes(browser, origin) {
  for (const { mode, preference, expected } of [
    { mode: "system", preference: "dark", expected: "rgb(23, 21, 19)" },
    { mode: "system", preference: "light", expected: "rgb(246, 243, 238)" },
    { mode: "dark", preference: "light", expected: "rgb(23, 21, 19)" },
    { mode: "light", preference: "dark", expected: "rgb(246, 243, 238)" },
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
      // pages; heavyweight syntax/diagram renderers must remain lazy.
      .imports.filter((item) => item.kind === "dynamic-import" && !/\/chunk-fonts-[^/]+\.js$/u.test(item.request_path))
      .map((item) => item.request_path),
  );
  if (requests.some((request) => dynamicPaths.has(request.pathname))) {
    throw new Error("A prose-only page requested a syntax or Mermaid entry path");
  }
  assertLoopbackOnly(requests);
  await context.close();
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
  await page.addInitScript(() => {
    globalThis.__cfPolicyViolations = [];
    globalThis.__cfLongTasks = [];
    document.addEventListener("securitypolicyviolation", (event) => {
      globalThis.__cfPolicyViolations.push({
        directive: event.effectiveDirective,
        blocked: event.blockedURI,
        sample: event.sample,
      });
    });
    if ("PerformanceObserver" in globalThis) {
      new PerformanceObserver((entries) => {
        globalThis.__cfLongTasks.push(...entries.getEntries().map((entry) => entry.duration));
      }).observe({ type: "longtask", buffered: true });
    }
  });
  await page.addInitScript({ content: axe.source });
  await page.goto(`${origin}/app`, { waitUntil: "networkidle" });
  await assertBundledFonts(page);
  await page.getByRole("button", { name: /Comment/ }).click();
  await page.getByTestId("feedback-history").waitFor();
  await page.getByTestId("feedback-history").locator("summary").click();
  await page.getByText("Matched uniquely in this revision.").waitFor();
  await page.getByText("Unpositioned: the referenced block is absent").waitFor();
  await page.getByRole("button", { name: "Close" }).click();
  const code = page.locator("code[data-cf-language='rust']");
  await code.scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("code[data-cf-language='rust']")?.getAttribute("data-cf-highlight") === "ready");
  const diagram = page.locator("[data-cf-diagram-title='Request flow']");
  await diagram.scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("[data-cf-diagram]")?.getAttribute("data-cf-diagram") !== "pending");
  if (await diagram.getAttribute("data-cf-diagram") !== "ready") {
    throw new Error(`Diagram did not render: ${await diagram.locator("[data-cf-diagram-status]").textContent()}`);
  }
  const diagramSvg = diagram.locator("svg[role='img']");
  await diagramSvg.waitFor();
  const diagramEvidence = await diagramSvg.evaluate((svg) => ({
    text: svg.textContent?.replace(/\s+/gu, " ").trim() ?? "",
    foreignObjects: svg.querySelectorAll("foreignObject").length,
    scripts: svg.querySelectorAll("script").length,
    externalLinks: [...svg.querySelectorAll("a")].filter((link) => {
      const href = link.getAttribute("href") ?? link.getAttribute("xlink:href") ?? "";
      return /^(?:https?:)?\/\//iu.test(href);
    }).length,
    visibleLabels: [...svg.querySelectorAll("text")].filter((label) => {
      const box = label.getBoundingClientRect();
      const style = getComputedStyle(label);
      return box.width > 0 && box.height > 0 && style.visibility !== "hidden" && style.display !== "none";
    }).map((label) => label.textContent?.trim() ?? ""),
  }));
  if (!diagramEvidence.text.includes("Input") || !diagramEvidence.text.includes("Review")) {
    throw new Error(`Diagram lost its semantic labels during rendering: ${JSON.stringify(diagramEvidence)}`);
  }
  if (!diagramEvidence.visibleLabels.includes("Input") || !diagramEvidence.visibleLabels.includes("Review")) {
    throw new Error(`Diagram labels are present but not visibly rendered: ${JSON.stringify(diagramEvidence)}`);
  }
  if (diagramEvidence.foreignObjects || diagramEvidence.scripts || diagramEvidence.externalLinks) {
    throw new Error(`Diagram hardening left an unsafe node: ${JSON.stringify(diagramEvidence)}`);
  }
  const denseDiagram = page.locator("[data-cf-diagram-title='Dense flow']");
  const denseStarted = Date.now();
  await denseDiagram.scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("[data-cf-diagram-title='Dense flow']")?.getAttribute("data-cf-diagram") !== "pending", null, { timeout: 15_000 });
  if (await denseDiagram.getAttribute("data-cf-diagram") !== "ready") {
    throw new Error(`Dense diagram did not render: ${await denseDiagram.locator("[data-cf-diagram-status]").textContent()}`);
  }
  const denseMetrics = await page.evaluate(() => ({
    longestTask: Math.max(0, ...globalThis.__cfLongTasks),
  }));
  if (Date.now() - denseStarted > 10_000 || denseMetrics.longestTask > 5_000) {
    throw new Error(`Dense diagram exceeded the responsiveness envelope: ${JSON.stringify(denseMetrics)}`);
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
    diagram: (() => {
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
  await page.locator("#cf-feedback-panel[data-open='true']").waitFor();
  await page.getByText("Nothing noted yet").waitFor();

  // Drag starting on the prose wrapper (padding around the paragraph) must stay
  // Text. Missing that hit-test is how region marquees steal text selection.
  {
    const prose = page.locator("#cf-present-document [data-cf-review-text-root]").first();
    const box = await prose.boundingBox();
    if (!box) throw new Error("Prose review-text-root has no box");
    await page.mouse.move(box.x + 8, box.y + 4);
    await page.mouse.down();
    await page.mouse.move(box.x + 160, box.y + 22, { steps: 10 });
    const marquee = await page.locator(".cf-region-draft").count();
    await page.mouse.up();
    await page.getByTestId("float-chip").waitFor({ state: "attached", timeout: 5000 });
    const kind = (await page.getByTestId("float-chip").locator(".lab").innerText()).trim();
    if (marquee !== 0) throw new Error("Prose drag drew a region marquee");
    if (kind !== "Text") throw new Error(`Prose drag opened ${kind}, expected Text`);
    await page.keyboard.press("Escape");
    await page.getByTestId("float-chip").waitFor({ state: "detached", timeout: 5000 }).catch(() => {});
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
    await page.getByTestId("float-chip").waitFor({ state: "attached", timeout: 5000 });
    const kind = (await page.getByTestId("float-chip").locator(".lab").innerText()).trim();
    if (kind !== "Text") throw new Error(`Stage label selection opened ${kind}, expected Text`);
    await page.keyboard.press("Escape");
    await page.getByTestId("float-chip").waitFor({ state: "detached", timeout: 5000 }).catch(() => {});
  }

  async function clickTool(testId) {
    await page.evaluate(() => {
      const tools = document.querySelector("details.cf-tools");
      if (tools) {
        tools.open = true;
        tools.scrollIntoView({ block: "end" });
      }
    });
    await page.getByTestId(testId).click({ force: true });
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
    await page.getByTestId("composer-save").click({ force: true });
    await page.getByTestId("composer").waitFor({ state: "detached", timeout: 10000 });
  }

  // Text note (tools open composer directly)
  await selectFixtureText(page);
  await clickTool("tool-add-text");
  await saveComposerNote("Keep this exact wording.");
  if ((await page.locator(".cf-note-row").count()) !== 1) throw new Error("Text note did not land in the rail");
  if ((await page.locator(".cf-marker").count()) !== 1) throw new Error("Speech marker missing for text note");

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
  if ((await page.locator("#cf-feedback-panel").getAttribute("data-open")) !== "true") {
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
    await page.getByTestId("submit-all").click();
    await page.getByTestId("toast").getByText(/Review received/).waitFor({ timeout: 10000 });
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
    if (kind !== "Region") throw new Error(`Shift+drag on prose opened ${kind}, expected Region`);
    return kind;
  }

  // Shift+drag on prose is Region (plain drag on the same node is Text, tested above).
  await shiftDragRegion(page.locator("#selection-target"));
  await page.keyboard.press("Escape");
  await page.getByTestId("float-chip").waitFor({ state: "detached", timeout: 5000 }).catch(() => {});

  await clickTool("tool-pick-element");
  await page.locator("#cf-present-document[data-cf-capture-mode='element']").waitFor();
  await page.locator("#cf-present-document[data-cf-capture-mode='element'] :focus").waitFor();
  await page.keyboard.press("Enter");
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
  await page.locator("[data-testid=skin-pills] [data-skin=instrument]").click();
  const identityPreserved = await page.evaluate(() => globalThis.__cfDocumentRoot === document.getElementById("cf-present-document"));
  if (!identityPreserved) throw new Error("Review chrome replaced the Rust-owned document root");

  await page.locator("[data-testid=scale-pills] [data-scale=large]").click();
  await page.getByTestId("typeface-plex").click();
  await page.waitForFunction(() =>
    document.documentElement.dataset.cfScale === "large"
    && document.documentElement.dataset.cfTypeface === "plex"
  );
  await page.locator("[data-testid=scale-pills] [data-scale=default]").click();
  await page.locator("[data-testid=typeface-pills] [data-typeface=instrument]").click();

  for (const theme of ["editorial", "instrument", "ink"]) {
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

function fixtureHtml(proseOnly) {
  const denseSource = denseDiagramSource(300);
  const enhancements = proseOnly
    ? ""
    : `<section data-cf-block-id="block-code" data-cf-block-label="Implementation" data-cf-block-digest="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa">
        <h2 id="implementation">Implementation</h2>
        <div data-cf-review-text-root><p id="selection-target">Review this exact sentence before approval.</p>
        <figure role="img" aria-label="Stage fixture"><svg viewBox="0 0 280 36" width="280" height="36"><text id="stage-label" x="8" y="24">Stage words</text></svg></figure></div>
        <pre tabindex="0" role="region" aria-label="Rust example"><code data-cf-language="rust">fn main() { println!("safe"); }</code></pre>
      </section>
      <section class="block block--diagram" data-cf-block-id="block-flow" data-cf-block-label="Flow" data-cf-block-digest="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb">
        <h2 id="flow">Flow</h2>
        <div class="cf-local-scroll" tabindex="0" role="region" aria-label="Request flow diagram" data-cf-diagram="pending" data-cf-diagram-title="Request flow" data-cf-diagram-description="A request moves from input to review.">
          <template data-cf-diagram-source>flowchart LR
            A[Input] --> B[Review]</template>
          <div data-cf-diagram-output></div>
          <p data-cf-diagram-status aria-live="polite">Rendering diagram…</p>
        </div>
        <div class="cf-local-scroll" tabindex="0" role="region" aria-label="Dense flow diagram" data-cf-diagram="pending" data-cf-diagram-title="Dense flow" data-cf-diagram-description="A bounded dense flow exercises the renderer responsiveness envelope.">
          <template data-cf-diagram-source>${denseSource}</template>
          <div data-cf-diagram-output></div>
          <p data-cf-diagram-status aria-live="polite">Rendering diagram…</p>
        </div>
      </section>`;
  const sandbox = proseOnly ? "" : '<figure><iframe sandbox title="Sandbox fixture" src="/sandbox/1/fixture"></iframe></figure>';
  const attackSandbox = proseOnly ? "" : '<iframe sandbox title="Attack sandbox" src="/sandbox/1/attack" style="width:1px;height:1px;position:fixed;left:0;top:0;opacity:0"></iframe>';
  const config = JSON.stringify({
    schema_version: 1,
    session_id: "019f9b53-a341-7fa7-84c2-5f198ceea001",
    revision: 1,
    event_sequence: 0,
    title: proseOnly ? "Plain-language review" : "Runtime review",
    shortcuts_enabled: true,
    review_limits: {
      max_notes: 2,
      max_visible_feedback: 256,
      max_text_utf16: 32,
      max_selector_utf16: 16,
      max_payload_bytes: 262144,
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
      <div data-cf-review-text-root><p>The document remains readable without its review controls.</p></div>
    </section>
    ${enhancements}
    ${sandbox}
    ${attackSandbox}
  </main>
  <template id="cf-present-config">${config}</template>
  <script type="module" src="${appPath}"></script>
</body>
</html>`;
}

function denseDiagramSource(edges) {
  return ["flowchart LR", ...Array.from({ length: edges }, (_, index) => `N${index}-->N${index + 1}`)].join("\n");
}

function exportFixture(mode) {
  const resolved = mode === "dark" ? "dark" : "light";
  return `<!doctype html><html data-cf-theme="editorial" data-cf-mode="${mode}" data-cf-mode-resolved="${resolved}"><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:; media-src data:; frame-src 'self' blob:; form-action 'none'"><style>${sourceStyles}\n${exportFallback}</style></head><body><main id="cf-present-document"><h1>Static export</h1><iframe sandbox title="Static export sandbox" srcdoc="&lt;p&gt;Static export sandbox content&lt;/p&gt;"></iframe><iframe sandbox title="Static export attack" style="width:1px;height:1px;position:fixed;left:0;top:0;opacity:0" srcdoc="&lt;meta http-equiv='refresh' content='0;url=https://example.invalid/export-escape'&gt;&lt;img src='https://example.invalid/export-pixel.png'&gt;&lt;p&gt;Static export attack stayed local&lt;/p&gt;"></iframe></main></body></html>`;
}

function assertLoopbackOnly(requests) {
  const remote = requests.find((request) => request.hostname !== "127.0.0.1");
  if (remote) throw new Error(`Non-loopback request observed: ${remote.href}`);
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
