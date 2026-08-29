# P3 ordinary-chat baseline

What a competent assistant types when asked how the `cf-present` qualification
review converged. Authored before either candidate.

---

The qualification harness went through eight rounds on this branch. `86582dc0`
created it, `74feaf04` hardened cleanup, and `82671551` closed the review gaps —
that last one is the exact candidate TSK-007 was qualified at.

`74feaf04` stopped the qualification child inheriting the whole host environment,
replacing the `...process.env` spread with an explicit `allowedEnvironment`
allow-list plus an assertion that nothing unrelated leaks in. It also stopped
cleanup swallowing the primary error: failures now accumulate through an
`attempt` helper, and a `tracingStarted` latch keeps it from stopping a trace
that never started. And it added `--inject-cleanup-failure` with a separate
`real-browser-cleanup-check.mjs`, so the injected-failure path is proved rather
than assumed. `82671551` then had to add `SystemRoot` to that same allow-list —
the first version had only `SYSTEMROOT`, which is not the casing Windows
actually uses — and redirected `USERPROFILE`, `LOCALAPPDATA`, `APPDATA`, `TMP`
and `TEMP` into task-owned roots. It also added the Windows profile-confinement
scope check, which proves start-time confinement only.

Then the cold-startup sequence. `14107e48` raised the navigation timeout from
20 s to 45 s. `c5ede869` decided the timeout was the wrong fix and replaced the
readiness semantics: the bootstrap file immediately submits into the loopback
service, so waiting for its full load event conflates the handoff with the
destination page. It introduced `openAuthenticatedPresentation`, which commits
the bootstrap navigation and then waits for the destination URL at
`domcontentloaded`. `30133fd7` then had to bound that: the refactor left the
commit wait on a default with no relation to the journey deadline, so it added
`BOOTSTRAP_COMMIT_TIMEOUT_MS` and a `timeoutWithinQualification` helper that
caps every wait by the time left in the 180-second journey.

`e194b1fd` handled the browser-context close timeout that appeared when an
earlier task-owned Chrome process was present. It added a bounded close with a
`BoundedTimeoutError`, an exact-owned TERM → 2.5 s grace → KILL fallback that
re-verifies each process fingerprint immediately before signalling, a
`browser_close_fallbacks` counter, a normal run that rejects any fallback at all,
and an injected `--inject-close-timeout` run that requires exactly one.

That was not the end of it. The full integration retry rejected an intermittent
normal close fallback anyway. The cause turned out to be outside the harness: it
had been driving Chrome 150 through Playwright 1.55, outside that release's
tested stable-browser window. `a47b62c3` aligned `playwright-core` to 1.62.1,
started recording the observed browser and Playwright versions in the results so
a recurrence is attributable, and made the cleanup check assert that toolchain.
It also raised the POSIX process-inventory timeout from 5 s to a shared 10 s
constant after a transient `ps` overrun failed closed during that verification.

Two things stay open. The Windows guard proves start-time confinement only, not
account, profile or VM teardown. And the close fallback is intermittent: three
consecutive normal journeys completed with zero fallbacks, which reduces rather
than eliminates the recurrence risk.
