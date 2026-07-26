---
id: ADR-0042
title: combine selected deterministic analyzers with contextual agentic verification
date: 2026-07-25
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — verification evidence is recorded in deterministic and contextual layers, with stack-specific analyzer selection and a repository-only post-public CodeQL canary
---

# ADR-0042 — layered code verification

## Context

Tests and model review answer different questions. Formatters, linters,
dependency scanners, SAST/dataflow tools, secret scanners, and architecture
checks are repeatable but cannot establish product intent or deep business
semantics. Cross-model review can reason about intent, state, emergent anomalies,
performance, and design quality but must not waive a deterministic red result.
A single universal analyzer would also be wrong across CodeFlow's consuming
stacks and hosting environments.

## Decision

Verification is intentionally layered and transparent:

```text
deterministic evidence                 contextual evidence
syntax/style, SCA, data/control flow   intent, business logic, deep semantics
taint, secrets, architecture rules   + performance, state/environment, anomalies
                    \                 /
                     integrated verdict
```

The settled plan selects only analyzers justified by the language, framework,
risk, hosting entitlement, existing CI, and quality of available rules.
`cf-stack` records project-owned commands in `.codeflow/test-config.json`;
`cf-customize` verifies the chosen local and CI paths, ownership, suppressions,
and gate level. Neither skill auto-installs a tool or prescribes CodeQL,
Semgrep, Sonar, or another vendor universally. When a relevant deterministic
SAST/taint lane is unavailable, the evidence ledger states the residual risk;
model agreement never converts the gap into a deterministic pass.

For CodeFlow itself, GitHub CodeQL is a repository-specific, server-side
complement after the repository becomes public. No CodeQL action, dependency,
or consumer scaffold is committed. The operator enables GitHub default setup
for Rust with `security-extended`, confirms expected file coverage and zero tool
errors in tool status, and observes five consecutive applicable pull-request
runs before considering the check required. CodeQL findings feed the same
security triage as other deterministic analyzers and never replace CodeFlow
hooks, `codeflow ci`, policy gates, tests, or cross-lineage review.

Model qualification adds role-neutral cases for the durable judgment duties,
keeps requested/observed identity as concrete binding evidence, rejects
lineage collapse, and adds layered-verification plus seeded-history
craftsmanship cases. A Fable-versus-Opus primary comparison changes only
`system.model`, runs the full suite three times per case, reports variance, and
cannot trade a semantic regression for cost or latency. Promotion requires
human approval; adoption then uses an event-based probation with a selection
revert as rollback.

## Consequences

- Repeated tools cover known patterns while independent models examine meaning
  and unexpected interactions; neither evidence class impersonates the other.
- Consuming projects gain a durable selection method without inheriting a
  vendor, paid service, or irrelevant gate.
- CodeFlow can benefit from GitHub's Rust dataflow analysis after the public
  flip without compromising portable scaffold behavior.
- The repository has no local CodeQL proof before activation; releases record
  that limitation rather than claiming the future gate passed.

## Rejected

- Sonar as a universal default: centralized quality governance may suit some
  teams, but it duplicates native stack checks and adds service administration
  that CodeFlow cannot justify for every consumer.
- A committed advanced CodeQL workflow: GitHub recommends default setup first,
  and an in-tree workflow would add maintenance without improving portability.
- `semgrep --config auto` as an unconditional floor: language/rule quality,
  network use, and false-positive ownership must be evaluated per project.
- Scanner or model consensus as authorization to ignore another red gate.

## References

- [Configuring CodeQL default setup](https://docs.github.com/en/code-security/code-scanning/managing-your-code-scanning-configuration/set-up-code-scanning-with-default-setup)
- [Code scanning tool status and file coverage](https://docs.github.com/en/code-security/code-scanning/managing-your-code-scanning-configuration/about-code-scanning-tool-status-page)
- [CodeQL language and query-suite support](https://docs.github.com/en/code-security/code-scanning/introduction-to-code-scanning/about-code-scanning-with-codeql)
