import { createServer } from "node:http";
import { access, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { chromium } from "playwright-core";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const assetsRoot = resolve(webRoot, "../assets");
const manifest = JSON.parse(await readFile(join(assetsRoot, "manifest.json"), "utf8"));
const assets = new Map(manifest.service.assets.map((asset) => [asset.request_path, asset]));
const appPath = manifest.service.entrypoints["present.app"];
const stylePath = manifest.service.entrypoints["present.style"];
const prepaint = manifest.service.inline["present.prepaint"].source;
const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    if (url.pathname === "/app") {
      response.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" });
      response.end(fixtureHtml(url.searchParams.get("case") === "prose"));
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
  await checkProseLazyPath(browser, origin);
  await checkInteractiveSurface(browser, origin);
  process.stdout.write("cf-present browser checks passed: lazy paths, modes, selection, diagrams, axe, and 320 px reflow\n");
} finally {
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
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
      .imports.filter((item) => item.kind === "dynamic-import")
      .map((item) => item.request_path),
  );
  if (requests.some((request) => dynamicPaths.has(request.pathname))) {
    throw new Error("A prose-only page requested a syntax or Mermaid entry path");
  }
  assertLoopbackOnly(requests);
  await context.close();
}

async function checkInteractiveSurface(browser, origin) {
  const context = await browser.newContext({ viewport: { width: 320, height: 760 }, colorScheme: "light" });
  const page = await context.newPage();
  const requests = [];
  page.on("request", (request) => requests.push(new URL(request.url())));
  page.on("console", (message) => {
    if (message.type() === "error") throw new Error(`Browser console error: ${message.text()}`);
  });
  await page.goto(`${origin}/app`, { waitUntil: "networkidle" });
  const code = page.locator("code[data-cf-language='rust']");
  await code.scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelector("code[data-cf-language='rust']")?.getAttribute("data-cf-highlight") === "ready");
  const diagram = page.locator("[data-cf-diagram]");
  await diagram.scrollIntoViewIfNeeded();
  await page.locator("[data-cf-diagram='ready'] svg[role='img']").waitFor();

  const pageOverflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
  if (pageOverflow) throw new Error("The review surface overflows at 320 CSS px");

  await page.evaluate(() => {
    const root = document.getElementById("cf-present-document");
    if (!root) throw new Error("Document root missing");
    globalThis.__cfDocumentRoot = root;
    const text = document.getElementById("selection-target")?.firstChild;
    if (!text) throw new Error("Selection fixture missing");
    const range = document.createRange();
    range.setStart(text, 0);
    range.setEnd(text, 11);
    const selection = getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
  });
  await page.getByRole("button", { name: /^Review /u }).click();
  await page.getByRole("button", { name: "Add selected text" }).click();
  await page.locator(".cf-notes textarea").fill("Keep this exact wording.");
  await page.getByLabel("Theme").selectOption("technical");
  const identityPreserved = await page.evaluate(() => globalThis.__cfDocumentRoot === document.getElementById("cf-present-document"));
  if (!identityPreserved) throw new Error("Review chrome replaced the Rust-owned document root");

  await page.addScriptTag({ content: axe.source });
  for (const theme of ["editorial", "technical"]) {
    await page.getByLabel("Theme").selectOption(theme);
    for (const mode of ["light", "dark"]) {
      await page.getByLabel("Mode").selectOption(mode);
      const axeResult = await page.evaluate(async () => globalThis.axe.run(document, {
        runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"] },
      }));
      if (axeResult.violations.length) {
        throw new Error(`${theme}/${mode} axe violations: ${axeResult.violations.map((item) => item.id).join(", ")}`);
      }
    }
  }
  assertLoopbackOnly(requests);
  await context.close();
}

function fixtureHtml(proseOnly) {
  const enhancements = proseOnly
    ? ""
    : `<section data-cf-block-id="block-code" data-cf-block-label="Implementation">
        <h2 id="implementation">Implementation</h2>
        <div data-cf-review-text-root><p id="selection-target">Review this exact sentence before approval.</p></div>
        <pre tabindex="0" role="region" aria-label="Rust example"><code data-cf-language="rust">fn main() { println!("safe"); }</code></pre>
      </section>
      <section data-cf-block-id="block-flow" data-cf-block-label="Flow">
        <h2 id="flow">Flow</h2>
        <div class="cf-local-scroll" tabindex="0" role="region" aria-label="Request flow diagram" data-cf-diagram="pending" data-cf-diagram-title="Request flow" data-cf-diagram-description="A request moves from input to review.">
          <template data-cf-diagram-source>flowchart LR
            A[Input] --> B[Review]</template>
          <div data-cf-diagram-output></div>
          <p data-cf-diagram-status aria-live="polite">Rendering diagram…</p>
        </div>
      </section>`;
  const config = JSON.stringify({
    schema_version: 1,
    session_id: "019f9b53-a341-7fa7-84c2-5f198ceea001",
    revision: 1,
    title: proseOnly ? "Plain-language review" : "Runtime review",
    shortcuts_enabled: true,
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
</head>
<body>
  <div id="cf-present-chrome" data-session-id="019f9b53-a341-7fa7-84c2-5f198ceea001"></div>
  <main id="cf-present-document">
    <header><p>Outcome</p><h1>${proseOnly ? "A focused explanation" : "A bounded review runtime"}</h1></header>
    <section data-cf-block-id="block-summary" data-cf-block-label="Summary">
      <h2 id="summary">Summary</h2>
      <div data-cf-review-text-root><p>The document remains readable without its review controls.</p></div>
    </section>
    ${enhancements}
  </main>
  <script id="cf-present-config" type="application/json">${config}</script>
  <script type="module" src="${appPath}"></script>
</body>
</html>`;
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
