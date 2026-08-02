# Portal operations

## Ownership

`portal.config.json` is user-owned and written once. Starter implementation,
lockfile, tests, and styles are managed with Git-tracked, opaque,
content-addressed pristine baselines. On an
upstream change, an untouched file is replaced; an edited file receives a
three-way merge; a conflict keeps the original and writes a collision-safe,
content-addressed `<path>.codeflow-<hash>.new` sidecar without overwriting an
existing path.
Commit `.codeflow/docs-portal.json` and its opaque
`.codeflow/.docs-portal-baseline/` blobs with the adopted starter. They are
team-portable reconciliation state, not a private runtime cache; their
content-addressed names prevent a second browsable lockfile/dependency surface.

## Dependency discipline

Use the committed `.node-version`, lockfile, and `npm ci`. Review install scripts before relaxing
an `--ignore-scripts` install; native packages may require their pinned scripts,
so an unavailable build is better than an unrecorded exception. Run `npm audit`
and the repository's dependency scanner. Rehearse upgrades in a disposable
worktree or directory, inspect transitive and runtime changes, and change exact
pins plus the lockfile together.

## Release evidence

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
tracked `browser:verify` harness uses separate temporary profiles and an
ephemeral loopback port, emits bounded evidence under
`.portal/browser-evidence/<run>/`, and removes profiles and server resources.

Portal-owned Markdown fragments are verified against renderer-produced heading
anchors in both the producer and the independent Rust validator. Fragments on
repository files that are not published into the portal remain under the
linked source host's authority and are not claimed as portal-verified anchors.

Repository reads use a bounded commit inventory plus batched blob reads rather
than one process per source. Git receives only an explicit process-environment
allowlist; model, cloud, credential, loader-injection, and Git configuration
variables are never inherited. It runs without fsmonitor, prompts, lazy
fetching, replacement objects, optional locks, pagers, or inherited
repository/config redirection. Missing local objects, output overflow, and
timeouts fail closed. Non-reserved public files are preserved only when they
are committed regular runtime inputs whose worktree bytes match the snapshot.
Publication writes the committed blobs; untracked active content, symlinks,
masked edits, or a file that changes identity during verification aborts.
