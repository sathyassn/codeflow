# Shared token evidence

Open `ink-swatches.html` to compare every changed figure ink in light and dark.
`ink-changes.json` contains old and new values and worst-pair measurements,
including measurements for each changed ink. The task record explains the
floors and the operator-approved palette constraints.

`theme-fixtures-before.txt` records the seven planted theme tests failing
against the initial validator stub. `token-tests-after.txt` records the passing
source tests. The two mutation logs record tests failing when the literal or
stage guard is removed from a temporary copy; they establish guard sensitivity,
not a historical run against the integration branch.

Browser captures and the bar/ring render proof are primary-owned. Run
`node crates/codeflow-present/web/scripts/token-browser-check.mjs` from the
worktree root after building its binary and portal. Captures go to
`target/tsk-1162-browser/`. These captures have not been produced by the builder.

`stylesheet-lint-before-after.txt` records a new consumer stylesheet accepted
before the file scan and rejected after it. The automated control and planted
fixture now run in `web/scripts/token-lint.test.mjs` as part of the web check.

`literal-fallback-before.txt` records the named-colour fallback fixture failing
before the guard was corrected. The passing source suite includes that case.

Fable repair evidence: `fable-tokens-before.txt` and `fable-tokens-after.txt`
show the export-palette and value-named-colour failures and passing source
suite. `portal-system-before.txt` and `portal-system-after.txt` show the
built-page script-order failure and the passing portal suite. This executes
inline scripts in a VM, not a browser. The primary browser script additionally
checks legacy system/auto mode and actual system exports with JavaScript both
enabled and disabled, including Slate light/dark captures.
