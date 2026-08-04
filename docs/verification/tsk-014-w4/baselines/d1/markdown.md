# Project organization — six layers

Ordinary repository Markdown: this is very close to what `AGENTS.md` already contains,
which is the point of keeping it as a baseline. It is not a design.

| Layer | Lives in | Changes |
|---|---|---|
| WHY — purpose, users, scope, non-goals | `docs/product.md` | rarely; human-owned |
| RULES — how we work | this file + the agent skills (`.claude/skills/`, `.agents/skills/`) | rarely |
| WHAT — what the system does | `docs/capabilities.md` (CAP-### registry) | every ship |
| HOW — structure and decisions | `docs/architecture.md` + `docs/decisions/` (ADRs) | per decision |
| WORK — planned and active work | `project-management/` (epics, specs, tasks — allocated by the CLI) | daily |
| TRACE — what happened and why | ledger + `codeflow recall` | automatic |

The traceability spine runs downward: capability → epic/task → ADR/spec → PR → ledger.

## Write authority

- **human-owned** — a person edits it directly, and rarely.
- **project-owned** — everything outside the managed markers belongs to this project.
- **ship flow only** — editable, but only in the same pull request as the code it describes.
- **tool-maintained** — a tool owns those bytes; an edit is overwritten on the next update.
- **CLI-allocated** — the CLI allocates the record and its stable id; you fill the body in.
- **append-only** — never edited; a change is a new entry that supersedes the old one.
- **generated** — produced on demand from the records; there is no file to maintain.
