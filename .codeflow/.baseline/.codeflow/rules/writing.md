# Writing

One reference for everything an agent writes: chat replies, status reports,
summaries, documents, records, commit messages and PR bodies. Managed by
`codeflow update`; the project's own voice rules go in the project section of
`AGENTS.md`.

## Replies and status

- **Outcomes first, in words.** A status report or summary names each item
  by what it achieved or what is at stake, in plain words a reader with no
  context understands. IDs, file names, numbers and branch names follow as
  references, never as the heading or the lead of a bullet.
- **Titles name the subject in words.** An identifier or bare acronym is
  never the whole title; it goes in the body.
- A longer reply or report opens with a summary that gives context only:
  what this is, why it matters, and where it stands, in one to three short
  sentences. The details follow as bullets, one point each, in a logical
  order (problem, change, effect, limits, or the order of the flow); a table
  for tabular data and a fenced block for pasted output.
- A simple answer stays simple: no figure, no headings, no recap, and a
  one-line answer stays one line.
- Durations for agent-delivered work are agentic estimates with stated bases;
  see "Durations" in `workflow-discipline.md`.
- When a reply names a link (a pull request, a served page, a file), give the
  exact link a tool printed or one you verified. Never guess a URL, port, or
  pull request number; state an unknown link as unknown.

## Figures by surface

When the point is a flow, dependency, structure, state change, or
comparison that is clearer drawn, the reply or document carries a figure.
Match the form to the surface:

- A multi-part explanation, comparison, plan or decision that benefits from
  one coherent surface and anchored feedback goes through `cf-present` where
  the harness can show it (standard and full tiers); say why you opened it.
- Where the harness renders one, use an inline HTML figure.
- Use fenced ASCII only on a terminal or other plain-text surface, or when
  unsure what the surface renders.
- Never use Mermaid.

## Shape the deliverable

**Shape the deliverable.** Layer it concept before detail, each layer
complete at its own altitude; condense by layering, never by cutting key
information. Keep presentation proportionate. Bullets for the enumerable,
prose that earns its place, and a figure whose scope fits the explanation
when structure, state, or a decision is materially clearer drawn. Use the
least complicated form that stays complete, not the physically smallest;
complex subjects may need a larger or layered view, with a caption or legend
when useful. Never add decorative or forced diagrams, headings, tables, or
recaps. Before done, take the audience's seat: structured, logical,
progressive? Sloppy work is a defect, not a style.

In prose, verified truth, policy, technical meaning, project voice, and accessibility
outrank decoration or fabricated personality: verified truth and policy outrank documented project voice,
audience, medium, task, and requested tone; preserve technical meaning and
never invent personality, experience, feelings, familiarity, or slang. At the
standard and full tiers use `cf-design` for material product, UX, UI,
interaction, or visual direction and `cf-editorial-review` for substantial
prose.

## Written content policy

The written content policy (ADR-0067): no em or en dash in new text; use a
comma, colon, full stop or hyphen instead. `git.policy_characters` checks
commit messages, PR bodies and added lines under `docs/`,
`project-management/` and skill trees (warn by default; a project may set
block); old lines are exempt. Write plainly: no slogans, no "not X but Y"
turns, no rhetorical triplets or dramatic fragments, no walls of text. No
emoji or AI attribution in commits and PR bodies. No hook sees a chat reply,
so these rules hold there by discipline, and evaluation judges them.
