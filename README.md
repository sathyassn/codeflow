# CodeFlow

**The AI-development discipline layer you install into any repo.**

One Rust binary (`codeflow`) that scaffolds, enforces, verifies, and remembers — while Claude Code (or any harness) does the developing.

## Status

**v2 under construction.** The v1 framework (PathFlow, agent teams, CRDT coordination) is archived in full at branch [`archive/v1`](../../tree/archive/v1) (tag `v1-final`). v2 is a ground-up rebaseline for Fable-class models: clarity in, light rails through, verification out.

The plan of record is [`docs/plan/v2/00-charter.md`](docs/plan/v2/00-charter.md).

## What v2 will provide

- `codeflow init` — scaffold the AI-dev discipline layer into any repo (three tiers: minimal / standard / full)
- Git discipline as config-driven policy: protected branches, commit standards, secrets, enforced at git-hook + harness + CI + remote planes
- A generic, stack-agnostic test gate (`codeflow test`)
- A knowledge model that doesn't rot: product → capabilities → architecture/ADRs → work → trace
- Cross-repo work records and recall (`codeflow status --all`, `codeflow recall`)
- Native-first: rides Claude Code's worktrees, subagents, workflows, and memory — never reimplements them
