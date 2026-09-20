---
id: ADR-0027
title: qualify model and harness bindings with native-interactive evaluations
date: 2026-07-17
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — the scaffold gains a model-qualification skill with requirement traceability, exact disposable fixtures, deterministic scoring, comparison, and fail-closed cleanup
---

# ADR-0027: native-interactive model evaluation

## Context

CodeFlow's behavior depends on the model, reasoning effort, harness, loaded
artifacts, tools, permissions, network boundary, and resource budget together.
A new model can follow individual markers while weakening the actual method; a
prior compression changed two independent plan drafts into Claude planning plus
Codex critique while marker-based tests stayed green. CodeFlow also lacked a
repeatable way to compare a new production binding with its current baseline.

Current evaluation practice treats the harness and environment as part of the
system under test, repeats stochastic trials, combines deterministic, model and
human graders, inspects trajectories, separates regression from capability
suites, and records validity threats. This follows Anthropic's
[agent-evaluation guidance](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents)
and OpenAI's
[trustworthy-evaluation playbook](https://openai.com/index/trustworthy-third-party-evaluations-foundations/).

## Decision

Ship `cf-evaluate-model` at standard/full tiers. It qualifies a declared
model+harness configuration through native interactive Codex App/CLI or Claude
Code sessions with the real tools, MCPs, settings and permission profile being
evaluated. It never invokes a subject model through `codex exec`, Claude print
mode, a raw API, desktop GUI scripting, or a hand-rolled peer driver.

The skill owns:

- stable hard-requirement IDs with source markers and behavioral-case links;
- balanced regression and capability cases, including the two-independent-plan
  anti-anchoring canary, hard 80% coverage floor, dual-vendor security lenses,
  plan-version invalidation, native-session routing, worktree parallelism, and
  token-efficiency-without-semantic-loss;
- exact JSON fixture overlays materialized on the scaffold revision under test;
- a fresh, one-commit disposable repository at an opaque neutral path for each
  trial, with evaluator state kept outside the subject tree and grader files
  removed before history is rebuilt;
- deterministic expected-versus-observed scoring, exact canary/full trial
  accounting, baseline comparison, and explicit validity flags;
- cleanup limited to a marked run root and gated by an exact run-ID
  confirmation.

Canary runs exercise selected regressions once during corpus development. Full
qualification runs every case three times and is mandatory before promoting a
new production model, harness, artifact, or permission binding. Promotion
requires no hard-case regression, complete evidence, resolved grader
disagreement and validity threats, and explicit human approval. Lower token use
or latency never compensates for a semantic regression.

The kit is not an automated model runner. The evaluator opens and supervises
the native sessions because the production contract requires those harnesses
and their configured tools. The script handles deterministic suite validation,
fixture construction, result scoring/comparison and safe cleanup only. There is
no new CodeFlow CLI subcommand, model runtime, CI model call, or generic cleanup
skill.

## Consequences

- New models can be compared against a pinned baseline without confusing one
  lucky run with reliable adherence.
- Hard doctrine has requirement-to-source-to-case traceability, so size or
  marker tests cannot silently substitute for behavioral intent.
- Subject fixtures do not expose the evaluation skill or its expected answers,
  reducing grader leakage; evaluation awareness remains a validity check.
- Full qualification is deliberate and potentially expensive. Deterministic
  suite checks run locally/CI; model trials run only for meaningful binding
  changes.
- The corpus needs maintenance: observed failures become regression cases,
  saturated capability cases graduate rather than disappear, graders remain
  human-calibrated, and baselines are re-run when cases or fixtures change.

## Architecture impact

`assets/base/agents/skills/cf-evaluate-model/` is a fully managed mirrored
skill. It contains the protocol, requirement/case/fixture registries and one
standard-library Python tool. Repository tests validate source markers,
traceability, fixture safety, scoring and cleanup. The binary remains model
agnostic.
