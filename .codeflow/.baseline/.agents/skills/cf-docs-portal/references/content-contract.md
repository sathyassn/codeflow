# Portal content contract

## Authority

- Read content from configured repository-relative source roots.
- Keep source Markdown and code authoritative; generated pages are disposable.
- Preserve source path, full source hash, repository commit, portal version,
  output hashes, and bounded line-range snippet hashes in evidence.
- Build only from one clean, full Git commit. Read configuration, source
  Markdown, optional primitive tokens, and referenced local media from bounded
  Git blobs at that commit. Compare the adopted runtime and every configured
  worktree input—configuration, source Markdown, optional primitive tokens,
  and referenced media—byte-for-byte with those blobs; dirty, staged, deleted,
  untracked, ignored, inaccessible, masked (`assume-unchanged` or
  `skip-worktree`), or changing inputs block publication. Read worktree files
  through bounded, no-follow handles and order evidence with the starter's
  locale-independent comparator. Never label worktree bytes with `HEAD`.
- Resolve relationships from declared frontmatter and strict stable IDs. Inline
  ID mentions may become links, but never invent a declared dependency.
- Treat ADR amendment headings as current history. Do not flatten a later Note,
  Update, or Correction into the original decision date.

## Identity and links

Accept only project-supported strict IDs such as `CAP-001`, `ADR-001`,
`EPC-001`, `SPC-001`, and `TSK-001`. Derive each route from its semantic layer
plus its path relative to the most-specific configured source root; physical
root moves therefore need not leak into reader-facing URLs. Routes must be
unique and deterministic.
Every relationship target must exist. Derived backlinks must be the exact
inverse of declared forward relationships.
Repository-relative document links resolve through the source-to-route graph.
Local media is copied only from committed, bounded PNG, JPEG, GIF, or WebP
blobs whose signatures, headers, dimensions, and aggregate pixel/byte budgets
are verified and recorded in evidence. Remote images, active
SVG/PDF copies, traversal, unsupported schemes, and broken targets fail closed;
ordinary HTTPS and mail links remain links and are never fetched.
Pin source links to the evidenced commit on known GitHub, GitLab, and Bitbucket
HTTPS repository URLs. A committed document excluded from the portal remains a
pinned provider link; for another provider, show a visible source path and
commit without manufacturing a route. A genuinely absent document still fails.

## Failure and staleness

Fail closed on malformed configuration, traversal, symlink escape, route
collision, duplicate identity, invalid configuration, or unsupported evidence
schema. A configured source root must be a non-empty committed directory. Every
publishable Markdown source selected by the committed configuration and tree
must have exactly one current page or error stub in evidence. When one current
committed Markdown blob cannot parse, emit only a bounded visible
error page at its stable route. Record the current source path, blob hash,
commit, output hashes, and bounded diagnostic; clear IDs, relationships,
backlinks, snippets, and status; and exclude the page from search, previews,
normal `llms.txt`, and the active graph. Continue healthy sibling pages. Never
walk Git history, republish an ancestor, or trust prior generated output or
evidence as content authority.

Generated public namespaces are `public/markdown/`, `public/media/`, and
`public/llms.txt`; each publication replaces them completely so deleted sources
cannot survive as reachable output. Other project-owned public files may be
preserved only through the bounded no-follow publication contract.

## Rendering

Parse GFM through the pinned Markdown syntax tree. Escape raw HTML outside code
nodes with no version-1 bypass; keep code, comments, references, headings, and
visible prose structurally distinct. Neutralize configuration, frontmatter,
diagnostic, and relationship text before reinserting it into generated
Markdown. Strict-ID mentions remain normal links with source-grounded
contextual previews. Use source-backed diagrams or explanations only when they
reduce cognitive load; do not manufacture facts to fill a layout. A missing
content layer may be collapsed honestly.
