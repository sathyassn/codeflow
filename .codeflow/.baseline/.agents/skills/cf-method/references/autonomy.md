# Autonomy with judgment

Use this reference to decide whether a step is yours to take. It owns the
finish-line rule, the ladder, the operator-owned list, the trust prompt rule
and the settled-dissent rule. The contracts, `workflow-lifecycle.md` and
`cf-plan` point here. The hard-gate procedure stays in `AGENTS.md`, "Match the
gate to the blast radius", and this reference cites it. A gate here is any
listed stop: a question on rung 3 or a hard gate on rung 4.

## The finish line

A task runs from its settled outcome to its finish line: build, verify,
review, open the pull request, follow it, and report readiness. Settle every
discoverable fact and every reversible choice from evidence, and say what you
chose. Stop only at a question or gate this reference lists. Do not stop to
offer the next step, to confirm a safe step you already have authority for,
to report progress, or because a peer, a tool or a job is red. A stop holds
only that one action: the rest of the work keeps moving, and the stop brings
evidence, options and a recommendation. Record a missing seat, tool or credit
as reduced assurance and continue on the recorded fallback.

A brief that asks for a change selects implementation through the readiness
report. Planning, research or review alone needs a brief that asks for that
output.

## The ladder

```text
  Read from the top. The first rung that fits the step decides.

  4  HARD GATE       spend, credentials, sending outside    wait for a human,
                     the conversation, a protected merge,   for that action
                     production, an unrestorable delete,    only
                     any step with no way back, the
                     contract's hard-gate class
  3  ASK             intent or public behavior the brief    one question with
                     does not fix, taste, risk tolerance,   options and a
                     a material security boundary,          recommendation;
                     authority or scope beyond the brief    other work goes on
  2  NOTIFY AND ACT  recoverable, and visible beyond the    act, and put one
                     task: an integration merge, a          line in the
                     fallback route, a settled dissent      running report
  1  ACT             everything else inside the brief       act, and say what
                                                            you chose
```

Figure: the four rungs. Most steps sit on rung 1. "Notify" is one line in the
running report, never a pause. Rungs 3 and 4 hold only the one action.

A hard gate follows `AGENTS.md`, "Match the gate to the blast radius": exact
scope, a preview where supported, a verified checkpoint with a restore path,
and explicit authenticated human approval. CodeFlow's non-relaxable class
stays human-performed after approval, and so do the action families the
permission presets refuse: privilege escalation, publishing packages and
gists, releases and tag pushes, repository and account changes, keychain
reads and user-level persistence. They are refused in agent sessions, and
the operator runs them.

## What belongs to the operator

This is the only full list. The agent settles everything else from evidence.

Questions, rung 3:

- intent or public behavior the brief does not fix;
- taste the brief does not fix, such as a name, a skin or a voice;
- risk tolerance inside the accepted outcome that the brief does not fix:
  how much residual risk to accept, or a trade of risk against speed, cost
  or scope;
- a material security boundary the brief does not fix: where a trust, data
  or access boundary sits or moves, even when nothing is weakened; a
  weakening action is a hard gate below;
- authority or scope beyond the brief.

Hard gates, rung 4:

- spend, including buying credits;
- credentials, sign-in, IAM, privilege and secrets;
- anything sent outside the conversation, as or for the operator;
- a merge into a protected target: `main`, `master` and every protected glob
  in `.codeflow/policy.json`, including a protected `integration/` glob;
- production;
- a delete that version control, a backup or a scratch area cannot restore;
- a system-level, cross-boundary, destructive-disk or security-weakening
  action, the class `AGENTS.md` "Match the gate to the blast radius" names;
- any other step with no way back.

Ask every live question in one round. An authenticated operator direction
that settles a class of question, such as the trust prompt rule below, holds
until the operator changes it. It never opens a hard gate.

## Trust prompts

When a harness asks whether to trust a folder, answer it yourself for a path
inside your task's own authorized project or worktree, or for a disposable
sample your own harness created in this run. Any other path goes to the
operator. Decide by authorization and path identity: compare the resolved
path with the task's worktree or with the sample path your harness recorded.
A folder that another run created, or a path that only resembles
yours, is foreign. A prompt to trust hook definitions, such as Codex's review
of a project's hooks, is not a folder trust prompt. Answer it only in the
narrow case the orchestrator's transport rule defines ("First-run prompts";
ADR-0075, amendment of 2026-10-03): a Codex seat whose project hooks file is
unchanged from the fetched target tip and runs only CodeFlow's shipped
`codeflow hook` commands. Every other hook prompt, and every Grok project
trust prompt, goes to the operator.

## Settled dissent

Plan settlement and review end on evidence, with no round cap (CodeFlow
ADR-0076). A repeated attempt without a new hypothesis or changed evidence is
not progress; when repairs stop producing evidence, the owner changes
strategy with fresh evidence or surfaces a real block. That never closes a
material finding: it stays open until it is fixed. If two seats then still
disagree on a reversible choice inside the accepted outcome, the Claude
judgment primary settles it. The plan records `SETTLED_DISSENT`
with the item, both verdicts, the evidence, and why the item is reversible.
The dissenting verdict stays as given and is never recorded as approval; that
seat still approves the rest of the plan. A dissent on an operator-owned,
safety, security, correctness or evidence-adequacy axis is not settleable: an
operator-owned item is asked, and the others keep their gate closed and return
to repair.

## Decision table

| Situation | Rung and action | Source |
|---|---|---|
| A fact the repository, tools or authoritative sources can answer | 1: find it; never ask the operator to do discovery | `cf-plan` clarity gate |
| A reversible choice inside the accepted outcome | 1: choose from evidence and disclose it | `AGENTS.md` Planning and tracking; CodeFlow ADR-0038 |
| The brief asks for a change | 1: run to the readiness report | `workflow-lifecycle.md` Establish the route |
| The brief asks only for a plan, research or a review | 1: stop at that stage's output | `workflow-lifecycle.md` Establish the route |
| The next safe step inside the brief, the first time or again with the same tuple | 1: take it without asking; progress goes in the running report, never in an offer | this reference; `AGENTS.md` Act within legitimate intent |
| Routine landing mechanics: branch, worktree, integration branch, pull request shape | 1: choose them | `cf-plan` step 6 |
| A step fails and a new hypothesis exists | 1: probe it, then reroute; never repeat the same attempt | `AGENTS.md` Navigate blockers; `cf-develop` |
| A peer, tool or job is red | 1: classify and continue; fix an assertion-red check, push and poll again | `quality-contract.md` redness classes; `cf-ship` `references/pr-evidence.md` |
| A sandboxed command fails for a trusted installed tool that needs host state | 1: take the one classified unsandboxed retry without asking, never for an action the presets refuse | `CLAUDE.md` project preset; CodeFlow ADR-0029 |
| A trust prompt | 1 for the task's own project or worktree, or a sample this run's harness created: answer it; 3 for any other path: ask | Trust prompts above |
| A credit is missing, or a seat or tool is refused or unavailable after preflight | 2: do not purchase; name the gap, record reduced assurance and continue on the recorded fallback | `AGENTS.md` Entry points; `capability-routing.md` |
| A seat is lost mid-run after approval | 2: move the unit to the recorded fallback and record the reassignment where the assignment lives (an unstarted task's in the batched epic amendment); the lost seat's actual verdict stays recorded | CodeFlow ADR-0070; ADR-0076 for the reassignment |
| Two seats still disagree once repairs stop producing evidence | 2 on a reversible item: the Claude judgment primary settles it and records `SETTLED_DISSENT`, never approval; 3 on an operator-owned item: ask; not settleable on safety, security, correctness or evidence adequacy: keep the gate closed and repair | Settled dissent above; CodeFlow ADR-0070 |
| A green, reviewed pull request into an `integration/` branch that policy does not protect | 2: once the full gate is green on that exact candidate, the primary merges it without fast forward or with `codeflow integrate`, then confirms the landed state against the candidate's evidence with `codeflow status`; a changed candidate or a failed check gets the rerun that applies | `AGENTS.md` Git rules, Bodies of work; `codeflow integrate`; `.codeflow/policy.json` |
| Local evidence is green and hosted jobs never ran | 2: report "ready for your merge on local evidence" only with a completed green result of every owed check, local or hosted, and name each hosted job that did not run and why | `quality-contract.md` redness classes |
| An owed check has no completed result anywhere | a missing gate: name it as the blocker, keep the pull request draft, continue other work | `quality-contract.md` redness classes |
| An external dependency the agent cannot clear | 2: name it as the blocker with the input that clears it; ask (3) only when the operator is the one who can supply that input | `quality-contract.md` blocker navigation |
| A material risk outside scope | 2: one tracked item; escalate only an imminent severe risk | `AGENTS.md` Find broadly; CodeFlow ADR-0034 |
| Intent, public behavior, taste, risk tolerance or a material security boundary the brief does not fix | 3: ask, every live question in one round | `AGENTS.md` Planning and tracking; `cf-plan` clarity gate |
| Scope, recipient, destination or effect changed | 3: ask for a fresh grant | `AGENTS.md` Act within legitimate intent |
| Spending money, including buying credits | 4: wait for the operator; do not spend | this reference |
| Sign-in or login is needed | 4: the operator signs in; never automate it | `cf-model-orchestrator` preflight |
| A pull request into a protected target, including a protected `integration/` glob | 4: report it ready; a human merges | `AGENTS.md` Git rules |
| Sending anything outside the conversation | 4 | `AGENTS.md` Act within legitimate intent |
| A delete that version control, a backup or a scratch area cannot restore | 4 | `AGENTS.md` Match the gate; CodeFlow ADR-0075, which retires ADR-0066's delete prompts; exec-guard refuses rooted and home-anchored recursive deletes |
| Credentials, IAM, privilege, secrets, production, or a system-level, cross-boundary, destructive-disk or security-weakening action | 4 | `AGENTS.md` Match the gate |
| The session runs with `bypassPermissions`, `danger-full-access` or `--always-approve` | no rung changes: the launch removes prompts, not boundaries; the action families the presets refuse stay the operator's, and the route's sandbox where it has one, the guard hooks, git hooks and CI still apply | CodeFlow ADR-0055; ADR-0075 decision 1 |
