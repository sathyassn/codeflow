---
name: cf-editorial-review
description: Review or revise substantial repository and user-facing prose without semantic drift. This description is the one trigger, by consequence. Use for substantial documentation, ADRs, proposals, release notes, operator communications, and user-facing copy whose structure, voice, credibility, or audience fit materially affects the outcome, in the same pass as the technical review where one is due. Do not invoke for every PR body, short conversational response, mechanical text substitution, exact quoted text, or generated machine-readable content.
---

# cf-editorial-review: preserve meaning, improve delivery

**Write plainly.** Everything an agent writes, replies and status updates
included, is simple, straightforward and clear, with the detail the reader
needs and no more. Avoid mannered prose, writing that performs for effect:
slogans, "not X but Y" turns, rhetorical triplets, dramatic fragments,
stacked hedges, colon reveals, self-narration, ceremonial framing and walls
of text. State the fact directly. Default to short prose and bullets, and
write long prose only when the reader asks for it or the artifact is prose
by nature. "Mannered prose" in
[references/editorial-smells.md](references/editorial-smells.md) lists each
pattern with its plain rewrite.

Review the artifact in its real project, audience, medium, and task context.
Improve clarity and credibility without flattening a legitimate voice or
inventing one.

This skill, its copy guide and its contextual-smells reference are the
canonical CodeFlow home for shared language guidance. Load
[references/copy-guide.md](references/copy-guide.md) when writing and
[references/editorial-smells.md](references/editorial-smells.md) when
reviewing. Other skills route here instead of copying
title, emoji, personality, or authority rules into parallel checklists.

## Authority order

Resolve conflicts in this order:

1. verified truth, evidence, exact technical meaning, and governing policy;
2. CodeFlow's evidence-bound, proportionate, honest working philosophy;
3. the consuming project's documented voice and human-approved examples;
4. the audience, medium, and task;
5. explicit user direction.

Never use a lower layer to distort a higher one. Surface the conflict when a
requested tone would overstate certainty, hide a limitation, violate policy, or
misrepresent the author or project. Within those limits, explicit operator
style direction outranks this skill's defaults.

## Review workflow

1. **Anchor the artifact.** Identify its purpose, audience, medium, required
   action, evidence, constraints, and canonical project voice source. If voice
   is undocumented, preserve the artifact's credible existing register; do not
   manufacture personality, experience, feelings, familiarity, or slang.
2. **Read coherent context.** Review the whole artifact or a complete section,
   plus the surrounding project material needed to understand it. For a batch,
   cluster items by shared purpose and audience, not one global rewrite.
3. **Protect meaning first.** Freeze identifiers, commands, numbers, citations,
   qualifications, decisions, requirements, and security or compatibility
   claims. Verify material assertions or mark them unverified. Never trade
   precision for fluency.
4. **Diagnose before rewriting.** Find actual problems: missing context,
   unsupported confidence, sycophancy, inflated importance, generic filler,
   repeated conclusions, abrupt fragments, muddled hierarchy, inconsistent
   terminology, or formatting that obscures the message. For ambiguous cases,
   consult [references/editorial-smells.md](references/editorial-smells.md).
5. **Make the smallest sufficient edit.** Preserve sound language, useful
   structure, domain terms, and the author's recognizable register. Rewrite a
   sentence, paragraph, or section only when the diagnosed issue warrants it;
   do not restyle unaffected material for consistency theater.
6. **Shape for use.** Lead with the outcome or decision, then supply the context
   and evidence needed at that altitude. Use prose, lists, tables, headings,
   punctuation, and emoji only when they fit the information, documented voice,
   medium, and repository policy. Titles, headings, navigation, and action
   labels normally name the actual subject or action in words; a bare record
   identifier or unexpanded acronym is a smell (ADR-0067), and a more
   expressive label must be earned by the product voice and remain
   understandable in context.
   Utility copy does not become product voice, and CodeFlow does not supply a
   personality for either. Keep formatting proportionate: a simple
   answer needs no apparatus, and when a relationship is materially clearer
   drawn, use a diagram whose scope and detail fit the explanation, in one
   of the nine families of the explanation method
   (`cf-present/resources/explanation-method.md`) and in the form the
   surface renders, as `.codeflow/rules/writing.md` "Figures by surface"
   sets out. In a Markdown file (a README, doc, record or PR body) that form is fenced ASCII, and on a
   docs-portal page it is the portal's figure grammar. Prefer the least
   complicated form that remains complete, not the physically smallest;
   complex subjects may need a larger, layered, or multi-view diagram. Add a
   brief caption or legend when it aids orientation. A decorative or forced
   diagram, heading, table, or recap is a defect, not polish. There is no
   universal word, punctuation, formatting, or emoji blacklist; the only fixed
   exclusions are the ones project policy names, and ADR-0067 lists them.
7. **Verify the revision.** Compare original and revision for lost meaning,
   changed certainty, dropped caveats, altered terminology, unsupported new
   claims, accidental policy violations, and visual/verbal mismatch where the
   copy belongs to an interface. Claim localized quality only from reviewable
   localized copy and relevant language evidence; English-only review is not
   localization verification. For review-only work, report precise findings
   instead of silently rewriting.

## Duo review

Within the CodeFlow duo, both primary seats check factual and technical
correctness. The directly invoked `claude-judgment-primary` reviews the
substantial artifact's design, voice, and final editorial quality, even when
Claude drafted it, in the same pass as the technical review where one is
due. Helpers may collect evidence but do not own the judgment. If the
selected primary is unavailable, record the fallback and reduced assurance.

## Review output

Return the revised artifact when editing was requested. Otherwise return only
material findings, each anchored to a section or exact excerpt, with the risk
and smallest useful correction. Keep optional taste preferences separate from
meaning, evidence, policy, or audience-fit defects. State what was not verified.

Do not use an AI detector, imitate a named person, or claim that surface cues
prove authorship. Judge whether the artifact works in context.
