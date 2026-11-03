import assert from "node:assert/strict";
import { chromium } from "playwright-core";
import { installedChrome } from "./browser-executables.mjs";
import { assertNoPolicyViolations, recordPolicyViolationsOutsidePage } from "./csp-violations.mjs";

const browser = await chromium.launch({ executablePath: await installedChrome(), headless: true });
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
  let refused;
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    try { await assertNoPolicyViolations(page, "scriptless frame negative control"); }
    catch (error) { refused = error; break; }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  assert.match(refused?.message ?? "", /recorded CSP violations.*csp-negative\.png/u,
    "a blocked resource in scriptless srcdoc must fail the policy check");
  const unrecorded = await browser.newPage();
  await assert.rejects(assertNoPolicyViolations(unrecorded, "unrecorded page"), /did not record CSP violations/u);
  process.stdout.write("CSP recorder controls passed: scriptless frame resource refused, missing recorder refused\n");
} finally {
  await browser.close();
}
