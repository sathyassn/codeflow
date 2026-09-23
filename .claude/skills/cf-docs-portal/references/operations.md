# Portal operations

## Ownership

Commit `.codeflow/docs-portal.json` with the adopted runtime. Schema v2 records
the root, release, file hashes, whole-runtime ownership and declared generator
identity. Fresh setup and successful legacy migration keep no pristine runtime
copies. General scaffold reconciliation is unchanged.

| Path or state | Setup/update behavior |
|---|---|
| Unchanged managed file | Replace with the incoming release |
| Missing managed file | Repair while managed |
| Bytes already equal to incoming | Accept without treating them as drift |
| Edited managed file, unknown incoming collision, edited retirement | Stop all portal writes and state advancement; preserve edits |
| Unchanged retiring managed file | Remove in the coherent update transaction |
| Project-owned configuration | Seed only at first adoption; preserve thereafter, including absence |
| Additional assets at unclaimed paths | Remain project-owned |
| Transferred runtime | Preserve all files and intentional deletions; no upstream reconciliation |

Bundled scripts, styles, fonts, licenses and favicon remain managed regardless
of directory name. There is no automatic source merge, lockfile merge or new
conflict sidecar. A portal conflict does not roll back unrelated changes already
made by the enclosing `codeflow update`.

Use supported config/token settings or rehome assets at unclaimed paths when
those seams fit. Otherwise preserve the current work and choose explicitly:
restore reviewed managed bytes, or take responsibility for the whole runtime:

```sh
codeflow portal transfer --confirm
```

Transfer operates only on the adopted root; it has no per-file or path selector.
On v1 or v2 it changes ownership/state, not runtime bytes: it accepts existing
edits and deletions without installing a newer embedded release or repairing
missing configuration. It freezes the previously adopted version/hashes as
transferred-from provenance. Repeating transfer is a no-op even after further
edits. No future setup/update resets ownership. A reviewed restoration of the
complete earlier runtime and state from Git is a separate operation, with
current work preserved; transfer is durable, not cryptographically irreversible.

## Supported customization

The portal and `cf-present` share design principles, not interchangeable import
schemas. In the adopted portal's project-owned `portal.config.json`,
`primitive_tokens` is either `null` or a **repository-relative path string**,
not an object with a `path` field. For a portal adopted at `guide/`, merge this
field into the existing configuration:

```json
{"primitive_tokens": "guide/primitive-tokens.json"}
```

The referenced project-owned file supplies only an accent for each appearance:

```json
{
  "schema_version": 1,
  "light": {"accent": "#005f56"},
  "dark": {"accent": "#72e2cf"}
}
```

These illustrative colors pass both current utility themes; choose the actual
accent for the repository and verify both modes. Each opaque six-digit color
must meet 4.5:1 contrast against every reader-selectable skin's surface and
selected background. The portal import does not
accept `cf-present`'s `colors`, typography or identity fields. Do not infer a
configuration shape from another utility or from a partial installation.

For an additional logo, use an unclaimed path such as
`guide/public/brand/logo.png`. To display it in `docs/product.md`, use a path
relative to that source document:

```markdown
![Repository logo](../guide/public/brand/logo.png)
```

The adapter imports committed PNG/JPEG/GIF/WebP images into derived media routes.
Root-relative Markdown URLs such as `/brand/logo.png` and inline SVG image imports
are not supported. Adding a public asset does not insert it into portal chrome.
Do not replace a bundled favicon or use the generated `public/media/`,
`public/markdown/` or `public/llms.txt` namespaces. Commit the source edit,
configuration, token file and public asset before building; publication verifies
their Git bytes. These changes need neither managed runtime edits nor ownership
transfer.

## Legacy recovery and migration

Run recovery through the normal portal command before interpreting or repairing
legacy state. The engine holds its lease and recovers a legitimate pending
journal before full adoption-state parsing and migration. Do not delete or edit
journals, staged output or lease records to force progress. If recovery cannot
authenticate its inputs, preserve the repository and report the actual failure.

Distinguish runtime drift from baseline integrity:

- **Runtime drift:** ordinary setup/update stops before portal writes. Review
  the supported customization, explicit restoration or confirmed-transfer
  choices above. Transfer preserves the changed runtime.
- **Legacy baseline integrity:** an unknown, changed, oversized, non-regular or
  unsafe entry stops the new migration or transfer before publication. Legitimate
  previously journaled recovery may already have completed; report that
  separately from the failed new operation. Inspect preserved content and
  repository history first.
  After journal recovery is settled, an operator may explicitly preserve and
  relocate the questionable baseline directory outside its reserved location,
  or restore exact reviewed bytes. Retain a verified copy and a restore path;
  do not prescribe deletion or line-ending normalization as a repair.

Then retry the intended operation. An absent baseline directory or expected blob
is benign because nothing remains there to authenticate or delete; it does not
waive ordinary migration's runtime-drift checks. Existing blobs are removed only
when bounded regular-file bytes match a recorded managed content hash, within
the successful transaction. Empty reserved directories are removed without
recursive traversal. Failed commit rolls back or retains valid recovery
evidence; unknown content is never recursively cleaned.

Filesystem safety rejects symlink/reparse traversal and ancestor redirection
through held directories. It is not inode compare-and-swap, hard-link/Unix-mount
byte-origin isolation or isolation from a hostile same-user process. Windows
directory handles temporarily deny delete sharing; native platform behavior
needs native execution evidence, not an inference from cross-target lint.

## Dependency discipline

Use the committed `.node-version`, lockfile, and `npm run deps:install`. The
managed installer first proves that every lockfile package declaring a lifecycle
script matches the exact reviewed-and-ignored inventory, then runs `npm ci
--ignore-scripts` with a minimal non-secret environment. The current starter has
no lifecycle-script exception. If a future pinned dependency cannot operate
without one, stop: review the exact package/version and script source, record why
the behavior is required, implement the narrow exception in the managed
installer under the same environment boundary, and add an executable canary.
Never use a manual `npm rebuild` or plain `npm ci` as an undocumented bypass; an
unavailable build is safer than an unreviewed exception. Run `npm audit` and the
repository's dependency scanner. Rehearse upgrades in a disposable worktree or
directory, inspect transitive and runtime changes, and change exact pins,
lockfile, reviewed inventory, and evidence together.

## Release evidence

### Experience qualification

Map checks to accepted intent and affected journeys. At minimum for a material
portal change, verify:

- locked build, deterministic adapter tests and the Rust evidence verifier;
- navigation, search, source links, Markdown twins, `llms.txt`, error handling,
  strict-ID previews (hover, focus, touch, keyboard, Escape and ordinary link
  navigation), and empty/tiny/monorepo fixtures as applicable;
- keyboard order, focus visibility, semantics, contrast, target size, zoom,
  reduced motion and responsive behavior against WCAG 2.2 AA;
- both fallback themes in light/dark, persisted preferences and no incorrect-mode
  flash; Chromium, Firefox and WebKit journeys when available;
- task-owned browser state, ports, test data, traces and screenshots, followed
  by verified resource cleanup. Unavailable platform evidence is not inferred.

`npm run browser:verify` uses a task-owned loopback preview under the workflow
lease. Routes, journeys and search terms come from validated configuration and
generated evidence. Chromium, Firefox and WebKit run sequentially with separate
temporary profiles; WCAG 2.2 AA axe rules supplement the journeys, console and
remote-request checks. Bounded hashed screenshots/traces and server/profile
teardown are recorded. Set a unique `PORTAL_BROWSER_RUN` for concurrent tasks.
Use a headed task-owned browser only for a finding the headless run cannot
settle, never the operator's profile or view. Review before and after a material
ownership migration; successful transfer alone says nothing about rendering.

### Recorded results

Record:

- exact Node/npm and package versions;
- clean locked install, tests, production build, and evidence validation;
- source, route, search, Markdown-twin, `llms.txt`, provenance, and version
  negative cases;
- dependency and secret scans;
- browser engine, viewport, appearance mode, accessibility, console, network,
  screenshot, and trace evidence applicable to the claim;
- unpacked and archive-equivalent starter bytes and CodeFlow release-binary
  delta against the recorded baseline;
- task-owned server, browser, temporary directory, and process teardown.

Do not publish generated output when source validation or evidence verification
fails. Do not treat an automated accessibility score as proof of complete
conformance or an agent review as a deterministic gate.

When `.codeflow/docs-portal.json` exists, a material change to authoritative
docs, relationships, repository/release version, portal configuration, or
starter behavior requires the locked check and build plus `codeflow validate
--portal <adopted-root>` before ship. Add rendered/browser review in proportion
to navigation, preview, search, theme, responsive, or accessibility impact.
This lifecycle gate applies only to adopters.

The adapter serializes generated-corpus and evidence writes with an expiring
single-writer lease, publishes complete staged directory sets, and recovers its
write-ahead journal before another writer proceeds. Same-filesystem file and
directory replacement failures retain or restore the prior corpus. POSIX builds
also sync changed directories; native Windows cannot portably make that
directory-sync guarantee, so power-loss durability there remains a release
canary claim rather than an inference from Unix tests.

The locked `check`, `build`, `dev`, and `preview` scripts hold one top-level
workflow lease, so another process cannot bind `dist` to the wrong snapshot.
Before Astro starts, the workflow rejects symlinks and non-regular entries in
tool-controlled `dist`, `.astro`, and Node cache roots; it never follows an
output link while cleaning. Run only one task-owned dev server per portal, stop
it before another locked workflow, and verify process and port teardown. The
tracked `browser:verify` harness holds the same workflow lease, refuses a stale
or mismatched generated manifest, and records the exact repository commit,
configuration, generator, evidence, and artifact-claim identity it exercised.
It derives base paths and layer journeys from project configuration, uses
separate temporary profiles and an ephemeral loopback port, emits bounded
success and failure evidence under `.portal/browser-evidence/<run>/`, and
removes profiles and server resources.

When you tell the operator where a dev or preview server runs, report the
exact served URL from the tool output; never guess the host or port.

Portal-owned Markdown fragments are verified against renderer-produced heading
anchors in both the producer and the independent Rust validator. Fragments on
repository files that are not published into the portal remain under the
linked source host's authority and are not claimed as portal-verified anchors.

Repository reads use a bounded commit inventory plus batched blob reads rather
than one process per source. Install, build, preview, browser, and Git children
receive only an explicit process-environment allowlist; model, provider, cloud,
package-registry credential, and loader-injection variables are never inherited.
Git additionally runs without inherited configuration, fsmonitor, prompts, lazy
fetching, replacement objects, optional locks, or pagers. Missing local objects,
output overflow, and timeouts fail closed. Non-reserved public files are
preserved only when they are committed regular runtime inputs whose worktree
bytes match the snapshot. Publication writes the committed blobs; untracked
active content, symlinks, masked edits, or a file that changes identity during
verification aborts.
