# cf-present qualification — review convergence

Ordinary Markdown treatment of the same content. `baseline.html` is a faithful
hand-authored HTML rendering of this file.

## Rounds

| Round | Commit | Subject |
|---|---|---|
| R1 | `86582dc0` | qualify owned runtime boundaries |
| R2 | `74feaf04` | harden qualification cleanup |
| R3 | `82671551` | close qualification review gaps |
| R4 | `14107e48` | tolerate cold browser startup |
| R5 | `c5ede869` | follow bootstrap readiness semantics |
| R6 | `30133fd7` | bound cold renderer startup |
| R7 | `e194b1fd` | harden browser close recovery |
| R8 | `a47b62c3` | align browser qualification driver |

## Findings

| Finding | Raised | Closed | Re-opened by |
|---|---|---|---|
| F1 child environment inherited host values | R2 | R3 | R2's own allow-list — `SystemRoot` casing missing |
| F2 cleanup swallowed the primary error | R2 | R2 | — |
| F3 injected-failure cleanup unproved | R2 | R2 | — (extended R7, R8) |
| F4 Windows profile confinement unscoped | R3 | R3 | — |
| F5 cold-runner startup exceeded bounded waits | R4 | R6 | R4's timeout, then R5's refactor |
| F6 browser-context close timed out | R7 | R8 | Chrome 150 on Playwright 1.55 |
| F7 POSIX process inventory failed closed | R8 | R8 | — |

## What each round changed

- **R2** — `allowedEnvironment` allow-list plus
  `assertChildEnvironmentDoesNotInheritUnrelatedHostValues`; `primaryError` and an
  `attempt` accumulator; a `tracingStarted` latch; `--inject-cleanup-failure` and
  `real-browser-cleanup-check.mjs`.
- **R3** — `SystemRoot` added to the allow-list; `USERPROFILE`, `LOCALAPPDATA`,
  `APPDATA`, `TMP`, `TEMP` redirected to task roots; `qualifyWindowsEnvironment`
  and `windows-qualification-scope.mjs`.
- **R4** — `NAVIGATION_TIMEOUT_MS = 45_000`, replacing
  `setDefaultNavigationTimeout(20_000)`.
- **R5** — `openAuthenticatedPresentation`: commit the bootstrap navigation, then
  wait for the destination URL at `domcontentloaded`.
- **R6** — `BOOTSTRAP_COMMIT_TIMEOUT_MS = 120_000` and
  `timeoutWithinQualification`, capping every wait by the remaining 180-second
  journey deadline.
- **R7** — `closeBrowserContext`, `BoundedTimeoutError`,
  `terminateOwnedProcessIdentities` (TERM → 2.5 s → KILL, fingerprint re-verified
  before signalling), `browser_close_fallbacks`; a normal run rejects any
  fallback, the injected run requires exactly one.
- **R8** — `playwright-core` 1.55.0 → 1.62.1; `toolchain.browser_version` and
  `toolchain.playwright_core_version` recorded and asserted;
  `PROCESS_INVENTORY_TIMEOUT_MS = 10_000` for both platforms.

## Residuals

- The Windows guard proves start-time confinement only, not account, profile or
  VM teardown.
- The close fallback is intermittent: three consecutive zero-fallback journeys
  reduce rather than eliminate the recurrence risk. The permanent zero-fallback
  gate remains the detector.
