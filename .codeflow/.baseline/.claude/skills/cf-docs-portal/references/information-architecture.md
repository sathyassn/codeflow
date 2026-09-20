# Repository-guide information architecture

## Default: one source graph, one portal

Start with the repository's existing Markdown, decisions, work records, and
technical references as one authoritative graph. Generate one portal over that
graph. The shipped starter scans `docs/` only. Decisions, epics, tasks and specs
stay in the repository: the portal points to their folders from one generated
pointer page and does not list them page by page. A portal is a reading and
navigation view, not another documentation authority and not a folder browser.

Use progressive depth where the source material supports it:

```text
concept and purpose
  -> capability or user journey
    -> system, surface, or owned area
      -> technical reference, code, and evidence; records pointed to
```

Collapse a layer when it adds no useful distinction. A tiny repository with a
README and a few short notes usually needs better Markdown, not a portal.

## Single-project repository

Orient a reader around the product intent and its main journeys first. Connect
those concepts to capabilities, architecture in effect and implementation
references through source links; cite decisions by id, linking to the
repository file. Do not copy a
concept into every technical page. Prefer a stable route based on meaning over
navigation based only on the current folder tree.

## Monorepo

Keep repository-wide purpose, shared journeys, platform boundaries, common
architecture, vocabulary, and governance at the global level. Then provide
drill-down by meaningful surface or owned area, for example web, iOS, Android,
backend, data, or infrastructure, before module and file references.

Configure multiple `source_roots` and explicit layer paths/prefixes to assemble
that view. Reader-facing routes use the semantic layer plus the path relative
to the most-specific configured source root, not the full repository folder
chain. Preserve one stable identity and relationship graph across areas.
A source-root-level `index.md` would collapse to the reserved layer-root route;
place it below a named directory or give it a descriptive filename instead.
Cross-area links remain normal graph edges with pinned source context. Do not
create a folder-per-package navigation dump, mirror package READMEs into new
prose, or hide shared behavior inside one team's section.

## When to split

Use more than one portal only when a material boundary makes one portal
incorrect or unsafe:

- audiences require meaningfully different information or access controls;
- products release and version independently enough that one version context
  would mislead;
- ownership or operational isolation requires separate publication lifecycles;
- the corpus is too large for one navigable experience after good layering and
  search have been tried and measured.

Convenience, package count, team count, or a long sidebar alone is not enough.
When a split is justified, keep shared concepts in one authoritative location,
link across portals with explicit source/version context, and record ownership,
release, navigation, and search boundaries. Never duplicate shared prose to
make each portal appear self-contained.

## Review questions

- Can a new reader move from purpose to the relevant implementation without
  knowing the repository layout first?
- Does every generated page point to one committed source and version?
- Are shared concepts authored once and linked rather than copied?
- Do area boundaries follow audience and system meaning rather than incidental
  folders?
- Would a proposed split improve correctness, access, release truth, ownership,
  or measured usability? If not, keep one portal.
