---
description: Get an independent second opinion from another vendor's CLI (read-only), then synthesize it against your own analysis
argument-hint: [what to consult on — a file, a diff, a design, a question]
---

You are getting a second opinion, not handing off work. Input: $ARGUMENTS

Load the `cf-delegate` skill for the consult doctrine and canonical
invocations. Consult is read-only: the delegate reads and reasons, never edits.

1. Frame the ask: state exactly what to review (paths, diff, or question) and
   the criteria to judge against. Do your own analysis first — the consult
   sharpens it, it does not replace it.
2. Pick the delegate: `codex` by default (verified, structured). Use `agy` only
   if the user names it, and only read-only (degraded tier — see the skill).
3. Preflight: `codex login status` (exit 0). Missing or unauthenticated → tell
   the user the remedy and stop; never automate auth.
4. Run the read-only consult via the skill's canonical invocation
   (`codex exec --json ... --sandbox read-only`), capturing `thread_id` for
   follow-up. For a machine-checkable verdict, add `--output-schema`.
5. Synthesize: compare the second opinion against your own analysis point by
   point, citing where you **agree** and **disagree** and why — with your own
   evidence (file:line, command output). Never paste the delegate's reply as
   your finding; an unverified claim is a defect.
6. Offer a follow-up on the same thread via `codex exec resume "$thread_id"`.
