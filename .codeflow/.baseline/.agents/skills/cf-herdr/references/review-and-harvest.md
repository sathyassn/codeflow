# cf-herdr: harvest a reply, and run a review or consult seat

Read this after `../SKILL.md` when a Codex or Grok seat's turn has started,
or before a cross-family review or consult runs in Herdr.

## Harvest and provenance

This is the evidence of a Codex or Grok seat under `cf-delegate`'s
five-obligation contract. Launch is the delivery script's confirmed started
turn. Wait until `herdr agent get <pane>` shows the seat's status leave
`working` for that turn, then read the reply with
`herdr pane read <pane> --source recent-unwrapped --lines <N>`. A
full-screen seat keeps only part of a long reply in the pane, so read a long
Codex reply from the seat's own Codex session record, the `rollout-*.jsonl`
file under `~/.codex/sessions/` whose session id matches the seat. That
record, and the seat's status line, give the observed model and effort;
without either, record them as requested. Read only the record of the seat
this run started; the record is never a completion signal. Recheck is the
resumable Codex session (or Grok session) by its id.

## Review and consult seats

A cross-family review or consult in Herdr runs this way, and only this way:

1. An interactive seat in its own `cf-` tab ("Create or resume" in the
   skill).
2. Launched in its CLI's autonomous permission mode, the consult and review
   row of the transport posture table, so it never stalls on an approval.
3. First-run prompts handled as the transport rule's "First-run prompts"
   section says: folder trust for the task's own folder only; hook trust
   only when each hook file matches `git show <target-tip>:<path>` byte for
   byte, otherwise the operator is told the seat is waiting in this tab.
4. The brief follows the review brief contract in the orchestrator's
   `resources/quality/findings.md`: one holistic pass over the whole unit at
   one head, its blast radius included. It is delivered with `deliver.py`.
5. The reply is harvested as above, once the status leaves `working`.
6. If the seat's client withholds a security verdict through a content
   filter, ask the same seat to restate it as a defensive review.
7. Later rounds of the same unit resume this tab; the tab is closed after
   the unit lands.
