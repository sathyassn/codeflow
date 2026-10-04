# Contributing to CodeFlow

CodeFlow is a Rust CLI. Changes land through pull requests.

## Ground rules

- The repository's full CodeFlow gate must be green before a PR is ready. It
  runs formatting, the workspace suite, warning-free Clippy and rustdoc, the
  90% aggregate line-coverage floor, CI parity, and model-evaluation contracts.
- Tests ship with the code that needs them, in the same PR.
- Commits follow the conventional format `type(scope): description`: imperative
  mood, lower-case type and no trailing period. Each commit is one logical
  change.
- **No AI attribution** in commit messages or PR bodies: no `Co-Authored-By`
  bot trailers, no "Generated with…" lines, no emoji. CodeFlow's own hooks
  enforce this.
- Branch names use a `type/kebab-name` prefix (`feat/`, `fix/`, `docs/`,
  `refactor/`, `test/`, `chore/`, …).

## Getting started

```sh
cargo build --release --locked
PATH="$PWD/target/release:$PATH" codeflow test --mode full --strict
```

- The gate requires `cargo-llvm-cov` (`cargo install cargo-llvm-cov --locked`).
- Run a narrower command while iterating, but report the full gate in the PR.
- Measure coverage locally with the full gate's `rust-coverage` target, the one
  instrumented `cargo llvm-cov nextest` run. Do not leave the numbers for CI to
  fill in.
- Write the PR Summary and Changes from the whole `base...HEAD` diff, not from
  the last commit or last review.

The release matrix targets native macOS, Linux and Windows binaries:

- WSL2 uses the Linux artifact.
- Git for Windows supplies the shell environment used by the hook shims on
  native Windows.
- Cross-target compilation is useful early evidence. The release checklist still
  requires native platform and installer canaries.

The operating contract for this repository is [AGENTS.md](../AGENTS.md). The
working method is in the portable `cf-method` skill under `.agents/skills/`,
mirrored for Claude Code under `.claude/skills/`. It covers planning weight,
when an architecture decision record (ADR) is warranted, and the capability
registry.

## Reporting bugs / requesting features

Open an issue using the templates. For security issues, see
[SECURITY.md](SECURITY.md) and do not open a public issue.

A bug report gives a reproduction and a severity. Severity is `critical`
when the bug is live in a published release or blocks current work and does
one of these:

- blocks adopters: a managed file, gate, hook or guard fails or refuses
  ordinary work in a consuming project;
- weakens a security boundary or lets it be bypassed (report it privately,
  as above);
- loses or rewrites data, records or history;
- deadlocks or hangs a gate, or blocks the tool's own fix path.

Anything else is `normal`. Maintainers confirm the severity at intake and
add the `critical` label. A fix then names the defect class, checks the
tree for every other site of that class, and closes the issue with a note
of what was fixed, what was deferred and which release carries it. The
full process is the issue-handling reference,
`.agents/skills/cf-method/references/issue-handling.md`; the release routes
for a critical bug are in [releasing](releasing.md#critical-issues).

## Releasing

Each work PR carries its release state (ADR-0062):

- a labelled pending `CHANGELOG.md` entry with its impact marker
- the coupled version stamps

`scripts/release.py` checks both. Conventional commit markers set a floor for
the version, not the version itself. cargo-dist builds the binaries, and a
human dispatches every publication.

See [releasing](releasing.md) for the runbook and for how a project that
*consumes* CodeFlow should handle its own versioning.

## License

By contributing, you agree that your contributions will be dual-licensed under
`MIT OR Apache-2.0`, as described in the [README](../README.md#license),
without any additional terms or conditions.
