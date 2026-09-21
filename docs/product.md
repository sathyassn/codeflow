# The CodeFlow product

<!-- WHY layer. Human-owned: agents propose changes here, the human accepts.
     Keep it small and stable; this file should change rarely.
     The non-goals in Technical are what planning is checked against. -->

## Concept

**CodeFlow installs the discipline; the harness does the developing.**

```cf-stage
codeflow init | one binary seeds the rules @accent
->
git hooks · guards · CI | one policy source, four planes enforce
->
Claude Code · Codex · Grok Build | the harness develops inside the rails
->
codeflow test · validate | verify the result
->
ledger · records · recall | remember why @positive
caption: scaffold, enforce, verify, remember; the harness supplies the middle
```

One Rust binary (`codeflow`) carries the scaffold that seeds those rules, and
it installs into any repository. The third stage is the only one CodeFlow does
not own: a vendor's interactive session does the developing, inside rails the
other four stages hold. Architecture below states the scope those stages cover;
the capabilities page names each piece and its status.

## Architecture

Scope is five commitments, and one of them is the knowledge model the other
four run around. The model is the record CodeFlow maintains inside the ship
flow, one layer per question a later reader asks.

```cf-stage
product | why this exists @accent
->
capabilities | what it does
->
architecture | how it is built
->
ADRs | why it is built that way
->
work | what is in flight
->
ledger | what happened @positive
caption: the six layer knowledge model, with recall over the whole record
```

The first five layers are committed Markdown. The structured records carry YAML
frontmatter; the capability registry carries YAML fences instead, one per
capability. The ledger is neither: it is the append-only JSONL event log the
binary writes, with an FTS5 cache rebuilt over both. In every case the record
survives the session that produced it. All five commitments, including the
model itself:

| Commitment | What CodeFlow ships |
|---|---|
| One binary | One installable binary with the scaffold embedded (rust-embed), offline and version-locked; `init`/`update` lay and refresh managed files by ownership class |
| Hard gates | A handful of hard gates around git, secrets, and tests, read from `.codeflow/policy.json` by every enforcement plane |
| The knowledge model | The six layers above, maintained inside the ship flow, with `recall` over the record |
| Graduated weight | Minimal, standard, and full tiers, where the git-discipline enforcement floor is the same at every tier and the tiers scale only the project-management on top (ADR-0019); the binary validates every shape, so growth is mechanical |
| Duo contracts | Host-neutral skill contracts for a Claude+Codex duo over every non-trivial repository task: stage-aware parallel independent work, capability-routed production and review, versioned dual approval, bounded worktree parallelism, effective network/tool autonomy, and evidence-based verification through each vendor's native interactive harness (ADR-0023, ADR-0024, ADR-0025, ADR-0046) |

Which tier a repository takes is chosen at `init` and recorded; the adoption
page walks that choice.

## Technical

Below are the audiences the scope is drawn for and the boundaries planning is
checked against.

### Users

| Audience | Standing |
|---|---|
| Solo, AI-assisted developers | First: someone running Claude Code, Codex, or another capable host who wants git, secret, and test discipline plus a durable why-record without standing up a framework |
| Cross-harness development | A scaffold design center when it composes vendor-native interactive sessions |
| Multi-user coordination | Deferred |

### Non-goals

This is the most load-bearing table in this file. Each row is something the
project will not do, stated so planning can be checked against it.

| Not this | What that rules out |
|---|---|
| Not a runtime harness, agent framework, or model router | No daemon or autorun. Vendor-native sessions, plugins, tools, worktrees, tasks, memory, sandbox, and permissions are composed at the process boundary, never reimplemented (the v2 charter §1, in the repository at `docs/plan/v2/00-charter.md`; ADR-0023) |
| Not a process-enforcement engine | No phase ordering, role boundaries, or review-before-X sequencing in code; gates exist only where a mistake is irreversible or invisible (charter §6.6) |
| Not a general GUI/TUI product or application shell | The bounded `cf-present` review document defined by SPC-004 is the sole interactive exception: an explicit CLI action may launch an isolated browser window backed by a loopback-only, per-session service that self-terminates. Agents author this session's catalog document; the runtime owns chrome and Comment. It is not a dashboard, remote server, persistent service, consuming-product UI framework, or a clone of the design-exploration board. The opt-in documentation portal is a generated static artifact that applies the same utility craft to durable source-linked docs, not an operated CodeFlow interface |
| No bespoke memory infrastructure | No embeddings, vector DBs, GraphRAG, or database-as-authority; markdown + JSONL truth with an FTS5 cache (D17) |
| Not a substitute for an OS or harness security boundary | CodeFlow ships macOS, Linux/WSL2, and x86-64 native-Windows binaries plus deterministic safety feedback, but it does not claim that every harness provides equal containment. In particular, native Windows Claude work that needs an OS sandbox moves to WSL2 or a container |
