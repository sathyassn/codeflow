# TSK-035 local guide verification

## Status and scope

Task `TSK-035` (epic `EPC-013`) on branch `task/TSK-035-local-guide`, based on
`7ed443e6111b929286bc3515d4afb4f3b2bca60d` (the merged TSK-034 patch).
`codeflow work start TSK-035` anchored the task there with dependency TSK-034
complete. Native Claude Fable 5.1 (high) authored the unit directly; no
subagent, orchestrator, or delegate ran. Source commits:

| Commit | Content |
|---|---|
| `2010c6c8cdc0f3cc2bf4e6ccbe0d8a4dc8e43241` | README repository-guide entry, adoption local workflow, capability Node roles, changelog note, `release_version: null` and reader-specific description |
| `2b4eab3a47aea97566602b888c53496866d60e55` | description reworded without a colon so the `llms.txt` twin does not escape it |
| closing commit (adds this file) | shorter code comments in README and adoption so the command block fits the content column; no command changed |

The exact locked sequence below was rehearsed on both source commits from a
clean committed checkout with nothing under `docs-portal/` besides the
committed runtime and ignored output. Browser evidence was recorded on
`2b4eab3a47aea97566602b888c53496866d60e55`; the closing commit changes only
comment text in two Markdown files and adds this record, and the host is
expected to rerun the sequence on the final head.

## Environment

| Item | Value |
|---|---|
| Host | macOS, Darwin 25.6.0, arm64 |
| Node | `v24.18.0` from `~/.asdf/installs/nodejs/24.18.0`, matching `docs-portal/.node-version`; ambient 23.10.0 was not used |
| npm | `11.16.0` |
| codeflow | `codeflow 3.0.0` at `~/.asdf/installs/rust/1.94.0/bin/codeflow`; `--version` prints no source commit, so the exact build commit of that binary is not verified here |
| Playwright | `@playwright/test 1.62.1` from the locked install; cached Chromium, Firefox, and WebKit builds |

The session ran under a macOS Seatbelt sandbox. Two steps needed an
unsandboxed rerun after a sandbox-attributable failure; each is named below.

## Commands and results

| Step | Command (from `docs-portal/` unless noted) | Result |
|---|---|---|
| Anchor | `codeflow work start TSK-035` (repository root) | anchored at `7ed443e6…` for `task/TSK-035-local-guide` to `integration/EPC-013-release-guides`; dependencies complete |
| Locked install | `npm run deps:install` | 452 packages added, dependency lifecycle scripts disabled; one npm `TAR_ENTRY_ERROR EPERM` on `node_modules/stream-replace-string/.vscode/settings.json` (an editor settings file the sandbox refused to write; not a runtime file) |
| Audit | `npm audit` and `npm audit --omit=dev` | 0 vulnerabilities in both |
| Check | `npm run check` | first sandboxed attempt failed in the adapter fixture: `git init` could not copy hook templates from the Homebrew git share directory (`Operation not permitted`); unsandboxed rerun passed on both commits: adapter tests 95 run / 95 pass / 0 fail, Astro check 0 errors and 0 warnings over 20 files, exit 0 |
| Build | `npm run build` | 141 pages built, 463 built artifacts recorded, exit 0, on both commits |
| Validator | `codeflow validate --portal docs-portal` (repository root) | `135 page(s) clean` on both commits; `.codeflow/policy.json clean` |
| Browser matrix | `PORTAL_BROWSER_RUN=tsk035-fable npm run browser:verify` | sandboxed attempt failed before any check: Chromium died at Mach port rendezvous (`Permission denied`) and WebKit timed out; unsandboxed rerun passed: keyboard tests 10 / 10, then Chromium, Firefox, and WebKit each passed 19 checks (landmarks and names, axe WCAG 2.2 AA, screen-reader structure, layout, search slash hit and follow, system and mode persistence before paint, display settings persist, layer journey, deep link, altitude tabs hide inactive (2), utility chrome (9), strict-ID hover, keyboard, touch, Escape and navigation, source link, keyboard traversal and focus, skins light and dark, target size, responsive, console, network isolation) |

Browser run identity: repository commit `2b4eab3a47aea97566602b888c53496866d60e55`,
config SHA-256 `0250443cff620eb1774fd770b529fe1285fbffce5e314c5a4a5084e98e89a059`,
evidence SHA-256 `1344dcdbe2fcf3ebb42b83eac14dbed1ae307bd4a74451000dea98c94c02f390`,
generator `@codeflow/docs-portal 2.0.0`, task-owned loopback port 49675,
headless, `teardown_verified: true`. Artifacts (21 hashed screenshots across
three engines, seven appearance and viewport variants each, three traces, and
`results.json`) are under `docs-portal/.portal/browser-evidence/tsk035-fable/`,
which Git ignores; they were retained locally for host review.

## Rendered facts

- Home page footer line: `Repository 2b4eab3a47aea97566602b888c53496866d60e55 · portal 2.0.0`, with no release label.
- Every page provenance comment carries `built_from_commit=2b4eab3a4…` and `release_version=none`; the evidence manifest records `release_version: null`.
- `public/llms.txt` opens with the title, the two-sentence description, and `Repository commit: 2b4eab3a4…`; no `Release version:` line.
- Layer composition: Orient 3 sources (product, capabilities, adoption); System 64 (architecture, present, 62 ADRs); Records 63; Reference 5 (v2 charter, execution status, rework-loop probe note, release checklist, releasing).
- `/orient/adoption/#reading-the-codeflow-guide-locally` resolves to the new section, so the README link target exists in the derived guide as well as on GitHub.

## Rendered judgment

A task-owned static loopback server over `dist/` plus headless Chromium
captured 14 routes at 1280x860 and 390x844 in light and dark with reduced
motion: home, the architecture page at `#concept`, `#architecture`, and
`#technical`, the present architecture page, the adoption section, product,
ADR-0048, TSK-035, and the System layer index. All returned HTTP 200, the hash
selected the named altitude tab with exactly one visible panel, no route had
horizontal page overflow, no console errors were logged, and the server closed
with its port released. Those screenshots live in the session scratchpad, not
the repository.

- Home reads as documentation: title, two-sentence purpose, and the four-layer journey with source counts; the version line names the commit only.
- Architecture, Concept panel: one governing claim in the display role, one ASCII stage as the primary carrier, one supporting sentence. Passes the five-second test.
- Architecture, Architecture panel: the crate figure and then the full-width `cf-stage` with six labeled nodes, named edges, and a caption; structure survives sentence removal. At 390px the stage stacks vertically with arrows and no overflow.
- Product: Purpose leads with the governing claim; source pin and built-from commit sit above it. No rewrite is warranted.
- Non-blocking observation, no source change made: the two ASCII `text` fences in `docs/architecture.md` are wider than the content column at 1280px and scroll inside their own code box, clipping the right-hand column until scrolled. The `cf-stage` figure does not have this issue. Narrowing those fences is an architecture-doc authoring choice for its ship flow, not this task.
- Non-blocking observation for host judgment: `docs/plan/v2` pages surface under Reference as the historical plan of record. They are canonical repository Markdown, so no exclusion was added here.

No material composition or altitude failure was found, so no canonical source
authoring adjustment was made beyond the sources this task owns.

## Teardown

No `.portal/workflow.lock*` or `.portal/publish.lock` remained after any run;
no Node process was listening on a TCP port after the browser matrix or the
screenshot server. The browser harness reported its own profile and server
teardown. `docs-portal/node_modules`, `dist/`, `.portal/generated/`, and the
browser evidence remain as ignored local output for host review.

## Follow-up after the first review

Commit `6387ccfd6ba8be21a166995e6e1f3fba5cc4573f` renames v2.1.0 the latest
verified published release in the copy this task introduced (publication is
verified; anonymous repository visibility is not), adds the pre-public
private-vulnerability-reporting gate to the release checklist, and extends the
changelog note by one clause. The GitHub setting was neither changed nor
checked here. The closing follow-up commit adds this section only.

Authoritative validation used the immutable candidate binary
`research-evidence/release-readiness-2026-09-19/bin/codeflow-tsk034-2a785423`
(`codeflow 3.0.0`), with its `codeflow-cli-target/release` directory prepended
process-locally so the commit hooks resolved the same build. Results on
`6387ccfd6`: `npm run build` 141 pages, 463 artifacts; candidate
`validate --portal docs-portal` reported `135 page(s) clean`; the home page
line read `Repository 6387ccfd6… · portal 2.0.0`. The host reruns both on the
final head, whose only change is this section.

`npm run preview` was rehearsed on loopback port 4321 (free beforehand) from
that build: `/` answered 200 with the commit line above, `/system/architecture/`
answered 200 with one altitude tablist, and the twin
`/markdown/system/architecture.md` answered 200. Nine screenshots were taken
against that live preview with a task-owned headless Chromium and no operator
profile, then the workflow process was sent SIGTERM. Afterwards: zero listeners
on 4321, zero preview processes, no lease files. Evidence, outside the
repository:
`research-evidence/release-readiness-2026-09-19/native/fable-tsk035-author/preview-evidence/`
(`preview.log`, `screenshots/`, `shots.json`). Visual judgment of the
preview-captured architecture panel in light and dark and the narrow dark home
matches the earlier judgment: display-role claim, crate figure, full-width
labeled stage with named edges and caption, one visible panel, no overflow.

## Not verified here

- The aggregate CI gate on Node 26.4.0 and the Windows adapter lane on 24.18.0 were documented from `.github/workflows/codeflow-ci.yml`, not executed.
- The full `codeflow test --mode full --strict` suite was not duplicated in this session.
- Independent Codex and Grok review, hosted, billing, native-platform, publication, installer, and release qualification remain with their existing owners.
- The exact source commit of the installed `codeflow` binary is not printed by `--version` and was not confirmed.
