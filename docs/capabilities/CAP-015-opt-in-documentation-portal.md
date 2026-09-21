# Opt-in documentation portal

<!-- WHAT layer, graduated from docs/capabilities.md. The registry keeps the
     machine-read yaml block, a summary, and a link here; this file holds the
     full contract for CAP-015. Update both in the PR that ships the work. -->

## Concept

**The portal is derived: one clean committed snapshot in, disposable output
out, and the Markdown stays the only authority.**

```cf-stage
committed snapshot | one clean commit, never working-tree bytes @accent
->
source-authority adapter | pages · twins · llms.txt · search · evidence
->
starlight build | static output, disposable
->
codeflow validate --portal | re-derives the byte claims @positive
caption: local generation is evidence; it never publishes a site
```

`codeflow portal setup --path <repository-relative-directory>` explicitly
adopts the exact-pinned Starlight and Pagefind repository-guide utility. The
adapter is the sole author of every generated file, so nothing downstream of
the snapshot can become a second place to record a fact. Architecture is the
ownership lifecycle; Technical is the adapter's fail-closed boundary and the
gates that prove it.

## Architecture

Adoption and transfer are the two explicit moves, and a repository sits in
exactly one of three states:

```cf-stage
not adopted | no portal workspace, lockfile or baseline @accent
->
managed | setup and update reconcile the runtime by release
->
transferred | the project owns runtime upgrades and dependencies @warn
caption: both moves are deliberate; neither happens to clear a conflict
```

The starter is absent from ordinary initialization, materializes offline once at
the selected root, preserves user-owned configuration, and participates in
replace-only updates without pristine runtime copies or source merges. Runtime
drift and collisions stop all portal writes; missing project-owned configuration
remains absent. `codeflow portal transfer --confirm` preserves edits and
intentional deletions while handing ongoing runtime maintenance to the project.
V2 adoption state freezes transferred-from provenance and declares the current
generator identity; v1 evidence stays strict in both ownership modes. Legacy
journals recover before migration, and unknown or changed baseline content
blocks both migration and transfer without deletion.

## Technical

The sections below are what an adopting repository has to honor: the Node
version each lane pins, the boundaries the adapter refuses to cross, how the
build is verified independently of the adapter, and the skill that owns the
craft.

### Node lanes

Each lane names the Node version it runs on, and only the starter is a floor.

| Lane | Node |
|---|---|
| Starter floor | 22.19.0 or newer |
| Aggregate CI gate | 26.4.0, the presentation renderer's pin; its full strict target installs, checks, builds, and validates the dogfood portal |
| Portal-local `.node-version` and the Windows adapter-test lane | 24.18.0 |

### Adapter authority and fail-closed boundaries

The source-authority adapter generates disposable pages, Markdown
twins, `llms.txt`, search output, and a versioned evidence manifest from one
clean committed snapshot. Its pinned GitHub Flavored Markdown (GFM) pipeline,
bounded no-follow reads,
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

### Independent verification

`codeflow validate --portal <directory>` executes no project code and writes
nothing. It independently checks eleven claims:

| Claim | What it proves |
|---|---|
| Bounded path | Every output path stays inside the portal workspace |
| Hash | Each published byte matches its recorded digest |
| Complete configured-source coverage | Every configured source produced a page |
| Exact source-derived identity | Each page's ids come from its own source |
| Relationship | Each declared relationship resolves to a real target |
| Error page | A broken source rendered the bounded error page, not silence |
| Provenance | Each page carries its visible source, commit or release |
| Version | The evidence schema is the one this binary reads |
| Raster dimension | Each raster asset matches its declared size |
| Output coverage | Nothing was published that the manifest does not list |
| Twin and `llms.txt` | The Markdown twins and the index agree with the pages |

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

### The skill that owns the craft

The mirrored `cf-docs-portal` skill owns proportional adoption, layered
information design, safe source interpretation, exact dependency operations,
browser/accessibility evidence, and cleanup. It applies the same utility
presentation craft as `cf-present` (tokens, altitude, stage grammar) to
durable source-linked docs for CodeFlow or any consuming project, without a
session Comment lifecycle. The repository dogfoods the starter under
`docs-portal/`; a generated local site is evidence and never an implicit
publish action.
