---
id: ADR-0095
uid: 84ae92c7-a752-4603-9d79-a7528ce54f6e
title: "Default effort follows each vendor's recommendation"
date: 2026-11-01
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "docs/architecture.md, the duo orchestration row, where seats default to each model's vendor-recommended effort and are raised by trigger or task need"
---

# ADR-0095: Default effort follows each vendor's recommendation

## Context

[ADR-0056](ADR-0056-high-primary-effort-with-bounded-workers.md) made high the
default effort of every development primary, so the shipped catalog bound the
orchestrating host, both planning primaries, the designer, the executor and
the reviewers at high whatever model the seat resolved to. The operator
directed on 2026-11-01 that sessions run at medium by default unless high is
really needed, that the default depends on the model and its vendor's
recommendation, and that tokens be used efficiently.

Vendor guidance, read on 2026-11-01:

- Claude Code's [model configuration](https://code.claude.com/docs/en/model-config)
  says effort defaults to high on models that support it, except Opus 5.5,
  Sonnet 5.5 and Haiku 5.5, which default to medium.
- Anthropic's [effort guidance](https://platform.claude.com/docs/en/build-with-claude/effort)
  recommends starting at medium for most work, agentic coding included.
- For Codex GPT-6 Astra, GPT-6.1 Sol and GPT-6 Luna the operator set medium;
  for Grok 4.7 the operator set high.

The operator added on 2026-11-01 that Fable 5.1 uses tokens and usage limits
much faster than the other Claude lines.

## Decision

This record supersedes only the primary-effort default in ADR-0056. The rest
of ADR-0056 and all of ADR-0087 stand.

- **Defaults by model.** A seat or worker starts at its bound model's
  vendor-recommended default: medium for Opus 5.5, Sonnet 5.5, GPT-6 Astra,
  GPT-6.1 Sol and GPT-6 Luna; high for Fable 5.1, whose lowest effort is
  high; high for Grok 4.7. Every launch passes that effort explicitly.
- **When to raise.** A seat or worker goes to high (xhigh where the model
  offers it) only for a named need applied to that lane: a hard build, a
  security review or a stuck diagnosis. The catalog's high triggers are
  exactly those three; the two earlier category triggers (complex or
  difficult work, material ambiguity) are removed, so a category never
  raises every seat. The resolver never raises the orchestrating director or
  coordinator session by trigger; it stays at its own model's default. The
  `orchestrate` duty therefore names each seat at that seat's default
  (Claude and Codex medium, Grok high) under the same-host relation, so the
  host session resolves to its own seat at its model's default.
- **The highest-cost reasoning model directs and never executes.** In this
  ensemble that is Fable. It serves key review, analysis and direction
  (design, architecture and planning decisions). Each run names its
  deliverable and its end and is closed when done. Execution goes to Opus or
  Sonnet. No execution or evidence-collection duty routes to it, and it
  never hosts or backs the long-running orchestrating session, even as a
  fallback; tests hold both.
- **A seat line starts at its own default.** When a seat line offers
  nothing at the seat route's effort, it starts at its own lowest effort
  above it, so Fable keeps high as a design co-owner or an advisory
  fallback. The orchestrating session is never raised, so this does not
  apply to it. Line and worker routes are never clamped.
- **Catalog.** Seat-targeted duties in `current-ensemble.json` carry medium,
  design included, except Grok seats at high. The duty floor is medium for
  every duty except `reasoning-support`, the stronger-reasoning request,
  which keeps high, and `xhigh-reasoning`, which keeps xhigh; a design
  OPERATOR_OVERRIDE has no separate floor. A high trigger raises a
  seat-targeted participant on that lane as it already raised a
  line-targeted one. A seat at medium resolves only to a line that supports
  medium, so the Claude seat at medium resolves to Opus, and Fable is
  reached only through high-effort review, reasoning and consultation
  routes.
- **Project bindings.** A binding in `.codeflow/model-selection.json` pins
  the model and its qualified effort. It accepts a request at or below that
  effort, the vendor-default request included, and resolves at the bound
  effort. A request above the bound effort is refused, because the binding's
  evidence does not cover it.

## Consequences

- Routine orchestration, planning, building and review run at the vendor
  default and use fewer tokens; hard work still reaches high through triggers.
- A project that wants high everywhere sets it per person (ADR-0087) or binds
  high in `.codeflow/model-selection.json`; the floor no longer forces it.
- Earlier qualification evidence recorded at high stays valid for the high
  tuples it names. A medium tuple needs its own native evidence before a
  binding is promoted.

## Architecture impact

`docs/architecture.md` and CAP-010 state the new default. The model catalog
schema is unchanged.
