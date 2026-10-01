---
id: ADR-0032
uid: 596a84e9-53f5-4535-88f4-bbcc7992728f
title: make editorial quality contextual and on demand
date: 2026-07-18
status: accepted
superseded_by: null
architecture_impact: the standard/full scaffold gains one mirrored editorial-review skill, a compact always-loaded principle, duo/reviewer/ship/customize routing, and behavioral evaluation without a prose linter dependency
---

# ADR-0032: contextual editorial quality

## Context

CodeFlow already requires evidence, audience-shaped outputs, proportionate
design, and independent Fable review. Those rules do not fully protect
substantial prose from semantic drift, generic voice replacement, unsupported
confidence, sycophancy, inflation, or formatting that obscures the outcome.
Encoding a long style checklist in AGENTS.md would spend always-loaded context
on work that most turns do not need. Mechanical word, punctuation, emoji, or
authorship detectors would also reject legitimate technical and project voice.

## Decision

Add `cf-editorial-review` as an on-demand standard/full skill mirrored
byte-identically across Claude and Codex skill surfaces. Trigger it for
substantial documentation, ADRs, proposals, release notes, PR narratives,
operator communications, and user-facing copy — not every short response.

Resolve editorial conflicts by verified truth/evidence and policy first,
CodeFlow philosophy second, the consuming project's documented voice and
human-approved examples third, audience/medium/task fourth, and explicit user
direction fifth. Preserve exact technical meaning and credible voice. Never
fabricate personality, experience, feelings, familiarity, or slang.

Review the complete artifact or a coherent section in context, clustering
related items by audience and purpose. Diagnose actual defects before making
the smallest sufficient edit. A progressive-disclosure reference provides
examples of credibility, structure, and delivery smells while explicitly
protecting legitimate punctuation, terminology, lists, headings, and
contextual emoji. Do not add Vale, an AI detector, or a lexical/style blacklist.

Route substantive prose through the existing duo, reviewer, ship, and customize
flows without duplicating the skill's detailed contract. Both primary seats
verify facts and technical meaning; the directly invoked Fable primary owns
the final contextual editorial verdict. Add `CF-OUT-002` with paired behavioral
cases for technical prose, operator updates, voice preservation, inflated PR
narratives, contextual emoji, and false-positive resistance.

## Consequences

- Always-loaded guidance grows by one principle-level paragraph, while detailed
  editorial reasoning loads only for consequential prose.
- A consuming project's established voice is preserved when credible; absent
  guidance does not authorize an invented persona.
- Editorial review cannot weaken evidence, policy, technical precision, or
  repository-specific output requirements.
- The evaluator can detect both under-review and over-correction, including
  blanket removal of valid punctuation, lists, terms, or emoji.
- There is no new binary command, runtime dependency, CI prose linter, or
  authorship classifier.

## Sources

- Google developer documentation voice and tone:
  <https://developers.google.com/style/tone>
- Microsoft Learn style guide:
  <https://learn.microsoft.com/en-us/contribute/content/style-quick-start>
- LAMP expert-editing study:
  <https://arxiv.org/abs/2409.14509>
- `blader/humanizer` and the OneAway Humanizer pattern catalogs, used as
  comparative inputs rather than copied policy:
  <https://github.com/blader/humanizer>
  <https://oneaway.io/skills/humanizer>
