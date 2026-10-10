### Changed

<!-- codeflow:release-impact minor -->
- **One rule for how one model family calls another.** The skills and
  managed instructions gave hosts different answers (issues 30 and 31).
  After `codeflow update`, the rule lives in one file,
  `cf-model-orchestrator/resources/routing/transport.md`, and every other
  skill, the CLAUDE.md and AGENTS.md rows and the exec-guard refusal cite
  it (ADR-0077). Another family runs as its own interactive CLI in a Herdr
  tab: Codex on its app-server, Claude with its turns tracked by
  `codeflow delegate`, Grok through Grok Build. The same family runs as
  native subagents, and print or exec modes stay refused. The official
  Codex plugin becomes an optional fallback on a Claude Code host, and
  tmux the last fallback when no Herdr server is reachable. `cf-herdr` now
  drives any reachable Herdr server from any host, inside a Herdr pane or
  not, with its anti-hijack rules unchanged. The seats' launch flags are
  stated once, so an edit handoff to Claude now launches in the production
  posture instead of auto mode, and a new seat's first-run prompts are
  named: the caller answers folder trust for the task's own folder, the
  operator answers every hook trust prompt (Grok's included), and a
  self-update offer is skipped. A Grok builder seat is marked not qualified
  until ADR-0075 D3's sandboxed route is proven, so building goes to a
  Claude or Codex seat. A long
  Codex reply and its observed model and effort are read from the seat's
  session record. Review and consult briefs ask for one holistic pass over
  the whole unit and its blast radius, earlier findings being checks within
  it, and `cf-herdr` states how a review seat runs. `codeflow doctor
  --check delegates` no longer warns about a missing Codex plugin and now
  warns when `herdr` is missing, naming tmux as the fallback. Projects that
  relied on the plugin keep it as the fallback; to use the default route,
  install Herdr. Where a project customised these skills and its edits
  overlap the new text, `codeflow update` leaves a `.new` proposal beside
  the file; reconcile it so the project's routing prose agrees. "Any host"
  adds no native Windows support: delegate state there still needs WSL2.
