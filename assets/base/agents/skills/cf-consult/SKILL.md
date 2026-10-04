---
name: cf-consult
description: Get an independent second opinion from another vendor's CLI (read-only), then synthesize it against your own analysis. Use when you want an outside pass on a file, diff, design, or question, including dual-lineage review. Do not use for edit handoffs (cf-delegate) or same-vendor self-review.
---

# cf-consult: an independent second opinion

You are getting a second opinion, not handing off work. Load the `cf-delegate`
skill for the consult doctrine and lane details. Consult is read-only: the
delegate reads and reasons, never edits. Use the seat that cross-family
transport names (`cf-model-orchestrator/resources/routing/transport.md`) or
its recorded native fallback through `cf-delegate`, never headless
(`codex exec`, `claude -p`).

The peer must be another vendor. Same-vendor scrutiny is useful, but it never
counts as independent cross-lineage review.

1. Frame the ask: state exactly what to review (paths, diff, or question) and
   the criteria to judge against, and ask for an explicit closing verdict line
   (`VERDICT: approved|changes_requested`) so the reply is checkable, and for
   each blocker and major finding the smallest evidenced remedy and its
   verification criterion, or the options when the fix is an operator decision.
   For a unit of work, the brief asks for one holistic pass under the review
   brief contract in the orchestrator's `resources/quality/findings.md`: the
   whole unit at one head, its full diff against its base and its blast
   radius, with earlier findings as checks within that pass, never its whole
   scope. Do your own analysis first; the consult sharpens it. Start the
   delegated prompt with `ROLE: peer`, bound it to this consult, and prohibit
   starting
   the top-level orchestrator or delegating back to the host lineage.
2. Launch through `cf-delegate`: it owns the lanes, the launch and delivery
   sequence, the preflight and the evidence contract. From any host with a
   reachable Herdr server, `cf-herdr` hosts the seat in a named tab and never
   takes over the caller pane. A consult launches in the transport table's
   consult and review posture, keeps "read and reason only; edit nothing" in
   the prompt, and ends with a verified worktree diff.
3. A missing or incompatible preferred CLI, Herdr server or plugin is a lane
   failure, not yet a vendor-seat failure. Exhaust the qualified native
   alternatives in the transport order, preserving the same read-only scope,
   provenance, sandbox, and lifecycle checks. An authentication failure still
   stops for operator action: state the exact remedy (`codex login`;
   install/login for `claude` or `grok`; `herdr`, or `tmux` as the last
   fallback) and never automate login. After every qualified other-vendor route is unavailable,
   report the unavailable seat and reduced assurance. Never substitute the
   host's own vendor or label same-family scrutiny as cross-lineage review.
4. Synthesize once, in the canonical review: the verdict, the material
   differences between the second opinion and your own analysis, the
   verified findings and their dispositions; group the agreements. Support
   each point with your own evidence (file:line, command output).
   Write the synthesis plainly: simple, straightforward and clear, no
   mannered prose (see `.codeflow/rules/writing.md`). Label each finding `axis: standards`
   or `axis: spec`; when both apply, label both so one cannot mask the other.
   For task acceptance, ask `cf-reviewer`'s two questions per criterion and
   refuse a block copied from an older commit. Disposition
   stays `fix now`, `track once`, or `drop` (the same vocabulary as the
   quality contract and `cf-reviewer`). Note which seat raised each item.
   Branch on the verdict line, then re-derive the findings. Meet the
   `cf-delegate` five-obligation evidence contract
   (launch/provenance/return/failure/recheck): a Codex reply counts only
   with its native thread ID, and a Claude reply only with its lifecycle
   records. Never paste the delegate's reply as your finding; an unverified
   claim is a defect.
