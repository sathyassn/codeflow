---
name: cf-consult
description: Get an independent second opinion from another vendor's CLI (read-only), then synthesize it against your own analysis. Use when you want an outside pass on a file, diff, design, or question.
---

# cf-consult — an independent second opinion

You are getting a second opinion, not handing off work. Load the `cf-delegate`
skill for the consult doctrine and lane details. Consult is read-only: the
delegate reads and reasons, never edits. Transport is interactive-only, one
lane per direction (ADR-0018) — never headless (`codex exec`, `claude -p`).

The delegate is a vendor you are **not**. Consulting your own vendor is
self-review with extra steps, not an outside opinion — never label it
independent.

1. Frame the ask: state exactly what to review (paths, diff, or question) and
   the criteria to judge against, and ask for an explicit closing verdict line
   (`VERDICT: approved|changes_requested`) so the reply is checkable. Do your
   own analysis first — the consult sharpens it, it does not replace it.
2. Pick the lane by your seat:
   - **From Claude Code → codex**, through the official `codex-plugin-cc`
     plugin: `/codex:review` is the diff/design read (read-only;
     `/codex:adversarial-review` for the security lens). Preflight: the
     `/codex:*` commands exist and `codex login status` exits 0.
   - **From codex → claude**, by driving the interactive `claude` CLI in tmux:
     `tmux new-session -d -s consult 'claude'`, `send-keys -l` the framed
     prompt (then a separate `send-keys Enter`), and poll `capture-pane -p`
     until the output stabilizes — two identical captures a few seconds apart
     with the input prompt back. The interactive TUI has **no sandbox flag**:
     read-only is by instruction ("read and reason only; edit nothing") —
     posture, not enforcement. Preflight: `claude` on PATH and `tmux` present.
3. The named vendor's CLI missing, the plugin surface absent, or auth failing
   → tell the user the remedy (`codex login`; install the plugin from a Claude
   Code session; install `claude`/`tmux`) and **stop, loudly** — never
   automate auth, and never quietly substitute your own vendor for the
   missing one.
4. Synthesize: compare the second opinion against your own analysis point by
   point, citing where you **agree** and **disagree** and why — with your own
   evidence (file:line, command output). Branch on the verdict line, then
   re-derive the findings. Never paste the delegate's reply as your finding;
   an unverified claim is a defect.
5. Offer a follow-up in the same session — the plugin thread is resumable, and
   the tmux pane keeps its context: send the next question to the same pane.
