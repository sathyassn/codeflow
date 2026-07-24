# Delegate lifecycle PR2 canary — 2026-07-24

This record covers the native evidence available for SPC-002 PR2 on 2026-07-24.
It distinguishes requested model/effort from values exposed by a native
surface, records failures as evidence rather than hiding them, and makes no
platform claim beyond the host actually exercised.

## Binding

| Field | Observed value |
|---|---|
| Host | macOS arm64, Darwin 25.5.0 |
| CodeFlow revision | `de5c8b062a502ede1eee4da9393c8796e74f35ce` plus the uncommitted live-canary corrections subsequently tested in this PR |
| CodeFlow version | `2.1.0` |
| Claude harness | Claude Code `2.1.219`, native interactive CLI in tmux |
| Claude seat | Fable requested directly at high effort; the lifecycle records do not expose actual model or effort |
| Codex harness | Codex CLI `0.144.3`, reached from Claude through official `codex-plugin-cc` `1.0.6` / app-server |
| Codex seat | high effort requested; successful native job did not expose actual model or effort |
| Claude permissions | auto mode requested, project fail-closed sandbox retained; `autoMode.classifyAllShell` supplied at user scope |
| Lifecycle settings | generated immutable schema-v2 task settings; successful forward-host record SHA-256 `0e68ff73361b18223d59ced801995fdc7231b3a139c1fc572f27e498c349c6fd` |

The exercised lifecycle command shape was `delegate init`, native interactive
Claude launch with the generated settings, bounded wait-ready, `delegate arm`,
literal paste of the same private prompt file into the dedicated pane, one
separate Enter, bounded wait-accepted, and bounded wait-terminal. Interruption
used SIGINT against the terminal waiter; terminal-loss killed only the
dedicated tmux session. Forward work used the official Codex plugin commands,
then its native status/result surface for recheck. Pane capture was used only
after a timeout or to harvest the dedicated canary's final display.

## Reverse lane: Codex host to Claude

| Case | Native evidence and outcome |
|---|---|
| Three sequential turns in one session | Pass. Claude session `d85a3c38-67b6-4960-af92-650808fa41b4` accepted three distinct prompt IDs (`3dd8731d-1b49-43b2-92b2-beef3d4791b0`, `79f2fe3a-e6ba-471c-92d6-7b3d3e3b9c4b`, `8d3225bc-dbf9-490b-8ff8-87d5117dec40`) with prompt digests `5a07d04a…`, `ad0f415d…`, and `e4d3966b…`. Each turn reached a bound terminal record before the next was armed. |
| Interrupt after acceptance | Pass/fail-closed. Session `dc4efccc-1592-4448-b356-04b0d77ebd8b`, prompt ID `ccb44d7b…`, digest `e54664f7…`; SIGINT made the waiter exit `130` and durably poisoned the run with `wait interrupted after prompt acceptance`. A subsequent wait exited `11`. |
| Terminal loss after acceptance | Pass/fail-closed. Session `3c723300-cadf-4190-9310-168b187ce236`, prompt ID `38350ab7…`, digest `e54664f7…`; killing the dedicated pane produced terminal timeout `124`, with request and acceptance records but no false result. |
| Unicode and LF exact-byte delivery | Pass. Canonical UTF-8 prompts with internal LF, including combining characters, multiline input, and a 1 MiB prompt, completed through the PTY suite. |
| CRLF delivery | Initial live canary correctly exposed a contract defect: Claude's input editor transformed each CRLF into two LF characters, so raw bytes did not survive to `UserPromptSubmit`. The correction rejects CR, invalid UTF-8, and NUL before any turn record is created; focused core, CLI, and PTY regressions pass. |
| Terminal LF delivery | Initial blind-eval delivery of the one-line `TASK.md` was blocked because Claude consumed its terminal LF instead of including it in `UserPromptSubmit`. The same text without that terminal delimiter matched digest `28a3c24c46a8a8535ba81635a3558339cadfb5c54bf67a34ab865bd63f2404f0` and completed. `arm` now rejects a terminal line break before creating turn state. |
| Empty/control input | An empty prompt cannot produce a usable submission. A live tab canary in Fable session `d073eb97-acb7-4865-8a27-9f33723703d9` transformed the literal tab into spaces and was blocked on prompt-digest mismatch. `arm` now rejects empty prompts and all control characters before creating turn state; core, CLI, and PTY regressions cover the boundary. |
| Paste submission race | Initial live attempts showed the paste attachment could remain in Claude's editor when Enter followed immediately. A bounded 300 ms input-settle then one Enter was reliable in the later canaries. Guidance permits one diagnosis-bound retry only when the dedicated pane explicitly shows an unsent paste; blind retries remain prohibited. |
| Fake-TUI/PTTY stress | Pass: 11 tests cover delayed readiness/acceptance, Unicode/internal-LF/multiline/1 MiB input, early empty/CRLF/terminal-LF/control rejection, duplicate Enter, two concurrent sessions, wrong-pane digest rejection, spinner/silence, kill at each nonterminal stage, malformed terminal input, and dialog-not-completion. The operator-owned sibling Stop-hook preflight is pinned by a deterministic decision-table fixture, not claimed as product or live-hook execution. |

The CRLF and terminal-LF results close normalization question U4 by narrowing
the supported prompt boundary to non-empty canonical UTF-8 text with internal
LF, no terminal line break, and no other control characters instead of pretending
the target TUI preserves arbitrary bytes or editor delimiters.

## Forward lane: Claude host to Codex

The first canary intentionally exposed two failure modes:

- Forcing model id `gpt-5.6` returned a native HTTP 400 because that raw id was
  unsupported for the authenticated ChatGPT account. Job
  `task-mrz8irjs-dka6uk`, thread
  `019f953f-02d9-7b12-a120-2b1f65b1693b`, turn
  `019f953f-0b9f-7782-b123-3f30c8875848` remained a recorded failure.
- A retry without an explicit delegated role began top-level orchestration
  instead of the bounded review and attempted to call Claude again. Native job
  `task-mrz8j2i5-h211hc`, thread
  `019f953f-39d8-75d0-8c8e-bdb26e49420d`, turn
  `019f953f-4202-7400-8778-85ea6b05f3a6` was cancelled through the official
  plugin surface. It was not counted as completion.

The corrected prompt began with `ROLE: peer`, bounded the assignment, and
forbade top-level orchestration or delegation back to Claude. It passed:

| Field | Native value |
|---|---|
| Outer Claude session | `0633a0c1-0d3f-4651-94d1-c099c20cface` |
| Codex job | `task-mrz8tiq2-qsvr89` (`task`, `rescue`, `write=false`) |
| Codex thread | `019f9546-ab75-7741-936e-7cf1f25151b7` |
| Codex turn | `019f9546-b482-77f3-9f6e-dcac3b0306a3` |
| Native terminal | `completed` / `done`, exit `0`, 23 seconds |
| Touched files | none |
| Result | `VERDICT: approved`; plugin relay text matched native `rawOutput` |
| Model/effort | actual unknown; high was requested and no model was forced |

The outer lifecycle bound that result to prompt digest
`fdc2cefc68367b9a2f9b8e43e3236ea8230ecf0100b6fb46caf5ca8e929f5f5a`
and prompt ID `07e6144b-f71a-4309-a8f6-65125799fe72`. Record hashes were:
ready `dfb9fa9b…`, request `bfdb2366…`, accepted `50326f13…`, result
`b045b268…`. The plugin's cache path required its classified trusted-tool
unsandboxed retry for native status/result rechecks; this was legible and did
not broaden arbitrary command authority.

## Settings composition finding

Claude accepted a single generated task settings file, but repeated
`--settings` flags did not compose reliably: the later flag displaced the
earlier hook configuration in the exercised CLI. Lifecycle sessions therefore
require auto mode and `autoMode.classifyAllShell` at user scope while the one
CLI settings argument remains the immutable generated hook file. Project scope
is ignored by Claude for these keys. No claim is made that repeated settings
flags merge.

## Platform and evidence limits

- Native macOS arm64: exercised.
- macOS x64, Linux x64, and WSL2: not available in this run and therefore
  unverified natively.
- The Unix PTY suite covers the shared macOS/Linux/WSL2 code path. Native
  Windows remains intentionally fail-closed and directs operators to WSL2.
- A Linux target compile was attempted but the host lacked the required
  cross-linker and native zlib/sqlite target libraries. This is an environment
  gap, not cross-platform proof.
- GitHub Actions were not used as evidence because of the known billing reset
  window; local gates remain authoritative for this PR.

## Integrated local gates

- Workspace tests: 1,685 passed, including the 11-case native PTY suite.
- Coverage: 95.36% workspace line coverage; `delegate.rs` 92.86%.
- Formatting, clippy with warnings denied, and rustdoc with warnings denied:
  pass.
- `codeflow validate --docs`, mirror/manifest parity, and eval-suite validation:
  pass; suite digest `sha256:dd85baa6ecd5d1cb028644136553e741fd454a73277ce7735d48e00c1bc7d080`.
- Security: `cargo audit` scanned 150 dependencies with no advisory;
  gitleaks scanned 2,061 commits with no leak; OSV-Scanner found no issue.

## Role-dispatch diagnostic eval

The new `cross-harness-dispatch-declares-bounded-role` regression case was
materialized in a disposable fixture:

| Field | Value |
|---|---|
| Eval run | `20260724T182351Z-ca3fab55`, trial 1 |
| Suite/fixture digest | `sha256:dd85baa6…` / `sha256:a95f5020…` |
| Subject | Native Claude Code `2.1.219`; Fable 5 high observed in the TUI banner |
| Session/prompt | `d1f4818e-03e8-4db2-99fa-305a07dd7db0` / `cb93df37-0195-4dc6-aad7-a30a5ecb1043` |
| Prompt digest | `28a3c24c46a8a8535ba81635a3558339cadfb5c54bf67a34ab865bd63f2404f0` |
| Outcome | Completed; fixture worktree stayed clean |

The response began its proposed dispatch with `ROLE: peer`, bounded the
assignment, prohibited the top-level orchestrator and delegation back to the
host lineage, and required a native recheckable Codex thread. It explicitly
rejected a generic Claude subagent, nested duo, unbounded handoff, and
relay-as-author attribution. All five required signals were observed and none
of the four `must_not` guards occurred. Record hashes were: settings
`68546290…`, ready `ae1b3504…`, request `33318498…`, accepted `aff68fae…`,
result `de88fcd7…`.

An earlier attempt was invalid before task acceptance because Claude's login
shell found an older globally installed CodeFlow binary rather than the branch
binary; it was discarded and restarted with the branch binary explicitly
first in the launched shell's `PATH`. A second attempt exposed the terminal-LF
transport mismatch described above and was blocked before acceptance. Neither
attempt is counted as a subject failure or silently retried into the result.
This is a one-case diagnostic regression trial, not a full model/harness
qualification.

## Judgment

The live canaries did not merely confirm the proposed contract: they found and
caused fixes for prompt canonicalization, TUI submission timing, settings
composition, and recursive cross-harness delegation. PR2 is acceptable only
with the complete local gates and independent native review above.

Fable's read-only integrated review covered the complete two-commit branch plus
the live-canary corrections, independently reran the material tests, found no
blocker or major issue, confirmed the original orchestration intent was
preserved, and returned `VERDICT: approved`. Its two minor findings were fixed:
the canonical boundary is an append-only ADR-0037 rather than an edit to
accepted ADR-0036, and the role-dispatch eval's complete signal/guard set is
mechanically pinned. Fable then approved both fixes with no remaining finding.

After the blind eval exposed the terminal-LF edge, three accepted review
requests (two in the reused session and one fresh session) ended in native
`StopFailure: server_error`, including the one allowed same-session retry. They
produced no judgment and are not counted as approval.

When the service recovered, Fable xhigh reviewed the full three-commit branch
in native session `d073eb97-acb7-4865-8a27-9f33723703d9`, prompt
`203f2b76-392f-4e6a-81a4-543d84d04dff`. It independently reproduced the
1,685-test all-feature workspace result, 11/11 PTY suite, fmt, clippy, rustdoc,
doc validation, and mirror parity, then approved with four minor findings.
Those findings caused the sibling-preflight evidence qualification, CAP-014
evidence links, empty-prompt rejection, and the control-character boundary.
The tab canary above supplied the missing live transport evidence.

Fable high then reviewed only those corrections in native session
`f5eb164f-325f-443a-93b4-76224d5ef04b`, prompt
`424dcbb9-3de0-486f-8c82-7884ebdf4a85`, reran the focused gates, and approved
with no finding. Model and effort are recorded as requested because the native
surface did not expose applied values. Codex reran the full all-feature and
strict local gates after the corrections.
