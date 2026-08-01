# Portal operations

## Ownership

`portal.config.json` is user-owned and written once. Starter implementation,
lockfile, tests, and styles are managed with Git-tracked, opaque,
content-addressed pristine baselines. On an
upstream change, an untouched file is replaced; an edited file receives a
three-way merge; a conflict keeps the original and writes `<path>.new`.
Commit `.codeflow/docs-portal.json` and its opaque
`.codeflow/.docs-portal-baseline/` blobs with the adopted starter. They are
team-portable reconciliation state, not a private runtime cache; their
content-addressed names prevent a second browsable lockfile/dependency surface.

## Dependency discipline

Use the committed lockfile and `npm ci`. Review install scripts before relaxing
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

The adapter serializes generated-corpus and evidence writes with an expiring
single-writer lease, publishes complete staged directory sets, and recovers its
write-ahead journal before another writer proceeds. Same-filesystem file and
directory replacement failures retain or restore the prior corpus. POSIX builds
also sync changed directories; native Windows cannot portably make that
directory-sync guarantee, so power-loss durability there remains a release
canary claim rather than an inference from Unix tests.
