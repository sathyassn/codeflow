# codeflow — architecture

<!-- HOW layer. Updated only inside the ship flow, in the same PR as the code,
     when an ADR declares architecture impact. Link to decisions by ADR id —
     never duplicate their content here.
     When an area outgrows this file, graduate it to docs/architecture/<area>.md
     and leave a one-line pointer behind. -->

## Overview

A two-crate Cargo workspace that builds one binary with the scaffold embedded.
`codeflow-core` is the engine library — all mechanics live here. `codeflow-cli`
is a thin dispatcher: `main.rs` is a clap command surface over 12 subcommands
(`init`, `update`, `hook`, `git-hook`, `orient`, `test`, `validate`, `status`,
`integrate`, `doctor`, `recall`, `remote`), each a small handler in `cmd/` that
calls into core; `embedded.rs` embeds `assets/` via rust-embed (debug builds
read `assets/` from disk for instant scaffold iteration). The consuming repo is
its own first consumer, so `assets/` is as much the product as the code.

## Areas

### engine — `crates/codeflow-core` + `crates/codeflow-cli`

Core modules grouped by responsibility:

- **Scaffold** (`scaffold/`): `init`, `update`, manifest, 3-way merge, and the
  ownership classes below; sourced from the rust-embed asset provider.
- **Enforcement** (`hooks/`, `security/`, `git/`, `integrate.rs`, `remote.rs`):
  the `git-guard` PreToolUse handler and git-client hook stages, the secret
  scanner, git conflict detection + CI wait, the flock-guarded `integrate`
  primitive with its gate-context token, and the GitHub remote-protect adapter.
- **Records / knowledge** (`models/`, `ledger/`, `workgraph/`, `validate/`,
  `capability.rs`, `recall.rs`, `registry.rs`): frontmatter models, the JSONL
  ledger, the work graph, `validate` (+ the `--docs` referential-integrity
  lint), the capability registry parser, FTS5 recall, and the cross-repo
  registry.
- **Support** (`doctor/`, `settings/`, `status.rs`, `testing/`, `file_lock.rs`,
  `error.rs`): the doctor check table (7 checks — hooks, claude, config,
  permissions, network, delegates, repo-integrity), structured settings merge,
  generated status views, the test-gate engine, path flock, and pruned error
  types.

Enforcement is spread across four planes, all reading one config
(`.codeflow/policy.json`): git client hooks, the Claude `git-guard` hook,
remote branch protection, and CI. Local planes are fast feedback; CI + remote
protection are the authoritative perimeter (ADR-0002; charter §6.5). The git
client plane adds a `pre-merge-commit` shim that blocks non-fast-forward merge
commits onto protected branches (ADR-0007); fast-forward merges fire no client
hook and are caught only in-session by `git-guard`.

Records follow the markdown-truth design (D17): markdown + YAML frontmatter is
the source of truth, the JSONL ledger is the append-only event log, and SQLite
FTS5 is a rebuildable cache — no database-as-authority, no embeddings. Core
reads through a `RecordStore` trait with a `MarkdownStore` implementation.

### scaffold — `assets/`

`assets/base/` holds the shipped scaffold (AGENTS.md/CLAUDE.md templates, the
`claude/` artifacts, policy.json, git-hook shims, docs and pm templates); the
engine manages it by three ownership classes (charter §4.3): **fully-managed**
files (agents, skills, commands, hook shims, CI) refresh by hash and 3-way
merge from `.codeflow/.baseline/`; **managed-region** files (AGENTS.md markers,
settings.json codeflow-prefixed keys) touch only their region; **user-owned,
schema-versioned** files (policy.json, all `docs/`) only gain new keys with
defaults, never mutating user values. `scaffold-manifest.toml` is the update
contract.

### docs — `docs/`

The six-layer knowledge model this file belongs to, plus `docs/plan/v2/` (the
charter, split, and the rebaseline ADRs).
