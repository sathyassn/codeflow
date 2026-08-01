# Portal content contract

## Authority

- Read content from configured repository-relative source roots.
- Keep source Markdown and code authoritative; generated pages are disposable.
- Preserve source path, full source hash, repository commit, portal version,
  output hashes, and bounded line-range snippet hashes in evidence.
- Build only from one clean, full Git commit. Configuration, adopted runtime
  inputs, source roots, optional primitive tokens, and referenced local media
  must match that commit before and after the snapshot read; dirty, staged,
  deleted, untracked, ignored, inaccessible, or changing inputs block
  publication. Read claimed files through bounded, no-follow handles and order
  evidence with the starter's locale-independent comparator. Never label
  worktree bytes with `HEAD`.
- Resolve relationships from declared frontmatter and strict stable IDs. Inline
  ID mentions may become links, but never invent a declared dependency.
- Treat ADR amendment headings as current history. Do not flatten a later Note,
  Update, or Correction into the original decision date.

## Identity and links

Accept only project-supported strict IDs such as `CAP-001`, `ADR-001`,
`EPC-001`, `SPC-001`, and `TSK-001`. Routes must be unique and deterministic.
Every relationship target must exist. Derived backlinks must be the exact
inverse of declared forward relationships.
Repository-relative document links resolve through the source-to-route graph.
Local media is copied only from committed, bounded, signature-checked PNG,
JPEG, GIF, WebP, or AVIF files and recorded in evidence. Remote images, active
SVG/PDF copies, traversal, unsupported schemes, and broken targets fail closed;
ordinary HTTPS and mail links remain links and are never fetched.

## Failure and staleness

Fail closed on malformed configuration, traversal, symlink escape, route
collision, duplicate identity, invalid frontmatter, or unsupported evidence
schema. If a future adapter deliberately retains the last good rendering after
a source failure, mark it stale visibly and exclude it from search and normal
current-content indexes. Never silently serve stale content as current.

## Rendering

Parse GFM through the pinned Markdown syntax tree. Escape raw HTML outside code
nodes with no version-1 bypass; keep code, comments, references, headings, and
visible prose structurally distinct. Neutralize configuration, frontmatter,
diagnostic, and relationship text before reinserting it into generated
Markdown. Strict-ID mentions remain normal links with source-grounded
contextual previews. Use source-backed diagrams or explanations only when they
reduce cognitive load; do not manufacture facts to fill a layout. A missing
content layer may be collapsed honestly.
