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
CodeFlow itself supports macOS and Linux; Windows is unsupported because its git
hooks and executable-bit handling use Unix paths. Run a narrower command while
iterating, but report the full gate in the PR.

The operating contract for this repo is [AGENTS.md](AGENTS.md); the working
method (planning weight, when an ADR is warranted, the capability registry) is
in the portable `cf-method` skill under `.agents/skills/`, mirrored for Claude
Code under `.claude/skills/`.

## Reporting bugs / requesting features

Open an issue using the templates. For security issues see
[SECURITY.md](SECURITY.md) — please do not open a public issue.

## Releasing

Releases are conventional-commit driven (git-cliff for the version + changelog,
cargo-dist for the binaries) and human-gated. See [docs/releasing.md](docs/releasing.md)
for the runbook — and for how a project that *consumes* codeflow should handle
its own versioning.

## License

By contributing, you agree that your contributions will be dual-licensed under
`MIT OR Apache-2.0`, as described in the [README](README.md#license), without any
additional terms or conditions.
