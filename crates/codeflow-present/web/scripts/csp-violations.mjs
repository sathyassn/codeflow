// Every present browser suite records the page's securitypolicyviolation
// events from the first script on and fails on any: a page that needs a
// source its policy does not grant is a defect, not a console warning.

const externalRecorders = new WeakMap();

// Chromium's live measuring frames forbid scripts. Observe their policy
// issues through the page target instead of injecting code into each frame.
// Install and enable before navigation, including on pages that later reload.
export async function recordPolicyViolationsOutsidePage(page) {
  if (externalRecorders.has(page)) return;
  const session = await page.context().newCDPSession(page);
  const violations = [];
  session.on("Audits.issueAdded", ({ issue }) => {
    const details = issue.details.contentSecurityPolicyIssueDetails;
    if (!details) return;
    violations.push({
      directive: details.violatedDirective.split(/\s/u)[0],
      blocked: details.blockedURL ?? "",
      sample: details.contentSecurityPolicyViolationType,
      details,
    });
  });
  await session.send("Audits.enable");
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
