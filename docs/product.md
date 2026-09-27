# The CodeFlow product

<!-- WHY layer. Human-owned: agents propose changes here, the human accepts.
     Keep it small and stable; this file should change rarely.
     The non-goals in Concept are what planning is checked against. -->

## Concept

CodeFlow is one Rust binary that installs git, secret and test discipline into
a repository, while the harness you already use does the developing.

CodeFlow owns the scaffold, the gates, the checks and the record. A vendor's
interactive session, such as Claude Code, Codex or Grok Build, works inside
those rails and is never replaced. The first audience is a solo developer who
wants that discipline and a durable record of why, without standing up a
framework.

| Audience | Standing |
|---|---|
| Solo, AI-assisted developers | First: someone running Claude Code, Codex, or another capable host who wants git, secret, and test discipline plus a durable why-record without standing up a framework |
| Cross-harness development | A scaffold design center when it composes vendor-native interactive sessions |
| Multi-user coordination | Deferred |

What CodeFlow is not. Planning is checked against each row.

| Not this | What that rules out |
|---|---|
| Not a runtime harness, agent framework, or model router | No daemon or autorun. Vendor-native sessions, plugins, tools, worktrees, tasks, memory, sandbox, and permissions are composed at the process boundary, never reimplemented (architecture decision record ADR-0023) |
| Not a process-enforcement engine | No phase ordering, role boundaries, or review-before-X sequencing in code; gates exist only where a mistake is irreversible or invisible |
| Not a general graphical or terminal UI product or application shell | The bounded `cf-present` review document defined by spec SPC-004 is the sole interactive exception: an explicit CLI action may launch an isolated browser window backed by a loopback-only, per-session service that self-terminates. Agents author this session's catalog document; the runtime owns chrome and the Comment tool. It is not a dashboard, remote server, persistent service, consuming-product UI framework, or a clone of the design-exploration board. The opt-in documentation portal is a generated static artifact that applies the same utility craft to durable source-linked docs, not an operated CodeFlow interface |
| No bespoke memory infrastructure | No embeddings, vector databases, GraphRAG, or database-as-authority; Markdown and JSONL are the truth, with a full-text search cache over them |
| Not a substitute for an OS or harness security boundary | From 3.0.0, CodeFlow ships macOS, Linux/WSL2 and x86-64 native-Windows binaries (the [adoption](adoption.md) page lists the archives per release) plus deterministic safety feedback, but it does not claim that every harness provides equal containment. Native Windows Claude work that needs an OS sandbox moves to WSL2 or a container |

## Architecture

The record CodeFlow maintains is six layers, one per question a later reader
asks, and each changes at its own pace.

- The first five layers are committed Markdown. Structured records carry YAML
  frontmatter; the capability registry carries one YAML fence per capability.
- The ledger is the append-only JSONL event log the binary writes, with a
  full-text search cache rebuilt over the whole record for `codeflow recall`.
- The traceability spine links a capability to its epics and tasks, their
  decisions and specs, the pull request and the ledger.

Which tier a repository takes is chosen at `init` and recorded; the
[adoption page](adoption.md) walks that choice.

## Technical

The five commitments CodeFlow ships, and the tier that brings each one.

| Commitment | What CodeFlow ships |
|---|---|
| One binary | One installable binary with the scaffold embedded (rust-embed), offline and version-locked; `init`/`update` lay and refresh managed files by ownership class |
| Hard gates | A handful of hard gates around git, secrets, and tests, read from `.codeflow/policy.json` by every enforcement plane |
| The knowledge model | The six layers above, maintained inside the ship flow, with `recall` over the record |
| Graduated weight | Minimal, standard, and full tiers, where the git-discipline enforcement floor is the same at every tier and the tiers scale only the project-management on top (ADR-0019); the binary validates every shape, so growth is mechanical |
| Duo contracts | Host-neutral skill contracts for a Claude+Codex duo over every non-trivial repository task: stage-aware parallel independent work, capability-routed production and review, versioned dual approval, bounded worktree parallelism, effective network/tool autonomy, and evidence-based verification through each vendor's native interactive harness (ADR-0023, ADR-0024, ADR-0025, ADR-0046) |

Every decision cited here is listed in the [decision map](decision-map.md).
