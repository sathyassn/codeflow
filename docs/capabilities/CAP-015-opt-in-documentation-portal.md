# Opt-in documentation portal

<!-- WHAT layer, graduated from docs/capabilities.md. The registry keeps the
     machine-read yaml block, a summary, and a link here; this file holds the
     full contract for CAP-015. Update both in the PR that ships the work. -->

## Concept

**The portal is derived: one clean committed snapshot in, disposable output
out, and the Markdown stays the only authority.**

`codeflow portal setup --path <repository-relative-directory>` explicitly
adopts the exact-pinned Starlight and Pagefind repository-guide utility. The
adapter is the sole author of every generated file, so nothing downstream of
the snapshot can become a second place to record a fact. Decision and work
records stay folders the guide points to, and a local build never publishes a
site.

## Architecture

The adapter turns one committed snapshot into pages, Markdown twins,
`llms.txt`, search output and a versioned evidence manifest. Two checks prove
the result without trusting the adapter: the Rust validator re-derives the
byte claims, and the browser gate measures the rendered pages.

The starter is absent from ordinary initialization, materializes offline once at
the selected root, preserves user-owned configuration, and participates in
replace-only updates without pristine runtime copies, sidecars or source merges. Runtime
drift and collisions stop all portal writes; missing project-owned configuration
remains absent.

## Technical

What an adopting repository has to honor: the ownership states, the Node
version each lane pins, the adapter's boundaries, and the independent checks.

### Ownership states

A repository's portal runtime is in exactly one ownership state, and both
moves between states are deliberate.

| State | What setup and update do |
|---|---|
| Not adopted | nothing; there is no portal workspace, lockfile or baseline |
| Managed | setup and update reconcile the runtime by release; unchanged managed files are replaced and missing ones repaired |
| Managed, locally edited | local runtime edits or unknown collisions stop all portal writes |
| Transferred | `codeflow portal transfer --confirm` preserved current edits and deletions; setup and update no longer reconcile the runtime, and the project owns runtime upgrades, dependencies and compatible generator evidence |

Never transfer automatically to clear a conflict. Adoption state freezes
transferred-from provenance and declares the current generator identity, and
evidence stays strict in both ownership modes. A pending journal from an
earlier portal version recovers before migration, and unknown or changed
baseline content blocks both migration and transfer without deletion. The
installed skill's operations reference and the
[portal ownership migration](../releasing.md#portal-ownership-migration) steps
cover recovery and baseline-integrity failures.

### Adopt and build the portal

Standard and full tiers include the concise `cf-docs-portal` workflow, but no
Node workspace or lockfile. Adopt the utility only when layered navigation,
search, source links, and machine-readable documentation twins justify it.

1. Run `codeflow portal setup --path docs-portal`. Setup is offline and
   repository-relative, and it records one adopted root.
2. In `docs-portal`, run `npm run deps:install`. The managed installer checks
   the lockfile's lifecycle-script inventory, keeps those scripts disabled, and
   passes only a small non-secret environment to npm. Do not substitute plain
   `npm ci`; a required future lifecycle exception belongs in the reviewed
   managed installer with an executable canary.
3. Run `npm run check` for the adapter tests, derivation and Astro check.
4. Run `npm run build` to derive pages and evidence and build the site.
5. Optionally run `npm run preview` to serve `dist/` on loopback until you
   stop it.
6. From the repository root, run `codeflow validate --portal docs-portal`. A
   clean report means every byte claim was re-derived.

`check`, `build`, `dev`, and `preview` share one workflow lease: run one at a
time and stop the preview before the next build. `npm run dev` derives the
pages once and serves them through Astro's dev server for authoring; it reads
the same committed snapshot. `npm run browser:verify` runs the isolated
headless journey matrix when Playwright browsers are installed.

The adapter reads only committed bytes and refuses a snapshot whose portal
runtime, configuration, or configured source roots differ from `HEAD`,
including untracked files under those roots, so commit source edits before
building.

`portal.config.json` is project-owned: after first adoption, even its absence
is preserved and reported. Configure source roots there rather than copying
authoritative prose into the portal. Its `records.enabled` key names who owns
the record folders, not whether they are visible: left `false`, the configured
folders stay out of the page set and one generated pointer page names them;
set `true`, the project publishes those folders itself as ordinary pages and
declares no pointers. Use supported config and token settings and unclaimed
asset paths for ordinary customization; bundled public assets remain managed.

### This repository's own guide

This repository dogfoods the starter at `docs-portal/`: a managed runtime, a
project-owned `portal.config.json`, and the Graphite skin (configured by its
older name, `signal`). The guide is a derived view of the Markdown under
`docs/` and `project-management/`; every generated page, Markdown twin, search
index, and `llms.txt` is disposable output that Git ignores. There is no hosted
site, and GitHub shows the Markdown sources, not the generated HTML.

The home page names the exact repository commit the guide was built from. A
`release_version` value renders as a release label, so it stays `null` unless
a verified published release exists for the built commit; the
[public version baseline](../releasing.md#public-version-baseline) explains
which release that is.

### Node lanes

Each lane names the Node version it runs on, and only the starter is a floor.

| Lane | Node |
|---|---|
| Starter floor | 22.19.0 or newer |
| Portal full strict target and the Windows adapter-test lane | 24.18.0, pinned by `docs-portal/.node-version`; the full strict target installs, checks, builds, and validates the dogfood portal |
| Presentation renderer target | 26.4.0, pinned by `crates/codeflow-present/web/.node-version` |

Each Node target runs on the version its own `.node-version` pins, locally and
in CI. `scripts/with-node.py` selects each version for its target, and
`gate-parity` holds the CI pins to those files.

### Adapter authority and fail-closed boundaries

The source-authority adapter generates every output from one clean committed
snapshot. These boundaries fail closed before mixed or active content can be
claimed:

- a pinned GitHub Flavored Markdown (GFM) pipeline;
- bounded no-follow reads and literal bounded Git pathspec batches;
- committed-blob authority and configured-tree source coverage;
- semantic source-root-relative routes and reserved generated-public
  namespaces;
- locale-independent ordering;
- a workflow lease and a recoverable publication transaction.

Index flags cannot hide changed runtime, configuration, source, token, or
media bytes.

The locked installer verifies the exact lifecycle-script inventory and
disables dependency scripts.

Install, build, preview, browser, and Git subprocesses receive only a small
non-secret environment allowlist; inherited provider, cloud, package-registry
credential, and loader variables never cross the boundary, while Git also
rejects inherited configuration.

A broken current Markdown blob yields only a bounded, visible, non-searchable
current-source error page; Git history and previous generated data are never
republished.

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
presentation craft as `cf-present` (tokens, altitude, figure grammar) to
durable source-linked docs for CodeFlow or any consuming project, without a
session Comment lifecycle. The design-exploration board that settled the craft
is a reference, not a page to clone.
