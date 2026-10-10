### Added

<!-- codeflow:release-impact minor -->
- **Operator feedback has one tracker at the full tier (issue 75).**
  `codeflow feedback new --topic <topic> --source <source> "<summary>"`
  issues an `FB-NNN` id from the shared registry and writes
  `project-management/feedback/FB-NNN.md` from a new template, with the
  operator's words verbatim and the agent's reading kept apart.
  `codeflow feedback status` moves an item by one table (received, placed,
  closed, declined only with the operator's confirmation, superseded), and
  `codeflow feedback list` groups items by topic with open items first,
  with `--json` and an optional generated `INDEX.md` (`--write`).
  `validate --docs` fails a malformed item and warns, never fails, when a
  written index is stale; `codeflow status` counts open items and
  `codeflow recall` searches them. Topics are the project's
  `[feedback] topics` in `.codeflow/project.toml`, written with a default
  list on first use. A record is for feedback that sets a standing rule,
  declines or reorders planned work, or spans several units; nothing
  requires one. `cf-method`'s project organization reference gains an
  "Operator feedback" section, and cf-ship's PR evidence names FB ids under
  Links. After `codeflow update`, a full-tier project has the template; the
  folder and the `[feedback]` section appear only on first use, and the
  standard and minimal tiers change nothing on disk. The registry gains an
  `FB` kind: an older binary's `ids check` reports an `ids/FB/` entry as a
  repaired-history warning and still passes.
