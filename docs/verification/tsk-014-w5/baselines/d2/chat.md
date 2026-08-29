# Ordinary chat reply — D2

What an agent would type into the terminal today. Retained as a plain baseline; it is
not a design.

---

SPC-004 is `approved`, created 2026-07-31. Four accepted decisions are linked to it:

| Claim | Section | Decision | Kind of link | Evidence |
|---|---|---|---|---|
| CLM-1 | § Behavior 1 | ADR-0050 | governs | a run exists at the exact candidate |
| CLM-2 | § Behavior 10 | ADR-0049 | governs | a run exists at the exact candidate |
| CLM-3 | § Behavior 8 | ADR-0049 | governs | evidence exists but was not promoted here |
| CLM-3 | § Behavior 8 | ADR-0052 | supersedes one named mechanism | evidence exists but was not promoted here |
| CLM-4 | § Interfaces and formats | ADR-0052 | governs | evidence exists but was not promoted here |
| CLM-5 | § Behavior 3 | ADR-0051 | governs the process | none recorded |

ADR-0052 is the most recent, and it says exactly how far it goes:

> ADR-0049 and ADR-0050 remain historical decisions; this ADR supersedes only their
> same-tree runtime assumption and post-create Windows DACL mechanism where those details
> conflict.

> This decision changes no product outcome, task owner, graph, or integration target.

`superseded_by` is `null` on ADR-0049, ADR-0050 and ADR-0052 alike, so frontmatter alone
does not tell you which one is in force for a given claim.
