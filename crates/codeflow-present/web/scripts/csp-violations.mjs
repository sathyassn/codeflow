// Every present browser suite records the page's securitypolicyviolation
// events from the first script on and fails on any: a page that needs a
// source its policy does not grant is a defect, not a console warning.

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
  const violations = await page.evaluate(() => globalThis.__cfPolicyViolations);
  if (!Array.isArray(violations)) throw new Error(`${label} did not record CSP violations`);
  if (violations.length) throw new Error(`${label} recorded CSP violations: ${JSON.stringify(violations)}`);
}
