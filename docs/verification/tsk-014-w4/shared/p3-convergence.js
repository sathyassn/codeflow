// Shared subject source for the P3 candidates: the cf-present qualification
// review convergence.
//
// It carries facts, relationships, verbatim quotations and the derivation of the
// answer — and nothing else. No markup, no CSS, no visual primitive, no token.
// Both P3 candidates read this one object, so they cannot disagree about the
// subject and can be compared on encoding alone.
//
// tools/verify.mjs binds every fact here to the repository:
//   - each round's sha, subject and date against `git log -1`;
//   - every diff line against `git show <sha> -- <path>`, character for
//     character, including the leading +/-/space;
//   - every quotation against the file it declares, whitespace-normalised;
//   - the derived answer against an independent recomputation from `findings`.
//
// Cell state vocabulary (closed set):
//   opened    the finding first appears together with its fix
//   reopened  the previous fix did not hold; carries `cause` and `answers`
//   closed    the finding's final fix landed in this round
//   extended  this round widened what the check proves; the earlier fix did NOT
//             fail. Deliberately distinct from `reopened` — the answer depends
//             on the difference
//   residual  a bounded remainder recorded in this round and still open
window.p3Convergence = (() => {
  const record = "project-management/tasks/TSK-014.md";
  const harness = "crates/codeflow-present/web/scripts/real-browser-check.mjs";
  const cleanup = "crates/codeflow-present/web/scripts/real-browser-cleanup-check.mjs";
  const manifest = "crates/codeflow-present/web/package.json";
  const qualification = "docs/verification/tsk-007-presentation/README.md";

  const rounds = [
    { id: "R1", sha: "86582dc0", subject: "test(present): qualify owned runtime boundaries", date: "2026-08-02" },
    { id: "R2", sha: "74feaf04", subject: "test(present): harden qualification cleanup", date: "2026-08-02" },
    { id: "R3", sha: "82671551", subject: "test(present): close qualification review gaps", date: "2026-08-02" },
    { id: "R4", sha: "14107e48", subject: "test(present): tolerate cold browser startup", date: "2026-08-03" },
    { id: "R5", sha: "c5ede869", subject: "test(present): follow bootstrap readiness semantics", date: "2026-08-03" },
    { id: "R6", sha: "30133fd7", subject: "test(present): bound cold renderer startup", date: "2026-08-03" },
    { id: "R7", sha: "e194b1fd", subject: "fix(present): harden browser close recovery", date: "2026-08-03" },
    { id: "R8", sha: "a47b62c3", subject: "fix(present): align browser qualification driver", date: "2026-08-03" },
  ];

  const findings = [
    {
      id: "F1",
      title: "the qualification child inherited unrelated host values",
      cells: [
        {
          round: "R2", states: ["opened"],
          what: "the whole-host spread is replaced by an explicit allow-list, with an assertion that nothing unrelated leaks in",
          hunk: {
            path: harness, header: "@@ -26,7 +36,7 @@",
            lines: [
              "   ? resolve(process.env.CF_PRESENT_EVIDENCE_DIR)",
              "   : null;",
              " const childEnvironment = {",
              "-  ...process.env,",
              "+  ...allowedEnvironment(functionalEnvironmentNames),",
              "   HOME: home,",
              "   XDG_STATE_HOME: join(home, \"state\"),",
            ],
          },
        },
        {
          round: "R3", states: ["reopened", "closed"], cause: "previous-fix", answers: "R2",
          what: "the allow-list carried only the shouted spelling; Windows sets SystemRoot, so the confinement it claimed had a hole",
          hunk: {
            path: harness, header: "@@ -14,7 +15,7 @@",
            lines: [
              " const functionalEnvironmentNames = [",
              "   \"APPDATA\", \"CARGO_HOME\", \"COMSPEC\", \"HOME\", \"LANG\", \"LC_ALL\", \"LC_CTYPE\",",
              "-  \"LOCALAPPDATA\", \"PATHEXT\", \"PATH\", \"RUSTUP_HOME\", \"SYSTEMROOT\", \"TEMP\", \"TERM\",",
              "+  \"LOCALAPPDATA\", \"PATHEXT\", \"PATH\", \"RUSTUP_HOME\", \"SystemRoot\", \"SYSTEMROOT\", \"TEMP\", \"TERM\",",
              "   \"TMP\", \"TMPDIR\", \"USERPROFILE\", \"WINDIR\", \"XDG_RUNTIME_DIR\",",
              " ];",
            ],
          },
        },
      ],
      quotes: [],
    },
    {
      id: "F2",
      title: "cleanup swallowed the primary error and its own failures",
      cells: [
        {
          round: "R2", states: ["opened", "closed"],
          what: "the primary error is captured before cleanup runs, and every cleanup step accumulates its own failure instead of discarding it",
          hunk: {
            path: harness, header: "@@ -238,60 +259,110 @@",
            lines: [
              "+} catch (error) {",
              "+  primaryError = error;",
              " } finally {",
              "+  const cleanupErrors = [];",
              "+  const attempt = async (phase, operation) => {",
              "+    try {",
              "+      await operation();",
              "+    } catch (error) {",
              "+      cleanupErrors.push(new Error(`Cleanup failed during ${phase}: ${error.message}`, { cause: error }));",
              "+    }",
              "+  };",
            ],
          },
        },
      ],
      quotes: [
        {
          text: "injected close-timeout and primary-failure checks proved cleanup recovery and primary-error preservation without leaving a task-owned root",
          source: record,
        },
      ],
    },
    {
      id: "F3",
      title: "nothing proved that an injected failure still cleans up",
      cells: [
        {
          round: "R2", states: ["opened", "closed"],
          what: "a deliberate mid-journey failure becomes an argument, and a separate check runs the journey with it and inspects what survived",
          hunk: {
            path: harness, header: "@@ -107,6 +122,9 @@",
            lines: [
              "+  if (injectCleanupFailure) {",
              "+    throw new Error(\"injected real-browser cleanup failure\");",
              "+  }",
            ],
          },
        },
        {
          round: "R7", states: ["extended"],
          what: "the same check gains a second injection: an injected close timeout must recover using exactly one exact-owned fallback",
          hunk: {
            path: cleanup, header: "@@ -33,7 +33,35 @@",
            lines: [
              "+  if (results.checks?.browser_close_fallbacks !== 1) {",
              "+    throw new Error(`Injected close timeout did not use one exact-owned fallback: ${JSON.stringify(results)}`);",
              "+  }",
            ],
          },
        },
        {
          round: "R8", states: ["extended"],
          what: "and a third assertion: the recovered run must have recorded the browser and Playwright versions it was driven with",
          hunk: {
            path: cleanup, header: "@@ -55,6 +62,12 @@",
            lines: [
              "+  if (",
              "+    !results.toolchain?.browser_version",
              "+    || results.toolchain?.playwright_core_version !== expectedPlaywrightCoreVersion",
              "+  ) {",
              "+    throw new Error(`Injected close timeout omitted its qualified browser toolchain: ${JSON.stringify(results)}`);",
              "+  }",
            ],
          },
        },
      ],
      quotes: [],
    },
    {
      id: "F4",
      title: "Windows profile confinement was unscoped",
      cells: [
        {
          round: "R3", states: ["opened", "closed", "residual"],
          what: "the guard is added and its scope is written into the result itself: confinement is claimed, teardown is explicitly not evaluated",
          hunk: {
            path: harness, header: "@@ -251,6 +272,10 @@",
            lines: [
              "+      windows_profile_confinement: windowsProfileConfinement ? \"pass\" : \"not applicable\",",
              "+      windows_profile_or_vm_teardown: windowsProfileConfinement",
              "+        ? \"not evaluated; outer native-Windows lane required\"",
              "+        : \"not applicable\",",
            ],
          },
        },
      ],
      residual: "start-time confinement only; no native Windows or WSL2 run exists to close it",
      quotes: [
        {
          text: "That guard proves start-time confinement, not account, profile, or VM teardown.",
          source: qualification,
        },
      ],
    },
    {
      id: "F5",
      title: "a cold runner outran the qualification's bounded waits",
      cells: [
        {
          round: "R4", states: ["opened"],
          what: "the bound is raised from 20 to 45 seconds — the number is treated as the problem",
          hunk: {
            path: harness, header: "@@ -495,7 +496,10 @@",
            lines: [
              "     observedPage.setDefaultTimeout(15_000);",
              "-    observedPage.setDefaultNavigationTimeout(20_000);",
              "+    // A cold shared runner may still be CPU-bound after the qualification's",
              "+    // release-build rehearsal. Keep navigation bounded, but leave enough room",
              "+    // for the local bootstrap and application assets to complete under load.",
              "+    observedPage.setDefaultNavigationTimeout(NAVIGATION_TIMEOUT_MS);",
            ],
          },
        },
        {
          round: "R5", states: ["reopened"], cause: "previous-fix", answers: "R4",
          what: "the number was never the problem: waiting for the bootstrap file's load event conflated the handoff with the destination page, so it could hang behind the application's own long-lived requests",
          hunk: {
            path: harness, header: "@@ -129,11 +129,7 @@",
            lines: [
              "   const page = context.pages()[0] ?? await context.newPage();",
              "-  await page.goto(pathToFileURL(bootstrapPath).href);",
              "-  await page.waitForURL(new RegExp(`^http://127\\\\.0\\\\.0\\\\.1:${port}/app/`, \"u\"));",
              "-  // The application keeps an authenticated long-poll open, so network-idle is",
              "-  // not a valid readiness signal. DOM readiness plus owned content is.",
              "-  await page.waitForLoadState(\"domcontentloaded\");",
              "+  await openAuthenticatedPresentation(page, bootstrapPath, port);",
              "   const initialText = await page.locator(\"body\").innerText();",
            ],
          },
        },
        {
          round: "R6", states: ["reopened", "closed"], cause: "previous-fix", answers: "R5",
          what: "the refactor had left both waits on defaults with no relation to the journey's own deadline; every wait is now capped by the time actually left in it",
          hunk: {
            path: harness, header: "@@ -748,6 +757,14 @@",
            lines: [
              "+function timeoutWithinQualification(ceiling, phase) {",
              "+  const remaining = qualificationDeadline - Date.now();",
              "+  if (remaining <= 0) {",
              "+    throw new Error(`Qualification exceeded its 180-second deadline before ${phase}`);",
              "+  }",
              "+  return Math.min(ceiling, remaining);",
              "+}",
            ],
          },
        },
      ],
      quotes: [],
    },
    {
      id: "F6",
      title: "the browser context close timed out with an earlier task-owned Chrome present",
      cells: [
        {
          round: "R7", states: ["opened"],
          what: "a bounded close gains an exact-owned TERM/KILL fallback, and the gate is made permanent: a normal run rejects any fallback at all",
          hunk: {
            path: harness, header: "@@ -228,8 +240,13 @@",
            lines: [
              "   tracingStarted = false;",
              "-  await closeBrowser(context, browserProfileV2, browserProcessesV2);",
              "+  browserCloseFallbacks += await closeBrowser(context, browserProfileV2, browserProcessesV2);",
              "   context = undefined;",
              "+  if (!injectCloseTimeout && browserCloseFallbacks > 0) {",
              "+    throw new Error(",
              "+      `Browser close required ${browserCloseFallbacks} exact-owned termination fallback(s)`,",
              "+    );",
              "+  }",
            ],
          },
        },
        {
          round: "R8", states: ["reopened", "closed", "residual"], cause: "outside-harness", answers: "R7",
          what: "the retry tripped that gate anyway, and the cause was not in the harness: it had been driving Chrome 150 through Playwright 1.55, outside that release's tested window",
          // A re-opening whose cause is not a round has an origin off the round
          // axis. Naming it is what lets a candidate draw the arrival from
          // somewhere rather than from nowhere; `tools/verify.mjs` requires the
          // label's identifying tokens to appear in the verbatim quotation.
          origin: {
            label: "the pinned browser toolchain",
            detail: "Chrome 150 × Playwright 1.55",
            tokens: ["Chrome 150", "Playwright 1.55"],
            quote: {
              text: "The qualification had been driving Chrome 150 through Playwright 1.55, outside that Playwright release's tested stable-browser window",
              source: record,
            },
          },
          hunk: {
            path: manifest, header: "@@ -27,7 +27,7 @@",
            lines: [
              "   \"devDependencies\": {",
              "     \"axe-core\": \"4.10.3\",",
              "     \"esbuild\": \"0.25.9\",",
              "-    \"playwright-core\": \"1.55.0\",",
              "+    \"playwright-core\": \"1.62.1\",",
              "     \"typescript\": \"5.9.2\"",
            ],
          },
        },
      ],
      residual: "intermittent: three consecutive zero-fallback journeys reduce, not eliminate, the recurrence risk. The permanent gate stays the detector.",
      quotes: [
        {
          text: "The `cf-present` qualification also exposed a browser-context close timeout while an earlier task-owned Chrome process was present.",
          source: record,
        },
        {
          text: "A normal qualification rejects any fallback use; the deterministic injected-timeout qualification requires exactly one.",
          source: record,
        },
        {
          text: "The full integration retry then correctly rejected an intermittent normal close fallback.",
          source: record,
        },
        {
          text: "The qualification had been driving Chrome 150 through Playwright 1.55, outside that Playwright release's tested stable-browser window",
          source: record,
        },
        {
          text: "`playwright-core` is now aligned to 1.62.1, the already-resolved docs-portal line tested against Chrome 151.",
          source: record,
        },
        {
          text: "three consecutive normal journeys completed with zero fallbacks, reducing rather than eliminating recurrence risk",
          source: record,
        },
      ],
    },
    {
      id: "F7",
      title: "the POSIX process inventory failed closed on a transient overrun",
      cells: [
        {
          round: "R8", states: ["opened", "closed"],
          what: "the POSIX inventory had half the Windows budget; both now share one constant",
          hunk: {
            path: harness, header: "@@ -772,7 +800,7 @@",
            lines: [
              "     const raw = execFileSync(\"/bin/ps\", [\"-axo\", \"pid=,ppid=,lstart=,command=\"], {",
              "       env: browserEnvironment,",
              "       encoding: \"utf8\",",
              "-      timeout: 5_000,",
              "+      timeout: PROCESS_INVENTORY_TIMEOUT_MS,",
              "     });",
            ],
          },
        },
      ],
      quotes: [
        {
          text: "The POSIX process-inventory timeout was aligned with the existing bounded Windows timeout after a transient five-second `ps` overrun failed closed during this verification.",
          source: record,
        },
      ],
    },
  ];

  // Context both candidates may show, quoted from the same record.
  const gate = {
    refused: {
      text: "The first task-to-integration gate then refused the merge.",
      source: record,
    },
    outstanding: {
      text: "The full integration gate remains to be rerun before this task lands.",
      source: record,
    },
  };

  // ------------------------------------------------------------- derivation
  const roundOrder = new Map(rounds.map((r, index) => [r.id, index]));
  const cellOf = (findingId, roundId) =>
    findings.find((f) => f.id === findingId)?.cells.find((c) => c.round === roundId) ?? null;
  const has = (finding, state) => finding.cells.some((c) => c.states.includes(state));
  const reopenings = (finding) => finding.cells.filter((c) => c.states.includes("reopened"));

  // A finding needed more than one round exactly when a later round had to
  // re-open it. An `extended` cell is not a re-opening and does not count.
  const multiRound = findings.filter((f) => reopenings(f).length > 0).map((f) => f.id);
  const external = findings
    .filter((f) => reopenings(f).some((c) => c.cause === "outside-harness"))
    .map((f) => f.id);
  const selfCaused = multiRound.filter((id) => !external.includes(id));
  const closedIn = (finding) =>
    finding.cells.filter((c) => c.states.includes("closed")).map((c) => c.round).at(-1) ?? null;
  const openedIn = (finding) =>
    finding.cells.find((c) => c.states.includes("opened"))?.round ?? null;
  const rounds_spanned = (finding) => {
    const first = roundOrder.get(openedIn(finding));
    const last = roundOrder.get(closedIn(finding));
    return first === undefined || last === undefined ? 1 : last - first + 1;
  };

  return {
    record, harness, cleanup, manifest, qualification,
    rounds, findings, gate,
    roundOrder, cellOf, has, reopenings, closedIn, openedIn, rounds_spanned,
    residuals: findings.filter((f) => has(f, "residual")).map((f) => f.id),
    extensions: findings.filter((f) => has(f, "extended")).map((f) => f.id),
    // Published to the DOM by both P3 candidates so the render harness can
    // capture it and verify.mjs can recompute it from this dataset.
    answer: { multiRound, external, selfCaused },
  };
})();
