---
name: cf-consult
description: Get an independent second opinion from another vendor's CLI (read-only), then synthesize it against your own analysis. Use when you want an outside pass on a file, diff, design, or question, including dual-lineage review. Do not use for edit handoffs (cf-delegate) or same-vendor self-review.
---

# cf-consult — an independent second opinion

You are getting a second opinion, not handing off work. Load the `cf-delegate`
skill for the consult doctrine and lane details. Consult is read-only: the
delegate reads and reasons, never edits. Use `cf-delegate`'s preferred lanes or
qualified native fallback—never headless (`codex exec`, `claude -p`).

The peer must be another vendor. Same-vendor scrutiny is useful, but it never
counts as independent cross-lineage review.

1. Frame the ask: state exactly what to review (paths, diff, or question) and
   the criteria to judge against, and ask for an explicit closing verdict line
   (`VERDICT: approved|changes_requested`) so the reply is checkable, and for
   each blocker and major finding the smallest evidenced remedy and its
   verification criterion, or the options when the fix is an operator decision.
   Do your own analysis first; the consult sharpens it. Start the delegated
   prompt with `ROLE: peer`, bound it to this consult, and prohibit starting
   the top-level orchestrator or delegating back to the host lineage.
2. Pick the TTY host, then the lane. When `HERDR_ENV=1`, load `cf-herdr` and
   host the other seat in a new or resumed named tab; do not hijack the caller
   pane. Consult launch is read-only (Claude auto; never bypass). Production
   host flags live in `cf-herdr`. Do not require tmux. Herdr
   `idle`/`done` is not consult completion. Outside Herdr,
   use the lanes below (tmux for Claude is the degraded TTY).
   Pick the lane by your seat:
   - **From Claude Code → codex**, through the official `codex-plugin-cc`
     plugin: `/codex:review` is the diff/design read (read-only;
     `/codex:adversarial-review` for the security lens). Preflight: the
     `/codex:*` commands exist and `codex login status` exits 0. If incompatible,
     use `cf-delegate`'s qualified native fallback with the same review scope.
   - **From codex → claude**, through CodeFlow's schema-v2 delegate lifecycle
     (`delegate init` → wait-ready → `arm` → canonical UTF-8/internal-LF
     exact-byte delivery → wait-accepted → wait-terminal, with bounded cleanup;
     CodeFlow ADR-0036; `cf-delegate` carries the full contract). When
     `HERDR_ENV=1`, start Claude in the named Herdr tab and deliver with
     `herdr pane send-text` then Enter. Outside Herdr, the degraded host is a
     dedicated worktree-scoped tmux session. Then:
     Run the sibling Stop-hook preflight from `cf-delegate` before delivery.
     Consume `last_assistant_message` from the terminal result record and
     branch on the stable exit states — never on pane appearance. Use
     `capture-pane` only for the dedicated tmux task pane (on Herdr, `herdr
     pane read` of the owned pane only) for bounded diagnosis or an explicit
     dialog—never as a stability heuristic and never against unrelated
     sessions. Launch the
     consult with the Claude primary selector and default effort from
     `../cf-model-orchestrator/resources/current-ensemble.json`. On the
     degraded tmux path, use
     `claude --model <selector> --effort <effort> --permission-mode auto
     --settings <state-dir>/settings.json`. On the Herdr path, native flags
     come from `cf-herdr`, plus `--settings` for the lifecycle hooks;
     when `.codeflow/model-selection.json` is nonempty, first require
     `codeflow doctor --check model-bindings` to pass and use only its effective
     qualified override for this harness;
     the generated settings file carries only the lifecycle hooks and is
     immutable — make `autoMode.classifyAllShell` effective at user scope.
     Keep "read and
     reason only; edit nothing" in the prompt. On the degraded tmux path,
     require the effective project settings to enable the OS sandbox with
     `sandbox.failIfUnavailable: true` and permit an auto-classified
     unsandboxed retry only for a trusted installed tool that requires host
     state. When `HERDR_ENV=1`, native flags come from `cf-herdr` (Auto by
     default; unattended overlay only if the operator asked). That is not a
     write grant. Verify the worktree diff. Preflight:
     `claude` is present; `herdr` when `HERDR_ENV=1`, else `tmux`; `claude mcp
     list` succeeds; a scoped interactive canary returns an authenticated
     response. The Claude judgment primary owns routing and its judgment
     under `../cf-model-orchestrator/resources/capability-routing.md`; never
     invoke a foreign worker as its reasoning seat. See
     <https://code.claude.com/docs/en/permission-modes>.
3. A missing or incompatible preferred plugin/CLI is a lane failure, not yet a
   vendor-seat failure. Exhaust the qualified native alternatives permitted by
   `cf-delegate`, preserving the same read-only scope, provenance, sandbox, and
   lifecycle checks. An authentication failure still stops for operator action:
   state the exact remedy (`codex login`; plugin installation from Claude Code;
   install/login for `claude`, plus `herdr` or degraded `tmux`) and never
   automate login. After every qualified other-vendor route is unavailable,
   report the unavailable seat and reduced assurance. Never substitute the
   host's own vendor or label same-family scrutiny as cross-lineage review.
4. Synthesize: compare the second opinion against your own analysis point by
   point, citing where you **agree** and **disagree** and why — with your own
   evidence (file:line, command output). Write the synthesis plainly:
   simple, straightforward and clear, no mannered prose (see
   `.codeflow/rules/writing.md`). Label each finding `axis: standards`
   or `axis: spec`; when both apply, label both so one cannot mask the other.
   For task acceptance, ask `cf-reviewer`'s two questions per criterion and
   refuse a block copied from an older commit. Disposition
   stays `fix now`, `track once`, or `drop` (the same vocabulary as the
   quality contract and `cf-reviewer`). Note which seat raised each item.
   Branch on the verdict line, then re-derive the findings. Meet the `cf-delegate` five-obligation evidence
   contract (launch/provenance/return/failure/recheck): a Codex reply counts
   only with its native thread ID, and a Claude reply only with its lifecycle
   records. Never paste the delegate's reply as your finding; an unverified
   claim is a defect.
5. Offer a follow-up in the same session — the plugin thread is resumable, and
   the lifecycle session keeps its context: arm the next turn and deliver to
   the same pane.
