# CAP-015: opt-in-documentation-portal

<!-- WHAT layer, graduated from docs/capabilities.md. The registry keeps the
     machine-read yaml block, a summary, and a link here; this file holds the
     full prose. Update both in the PR that ships the work. -->

`codeflow portal setup --path <repository-relative-directory>` explicitly
adopts the exact-pinned Starlight and Pagefind repository-guide utility. The
portal build requires Node 22.19.0 or newer. The aggregate CI gate runs on Node
26.4.0, the presentation renderer's pin, and its full strict target installs, checks, builds, and validates the
dogfood portal; the portal-local `.node-version` and the Windows adapter-test
lane pin Node 24.18.0.
The starter is absent from ordinary initialization, materializes offline once at
the selected root, preserves user-owned configuration, and participates in
replace-only updates without pristine runtime copies or source merges. Runtime
drift and collisions stop all portal writes; missing project-owned configuration
remains absent. `codeflow portal transfer --confirm` preserves edits and
intentional deletions while handing ongoing runtime maintenance to the project.
V2 adoption state freezes transferred-from provenance and declares the current
generator identity; v1 evidence stays strict in both ownership modes. Legacy
journals recover before migration, and unknown or changed baseline content
blocks both migration and transfer without deletion. The source-authority adapter generates disposable pages, Markdown
twins, `llms.txt`, search output, and a versioned evidence manifest from one
clean committed snapshot. Its pinned GFM pipeline, bounded no-follow reads,
literal bounded Git pathspec batches, committed-blob authority,
configured-tree source coverage, semantic source-root-relative routes,
reserved generated-public namespaces, locale-independent ordering, workflow
lease, and recoverable publication transaction fail closed before mixed or
active content can be claimed. Index flags cannot hide changed runtime,
configuration, source, token, or media bytes. The locked installer verifies the
exact lifecycle-script inventory and disables dependency scripts. Install,
build, preview, browser, and Git subprocesses receive only a small non-secret
environment allowlist; inherited provider, cloud, package-registry credential,
and loader variables never cross the boundary, while Git also rejects inherited
configuration. A broken current Markdown blob yields only a
bounded, visible, non-searchable current-source error page; Git history and
previous generated data are never republished.
`codeflow validate --portal <directory>` executes no project code and writes
nothing; it independently checks bounded path, hash, complete configured-source
coverage, exact source-derived identity/relationship, error-page, provenance, version, raster-dimension,
output-coverage, twin, and
`llms.txt` claims.
It compares generator evidence with the declared identity, retaining managed
release pins but accepting a genuinely renamed transferred generator. Matching
evidence is not runtime attestation or a substitute for rendered qualification.

The repository-owned full gate runs the complete locked JavaScript authority
suite, a real Starlight build, and the Rust verifier locally and on Ubuntu; the
Windows lane runs the same JavaScript authority/path suite. Shared fixtures pin
configuration, strict-frontmatter, 40/64-character Git object ID, exact
case-sensitive route, and URL-boundary behavior across the producer and
verifier. These dogfood gates do not leak a Node requirement into the generic
consumer CI scaffold: adopted consumer portals opt into their project test
configuration.

The mirrored `cf-docs-portal` skill owns proportional adoption, layered
information design, safe source interpretation, exact dependency operations,
browser/accessibility evidence, and cleanup. It applies the same utility
presentation craft as `cf-present` (tokens, altitude, stage grammar) to
durable source-linked docs for CodeFlow or any consuming project, without a
session Comment lifecycle. The repository dogfoods the starter under
`docs-portal/`; a generated local site is evidence and never an implicit
publish action.
