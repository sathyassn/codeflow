# Ordinary chat reply — D1

What an agent would type into the terminal today. Retained as a plain baseline; it is
not a design.

---

This repository organises its knowledge into six layers. From `AGENTS.md`:

| Layer | Lives in | Changes |
|---|---|---|
| WHY — purpose, users, scope, non-goals | `docs/product.md` | rarely; human-owned |
| RULES — how we work | this file + the agent skills (`.claude/skills/`, `.agents/skills/`) | rarely |
| WHAT — what the system does | `docs/capabilities.md` (CAP-### registry) | every ship |
| HOW — structure and decisions | `docs/architecture.md` + `docs/decisions/` (ADRs) | per decision |
| WORK — planned and active work | `project-management/` (epics, specs, tasks — allocated by the CLI) | daily |
| TRACE — what happened and why | ledger + `codeflow recall` | automatic |

On who may write what, the same file says:

- everything outside the codeflow markers in `AGENTS.md` is project-owned; the block
  between them is maintained by `codeflow update`;
- docs mutate only inside the ship flow, in the same PR as the code;
- ADRs and the ledger are never edited — supersede with a new entry instead;
- CodeFlow writes the flat stable-ID records under `project-management/`;
- status views are generated (`codeflow status`) — never hand-maintain a dashboard.

There is one declared relationship between them:

> The traceability spine runs downward: capability → epic/task → ADR/spec → PR → ledger.
