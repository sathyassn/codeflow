---
id: ADR-0048
title: isolate portal adoption and verify derived evidence
date: 2026-08-01
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — an opt-in managed portal starter is embedded once outside the mirrored skills, while validate gains a read-only verifier for its derived evidence manifest
---

# ADR-0048 — isolate portal adoption and verify derived evidence

## Context

SPC-005 requires an offline, exact-pinned Astro Starlight and Pagefind starter,
managed updates, and deterministic portal checks through `codeflow validate`.
Putting the starter under `cf-docs-portal` would mirror its workspace and
lockfile into both harness skill trees in every standard and full project. That
would charge non-adopters twice in repository bytes and make their dependency
scanner assess a Node toolchain they did not select. Letting `validate` execute
an installed project-owned script would instead make a binary-owned gate depend
on mutable project code and an available Node runtime.

## Decision

Ship only the concise `cf-docs-portal` workflow and references through the
normal cross-harness skill mirrors. Embed one starter source under
`assets/docs-portal/`, outside the scaffold manifest and skill trees. An
explicit `codeflow portal setup --path <repository-relative-directory>` adopts
it without network access, records the chosen root and starter version, and
installs a managed copy with Git-tracked, opaque, content-addressed pristine
baselines. Repeated setup and
ordinary `codeflow update` reconcile an adopted starter by the existing
never-clobber contract: replace pristine files, three-way merge edited managed
files, write conflicts beside the original, preserve user-owned configuration,
and report every outcome. Paths outside the repository, symlink escapes,
reserved CodeFlow/Git state, and a different root after adoption fail closed.
Non-adopters receive no portal workspace or lockfile on disk.

The exact-pinned Node adapter remains the single author of the derived portal
graph. It reads authoritative content from bounded Git blobs at one full
commit, compares the adopted runtime and configuration with that commit through
bounded, no-follow file handles, parses GFM structurally, refuses active or
remote source content, and publishes generated directories through a
recoverable single-writer transaction. Git pathspecs are top-anchored and
literal, and blob comparison defeats index flags that hide worktree changes.
The adapter inventories the commit once and reads source/media objects in
bounded batches with prompts, lazy fetching, fsmonitor, pagers, optional locks,
replacement objects, and inherited Git redirection disabled. Status pathspecs
are split beneath a conservative native-Windows command limit. Git subprocesses
receive only a small non-secret process-environment allowlist; model, cloud,
credential, loader-injection, and Git configuration variables are excluded. If
one current source cannot parse, its
route contains only a small visible error page for that current blob. It has no
active identities or relationships and is absent from search, previews, and
normal current-content indexes. The adapter never walks history or republishes
ancestor or prior generated content. A workflow lease spans adapter, renderer,
and final evidence collection so a concurrent build cannot bind output to a
different snapshot.
Ordering is defined by one locale-independent comparator. The adapter emits a
versioned evidence manifest beside disposable build data. The new
`codeflow validate --portal <portal-root>` path performs no writes and executes
no project code. Rust validates only claims that can be checked independently
against repository and output bytes: schema and size limits, safe relative
paths, source and line-range snippet hashes, existing strict IDs, unique
routes, exact configured-tree source coverage, source-derived identities and
forward relationships, derived
backlink consistency, error-page exclusion, pinned commit/version/source
metadata, bounded raster headers/dimensions, covered output pages, per-page
Markdown twins, and `llms.txt`. Rendering
and graph-generation semantics remain in the exact-pinned Node build and test
lanes; Rust does not implement a second content graph.

Routes combine the selected semantic layer with the exact NFC portable source
path relative to the most-specific configured source root. That text remains
the content slug and evidence/artifact identity; URL boundaries percent-encode
each segment. Case folding detects collisions but never defines identity.
Generator upgrades requalify this seam with a real mixed-case, space,
punctuation, and Unicode build. Generated public Markdown twins, media, and
`llms.txt` occupy reserved namespaces that are fully replaced on publication.
Committed user-owned public files outside them remain preserved from their Git
blobs after byte equality is checked; untracked active files, symlinks, masked
edits, and changing inputs fail closed.

The starter source is capped at 2 MiB unpacked, its archive-equivalent content
at 1 MiB, and its measured release-binary increase at 1 MiB. `SKILL.md` remains
within the existing 300-line skill limit. The macOS arm64 release baseline at
commit `88215cc977039ed405b8875a38ae00cd61f614c5` is 10,473,952 bytes; the final
task implementation is 11,161,104 bytes, a 687,152-byte increase. The starter
is 459,410 tracked bytes and 103,155 archive-equivalent bytes. Release builds
use `rust-embed`'s upstream compression support so the offline starter does not
consume its unpacked size in every binary; debug builds retain the existing
disk-backed development behavior. The compression dependency set remains
subject to the normal locked supply-chain audit.

## Consequences

Portal adoption stays explicit, offline, versioned, and upgradeable without
making Node part of CodeFlow's default repository surface. The binary gains one
deterministic materializer and one bounded read-only manifest verifier; it does
not gain a model-powered generator, runtime server, arbitrary command registry,
or second documentation authority. The evidence manifest is a public derived
format and therefore needs schema tests, path and byte caps, and compatible
version handling. An adopted starter can carry project edits, so upstream
changes may require a reported three-way merge or conflict instead of silent
replacement. Compressed embedding adds bounded decompression work when an
embedded asset is read. This is paid during explicit scaffold/portal
materialization rather than normal repository operations and preserves the
single-binary, offline contract within the measured size budget.

The Rust verifier is intentionally narrower than the Node adapter. A green
manifest check proves that emitted claims match bytes and repository IDs; it
does not prove design fidelity, complete accessibility, browser behavior,
Pagefind semantics, or source interpretation. Those claims remain separate
build, browser, accessibility, and cross-model evidence.

## Rejected

- Install the starter under the mirrored skill: duplicates a lockfile-bearing
  workspace and exposes non-adopters to irrelevant dependency findings.
- Run the installed Node validator from `codeflow validate`: executes mutable
  project code inside a binary-owned validation surface and makes Node a gate
  prerequisite.
- Register the validator only under `codeflow test`: useful as an additional
  project test, but it does not satisfy SPC-005's explicit validate boundary.
- Add a general feature/plugin materializer: one adopted bundle does not justify
  a provider framework or arbitrary executable registry.

## Architecture impact

The scaffold area gains a single-instance, opt-in managed bundle backed by an
embedded non-scaffold asset source. The engine validation area gains a bounded
portal evidence-manifest verifier; the generated graph and frontend stay in the
isolated Node utility.

## Update — 2026-08-01: current-source failure boundary

The task-level Claude and Codex architecture/security checkpoint rejected
automatic ancestor-page recovery. Although an ancestor can be authenticated,
republishing it can disclose content deliberately removed later, preserve links
that no longer resolve, and make a current route look semantically usable when
its current source is broken. Plan v3 therefore adopts the bounded current-source
error-page behavior above. This changes an unshipped `building` capability; no
legacy evidence compatibility path is retained.
