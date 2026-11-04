import assert from "node:assert/strict";
import { chromium } from "playwright-core";
import { installedChrome } from "./browser-executables.mjs";
import { assertNoPolicyViolations, recordPolicyViolationsOutsidePage } from "./csp-violations.mjs";

async function expectBlockedResource(page, label, resource) {
  let refused;
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    try { await assertNoPolicyViolations(page, label); }
    catch (error) { refused = error; break; }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  assert.ok(refused?.message.includes("recorded CSP violations") && refused.message.includes(resource),
    `${label} must refuse ${resource}, got ${refused?.message ?? "no rejection"}`);
}

const browser = await chromium.launch({ executablePath: await installedChrome(), headless: true, args: ["--site-per-process"] });
try {
  const page = await browser.newPage();
  // Fail immediately if recording ever goes back to injecting frame scripts.
  page.addInitScript = () => { throw new Error("CSP recording must stay outside the page"); };
  page.context().addInitScript = page.addInitScript;
  await recordPolicyViolationsOutsidePage(page);
  await page.setContent('<!doctype html><title>CSP recorder control</title><iframe sandbox="allow-same-origin" title="Scriptless stage"></iframe>');
  await assertNoPolicyViolations(page, "clean scriptless frame");

  const source = '<meta http-equiv="Content-Security-Policy" content="default-src &#39;none&#39;"><img src="https://example.invalid/csp-negative.png" alt="Blocked control">';
  await page.locator("iframe").evaluate((frame, html) => { frame.srcdoc = html; }, source);
  assert.equal(await page.locator("iframe").getAttribute("sandbox"), "allow-same-origin");
  assert.ok(!source.includes("<script"));
  await expectBlockedResource(page, "scriptless frame negative control", "csp-negative.png");

  // Intercepted HTTP responses retain real cross-site origins, with no
  // external server. Require separate targets instead of assuming isolation.
  const crossSite = await browser.newPage();
  await recordPolicyViolationsOutsidePage(crossSite);
  await crossSite.route("http://*.test/**", async (route) => {
    const url = new URL(route.request().url());
    const body = url.hostname === "grandchild.test"
      ? '<title>Scriptless descendant</title><img src="https://example.invalid/child-target-csp-negative.png" alt="Blocked descendant">'
      : url.pathname === "/nested"
        ? '<title>Scriptless child</title><iframe sandbox="allow-same-origin" src="http://grandchild.test/"></iframe>'
        : '<title>Clean cross-site control</title>';
    await route.fulfill({
      contentType: "text/html",
      headers: { "Content-Security-Policy": "default-src 'none'; frame-src http://child.test http://grandchild.test" },
      body: `<!doctype html>${body}`,
    });
  });
  await crossSite.goto("http://parent.test/");
  await assertNoPolicyViolations(crossSite, "clean parent before child creation");
  const childNavigation = crossSite.waitForEvent("framenavigated", { predicate: (frame) => frame.url() === "http://child.test/" });
  await crossSite.evaluate(() => {
    const frame = document.createElement("iframe");
    frame.sandbox.add("allow-same-origin");
    frame.src = "http://child.test/";
    document.body.append(frame);
  });
  const child = await childNavigation;
  const parentSession = await crossSite.context().newCDPSession(crossSite);
  const childSession = await crossSite.context().newCDPSession(child);
  const parentInfo = (await parentSession.send("Target.getTargetInfo")).targetInfo;
  const childInfo = (await childSession.send("Target.getTargetInfo")).targetInfo;
  assert.equal(childInfo.type, "iframe", "control requires an actual OOPIF target");
  assert.notEqual(childInfo.targetId, parentInfo.targetId);
  await assertNoPolicyViolations(crossSite, "clean separate child target");

  // A descendant created after recording started must be covered too.
  const descendantNavigation = crossSite.waitForEvent("framenavigated", { predicate: (frame) => frame.url() === "http://grandchild.test/" });
  await crossSite.locator("iframe").evaluate((frame) => { frame.src = "http://child.test/nested"; });
  const descendant = await descendantNavigation;
  const descendantSession = await crossSite.context().newCDPSession(descendant);
  const descendantInfo = (await descendantSession.send("Target.getTargetInfo")).targetInfo;
  assert.equal(descendantInfo.type, "iframe", "control requires an actual descendant OOPIF target");
  assert.notEqual(descendantInfo.targetId, childInfo.targetId);
  assert.notEqual(descendantInfo.targetId, parentInfo.targetId);
  await expectBlockedResource(crossSite, "later descendant target negative control", "child-target-csp-negative.png");
  const unrecorded = await browser.newPage();
  await assert.rejects(assertNoPolicyViolations(unrecorded, "unrecorded page"), /did not record CSP violations/u);
  process.stdout.write("CSP recorder controls passed: scriptless srcdoc and later descendant target resources refused, missing recorder refused\n");
} finally {
  await browser.close();
}
