# Contributing to codeflow

Thanks for your interest. codeflow is a Rust CLI; changes land through pull
requests.

## Ground rules

- The repository's full CodeFlow gate must be green before a PR is ready. It
  runs formatting, the workspace suite, warning-free Clippy and rustdoc, the
  90% aggregate line-coverage floor, CI parity, and model-evaluation contracts.
- Tests ship with the code that needs them, in the same PR.
- Commits follow the conventional format `type(scope): description` — imperative
  mood, lower-case type, no trailing period; one logical change per commit.
- **No AI attribution** in commit messages or PR bodies: no `Co-Authored-By`
  bot trailers, no "Generated with…" lines, no emoji. codeflow's own hooks
  enforce this.
- Branch names use a `type/kebab-name` prefix (`feat/`, `fix/`, `docs/`,
  `refactor/`, `test/`, `chore/`, …).

## Getting started

```sh
cargo build --release --locked
PATH="$PWD/target/release:$PATH" codeflow test --mode full --strict
```

The gate requires `cargo-llvm-cov` (`cargo install cargo-llvm-cov --locked`).
The release matrix targets native macOS, Linux, and Windows binaries; WSL2 uses
the Linux artifact. Git for Windows supplies the shell environment used by the
hook shims on native Windows. Cross-target compilation is useful early evidence,
but the release checklist still requires native platform and installer canaries.
Run a narrower command while iterating, but report the full gate in the PR.
Measure coverage locally with the same `cargo llvm-cov` command CI uses; do
not leave the numbers for CI to fill in. Write the PR Summary and Changes
from the whole `base...HEAD` diff, not from the last commit or last review.

The operating contract for this repo is [AGENTS.md](AGENTS.md); the working
method (planning weight, when an ADR is warranted, the capability registry) is
in the portable `cf-method` skill under `.agents/skills/`, mirrored for Claude
Code under `.claude/skills/`.

## Reporting bugs / requesting features

Open an issue using the templates. For security issues see
[SECURITY.md](SECURITY.md) — please do not open a public issue.

## Releasing

Release state lives in each work PR (ADR-0062): curated pending CHANGELOG
notes, one reviewed impact annotation adjacent to each new entry, and the
coupled version stamps. Conventional markers are tripwires against an
understated impact, not a second version calculator. cargo-dist is the sole
tag, release, and artifact publisher, and publication is a deliberate human
dispatch. See [docs/releasing.md](docs/releasing.md) for the runbook — and for
how a project that *consumes* codeflow should handle its own versioning.

## License

By contributing, you agree that your contributions will be dual-licensed under
`MIT OR Apache-2.0`, as described in the [README](README.md#license), without any
additional terms or conditions.
