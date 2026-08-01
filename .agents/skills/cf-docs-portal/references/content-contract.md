# Portal content contract

## Authority

- Read content from configured repository-relative source roots.
- Keep source Markdown and code authoritative; generated pages are disposable.
- Preserve source path, full source hash, repository commit, portal version,
  output hashes, and bounded line-range snippet hashes in evidence.
- Resolve relationships from declared frontmatter and strict stable IDs. Inline
  ID mentions may become links, but never invent a declared dependency.
- Treat ADR amendment headings as current history. Do not flatten a later Note,
  Update, or Correction into the original decision date.

## Identity and links

Accept only project-supported strict IDs such as `CAP-001`, `ADR-001`,
`EPC-001`, `SPC-001`, and `TSK-001`. Routes must be unique and deterministic.
Every relationship target must exist. Derived backlinks must be the exact
inverse of declared forward relationships.

## Failure and staleness

Fail closed on malformed configuration, traversal, symlink escape, route
collision, duplicate identity, invalid frontmatter, or unsupported evidence
schema. If a future adapter deliberately retains the last good rendering after
a source failure, mark it stale visibly and exclude it from search and normal
current-content indexes. Never silently serve stale content as current.

## Rendering

Escape raw HTML outside code fences with no version-1 bypass. Keep code fences intact. Use source-backed diagrams
or explanations only when they reduce cognitive load; do not manufacture facts
to fill a layout. A missing content layer may be collapsed honestly.
