// Fixture suites record DOM securitypolicyviolation events from init scripts.
// Scriptless live frames use Playwright's external console/pageerror stream,
// including child targets. Both paths refuse errors instead of muting them.

const externalRecorders = new WeakMap();

// Playwright recursively attaches iframe targets before resuming them and
// forwards their Log/Runtime errors to the owning Page. Register before
// navigation so later child targets and reloads retain the same coverage.
// Refuse every error, as the real-browser check already does; do not depend
// on matching a browser's CSP wording or inject scripts into sandboxed frames.
export async function recordPolicyViolationsOutsidePage(page) {
  if (externalRecorders.has(page)) return;
  const violations = [];
  page.on("console", (message) => {
    if (message.type() === "error") {
      violations.push({ kind: "console", text: message.text(), location: message.location() });
    }
  });
  page.on("pageerror", (error) => {
    violations.push({ kind: "pageerror", text: error.message });
  });
  externalRecorders.set(page, violations);
}

export async function recordPolicyViolations(target) {
  await target.addInitScript(() => {
    globalThis.__cfPolicyViolations = [];
    document.addEventListener("securitypolicyviolation", (event) => {
      globalThis.__cfPolicyViolations.push({
        directive: event.effectiveDirective,
        blocked: event.blockedURI,
        sample: event.sample,
      });
    });
  });
}

export async function assertNoPolicyViolations(page, label) {
  const violations = externalRecorders.has(page)
    ? externalRecorders.get(page)
    : await page.evaluate(() => globalThis.__cfPolicyViolations);
  if (!Array.isArray(violations)) throw new Error(`${label} did not record CSP violations`);
  // Firefox fetches /favicon.ico for its tab itself; the page never does.
  const pageViolations = violations.filter((violation) => !(violation.directive === "img-src" && new URL(violation.blocked, "http://x").pathname === "/favicon.ico"));
  if (pageViolations.length) throw new Error(`${label} recorded CSP violations: ${JSON.stringify(pageViolations)}`);
}
