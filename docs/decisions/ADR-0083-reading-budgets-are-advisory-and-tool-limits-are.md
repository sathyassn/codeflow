---
id: ADR-0083
uid: 119bcd1e-e915-42d2-89bb-99e90888a021
title: "Reading budgets are advisory and tool limits are hard"
date: 2026-10-11
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: none # none | one line naming what in architecture.md changes
---

# ADR-0083: Reading budgets are advisory and tool limits are hard

## Context

TSK-150 turned the byte caps on the shipped instruction files into
guideline numbers that `codeflow doctor` reports, but the numbers stayed
byte literals set one percent above the measured sizes, sixteen of them
per skill, and the written rule that a file moving past its number sets
the new number in the same change made them caps again: a chain merge
left the reviewer agent 86 bytes over and the text was trimmed. The
research of 2026-10-11 (TSK-267) found one tool limit behind any of these
numbers, Codex's `project_doc_max_bytes` of 32 KiB by default, one API
limit, a skill or agent description of at most 1,024 characters, and two
vendor conventions stated in lines: a skill or rule body under 500 lines
(Anthropic's skill authoring guide, Cursor's rules guide) and the
always-loaded CLAUDE.md set under 200 lines (Claude Code's memory guide).
The operator asked that no number impose a cap that handicaps the work,
that proliferation still be controlled, and that the numbers be
configurable where that makes sense.

## Decision

A size number is hard only where a tool imposes it: the Codex instruction
limit on the whole `AGENTS.md` stays a failing test for the generated
document and a doctor warning for a project, and reads
`project_doc_max_bytes` from the project's `.codex/config.toml` when it is
set; a description over 1,024 characters is a doctor warning. Every other
number is an advisory guideline in the unit its vendor publishes, reported
by `codeflow doctor` as a note that states the measure and its basis,
never a warning: a skill or agent body against 500 lines, the always-loaded
instruction set against 200 lines, the per-task reading chain against 160
KiB (about one fifth of a 200k window at four bytes per token), and the
kernel against the number derived from the Codex limit less a 16 KiB
project section and the header. A number without a basis is removed: the
per-skill byte table, the class fallbacks, the rule count, the rule line
bytes, the agent and CLAUDE byte numbers and the guidance block number, in
CodeFlow's own repository too. A project tunes the advisory numbers in the
user-owned `[reading]` section of `.codeflow/project.toml`, and the PR body
word warning follows `git.pr_body_max_words` (1000 by default, 0 off). A
guideline number changes only with a stated basis and a line in this
record; a measure above it is a prompt to move detail behind a trigger,
never to cut a duty. The structure test and review, not numbers, control
proliferation. This supersedes the guideline table of the v2 charter's
section 4.4.

## Consequences

- An edit that grows a shipped file past an advisory number produces a
  note, exit 0, with the convention named; no task may make "within its
  guideline" an acceptance criterion on an advisory number, and a reviewer
  reads the note as a prompt to look at structure.
- A fresh install reports clean with headroom (the largest shipped skill
  is 408 lines; the chain is 132 KiB), so a note still points at a change.
- The Codex check, the one byte failure, moves with a project's own
  `project_doc_max_bytes`; a project that raised it is not warned at 32
  KiB. The derived kernel number moves with it too.
- `codeflow doctor --check reading` changes its output: lines for bodies
  and the always-loaded set, bytes for the kernel and the chain, a note
  instead of a warning, and when the installed skills' read edges differ
  from the map the running binary ships, the chain as information naming
  both versions. Nothing fails that passed before.
- Two new configuration surfaces: `[reading]` (`kernel_max_bytes`,
  `chain_max_bytes`, `skill_max_lines`, `instructions_max_lines`) and
  `git.pr_body_max_words`. Both are optional with serde or load-time
  defaults; `codeflow update` writes neither, and an older policy file
  parses unchanged.
- The description limit becomes a failing contract test on shipped files,
  since the API refuses such a file; the longest shipped description is 685
  characters.
- Ruled out: a byte number per skill anywhere, a count cap on rules, and a
  convention that resets a number to the measured size.

## Architecture impact

None. `docs/capabilities.md`, `docs/troubleshooting.md` and
`docs/policy-reference.md` carry the check and the keys.
