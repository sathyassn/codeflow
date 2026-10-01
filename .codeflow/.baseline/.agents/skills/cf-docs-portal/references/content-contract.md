# Portal content contract

## Authority

- Read content from configured repository-relative source roots.
- Keep source Markdown and code authoritative; generated pages are disposable.
- Preserve source path, full source hash, repository commit, portal version,
  output hashes, and bounded line-range snippet hashes in evidence.
- Build only from one clean, full Git commit. Read configuration, source
  Markdown, optional primitive tokens, and referenced local media from bounded
  Git blobs at that commit. Compare the adopted runtime and every configured
  worktree input (configuration, source Markdown, optional primitive tokens,
  and referenced media) byte-for-byte with those blobs; dirty, staged, deleted,
  untracked, ignored, inaccessible, masked (`assume-unchanged` or
  `skip-worktree`), or changing inputs block publication. Read worktree files
  through bounded, no-follow handles and order evidence with the starter's
  locale-independent comparator. Never label worktree bytes with `HEAD`.
- Resolve relationships from declared frontmatter and strict stable IDs. Inline
  ID mentions may become links, but never invent a declared dependency. An ID
  whose record is not a portal source links to the repository file, or to the
  pointer page for its folder; it is never a dangling link and never a page.
- Depth-2 `Concept` / `Architecture` / `Technical` sections are the altitude
  grammar: the adapter renders the trio as a tablist with one visible panel in
  the derived page only. Sources stay plain Markdown; raw source HTML stays
  escaped.
- A leading depth-1 heading that repeats the page title, exactly or with
  only a record-ID prefix such as `ADR-0001:`, renders once (the shell
  already shows the title); any other heading is author content.
- A `cf-stage` fence is generated-figure input: bounded node/stage/caption
  grammar, every text field escaped, roles whitelisted. An invalid figure
  produces the bounded error page, never partial or unescaped output.
- A figure declaration bound in `portal.config.json` is generated-figure input
  under the same pins as a source: the build refuses a declaration that fails
  the grammar, a fact its source does not give, or a binding to a missing
  route or anchor. An illustrated or pass-through source keeps its bytes; the
  page and its Markdown twin attribute companion figures to the declaration.
- Treat an explicit ID or relationship field as authority: a wrong type,
  malformed ID, invalid target, or duplicate key produces the bounded error
  page. Infer a supported ID from the filename only when `id` is absent.
- Treat ADR amendment headings as current history. Do not flatten a later Note,
  Update, or Correction into the original decision date.

## Identity and links

Adoption state and emitted evidence are separate formats. Evidence remains
schema v1. While managed, its generator name is `@codeflow/docs-portal` and its
version equals both the declared generator version and installed starter
version. After transfer, the declared generator identity describes the current
project generator; the starter version and file hashes describe only the
transferred-from release. A genuinely renamed/versioned fork updates its actual
generator identity and the declaration together. Evidence must match that
declaration, not frozen provenance. Names/versions are nonempty and bounded to
128 UTF-8 bytes; unknown fields and unsupported schemas still fail closed.
Passing the read-only verifier proves checked claims match bytes, not generator
attestation, visual quality, security, accessibility or browser behavior.

Accept only project-supported strict IDs such as `CAP-001`, `ADR-001`,
`EPC-001`, `SPC-001`, and `TSK-001`. Derive each route from its semantic layer
plus its NFC, portable path relative to the most-specific configured source
root; physical root moves therefore need not leak into reader-facing URLs.
That exact text is the content slug, generated-file identity, and evidence
route. Percent-encode each segment only when producing a URL. Never compare
case-folded build paths: case folding is collision detection, not identity.
Routes must be unique and deterministic. A pinned generator upgrade must pass
the real-build route fixture for mixed case, spaces, punctuation, and Unicode,
not only a string-transformation unit test.
Every relationship target must exist. Derived backlinks must be the exact
inverse of declared forward relationships.
Repository-relative document links resolve through the source-to-route graph.
Local media is copied only from committed, bounded PNG, JPEG, GIF, or WebP
blobs whose signatures, headers, dimensions, and aggregate pixel/byte budgets
are verified and recorded in evidence. Remote images, active
SVG/PDF copies, traversal, unsupported schemes, and broken targets fail closed;
ordinary HTTPS and mail links remain links and are never fetched.
For authors: on the portal and in present a figure is inline SVG through the
figure block, in a README it is the ASCII chat form, and in a chat reply the
surface rule in `cf-method/references/workflow-lifecycle.md` picks the form.
Pin source links to the evidenced commit on known GitHub, GitLab, and Bitbucket
HTTPS repository URLs. A committed document excluded from the portal remains a
pinned provider file link. A relative link to a committed directory uses the
provider's pinned tree route, with or without a trailing slash; directory
identity comes only from ancestors in the committed inventory, never from the
working tree. Existing query and fragment suffixes are preserved. For another
provider, show a visible source path and commit without manufacturing a route.
Directory resolution is link-only and follows exact-file resolution: images
targeting directories fail, and symlinks or submodules are never treated as
ordinary directories. Existing file-link behavior is unchanged. Similarly
prefixed paths and genuinely absent targets still fail.

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
cannot survive as reachable output. Other committed project-owned public files
are runtime inputs: inventory their regular Git blobs, compare worktree bytes,
and republish the committed bytes. Untracked active public content, symlinks,
masked edits, and changing files block publication rather than being copied.

## Rendering

Parse GFM through the pinned Markdown syntax tree. Escape raw HTML outside code
nodes with no version-1 bypass; keep code, comments, references, headings, and
visible prose structurally distinct. Neutralize configuration, frontmatter,
diagnostic, and relationship text before reinserting it into generated
Markdown. Strict-ID mentions remain normal links with source-grounded
contextual previews. Use source-backed diagrams or explanations only when they
reduce cognitive load; do not manufacture facts to fill a layout. A missing
content layer may be collapsed honestly.
