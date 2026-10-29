# Claude context-window and compaction policy

Read this when a project asks when Claude compacts automatically, or wants to
change or remove the compaction settings CodeFlow writes.

## The default

`codeflow init` writes these string values into the top-level `env` object of
`.claude/settings.json` at every tier and with every permission preset, and
`codeflow update` adds them to a project installed before they shipped:

```json
"env": {
  "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "1000000",
  "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "35"
}
```

The window variable takes a plain integer from 100000 to 1000000 and is
capped at the model's real context window. The percentage, from 1 to 100, can
only lower the compaction threshold, and a value above Claude Code's default
is ignored. The percentage applies to main sessions and to subagents.

| Session | Claude Code default | With these values |
|---|---|---|
| Model with a native 1M window | compacts at about 967K tokens | compacts at about 350K tokens |
| Session limited to 200K | compacts at the 200K boundary | compacts at about 70K tokens |

These values decide when automatic compaction runs. They are not a billing
cap, they do not guarantee that detail survives a compaction, and they are
not evidence of time or token savings.

Once the window variable is set, the status line's `used_percentage` measures
against the full model window, so it no longer shows when compaction runs.

## Why 35 percent, and what it costs

The keys live in the shared file because the compaction point is a team cost
policy that also applies to subagents. CodeFlow shipped 50 percent until
3.2.0; 35 percent was chosen on 2026-10-27 and is revisited once an adopter's
measurement of 50, 40, 35 and 30 percent is published on CodeFlow issue 358.

- **Cost per turn.** Every turn sends the whole context as input. Prompt
  caching bills most of it as cache reads, but per-turn input still grows
  with the context carried: a turn near 350K carries about 0.7 times the
  input of a turn near 500K, and about 2.3 times one near 150K. Some models
  price input above a size threshold at a higher rate; check the current
  rate for the model in use.
- **Compaction frequency.** A lower point compacts more often. Each
  compaction reads the full context once and writes a summary, and detail
  the summary drops is lost.
- **Sessions limited to 200K.** The percentage applies to them too, so they
  compact at about 70K.
- **Sessions per phase.** A session that ends at a phase boundary, such as a
  fresh session per task, may never reach the threshold. The point matters
  most for long orchestrator sessions that run across many units.
- **What survives a compaction.** Detail that lives only in the
  conversation may be lost. Durable records (task records, git, `AGENTS.md`)
  let a session recover its task, branch and next step when it rereads
  them. The orient digest that `SessionStart` prints after a compaction
  supplies context and pointers to those records; it does not restore the
  session's state by itself.

Without these keys, a session on a 1M-window model compacts near 967K, so any
value here lowers the peak context against the harness default.

## Change the values or opt out

- **For the project:** edit the values in the shared `env` of
  `.claude/settings.json`. Update keeps a value that differs from the one
  CodeFlow last shipped.
- **For one person:** set `env` in `.claude/settings.local.json`, which is
  gitignored and takes precedence over the shared file. Since Claude Code
  v2.1.211 a worktree session uses the file at the main checkout's root, so
  one file serves every worktree; with the stated exceptions (outside git,
  the repository root at the home folder, Windows, or another owner) each
  checkout needs its own copy. See `personal-settings.md` in this folder.
  Update never reads or writes this file.

`codeflow update` and the shared `env`:

- A value still equal to the one CodeFlow last shipped, as recorded under
  `.codeflow/.baseline/`, moves to the new shipped value, whatever the
  project meant by it: update cannot tell a deliberate 50 from the shipped
  50, so a project on 50 moves to 35. To keep 50, set it again in the shared
  `env` after the update; update then keeps it, since it no longer equals
  the shipped value. A person who wants 50 for themselves sets it in
  `.claude/settings.local.json` before or after the update.
- A value that differs from the one CodeFlow last shipped stays. When the
  shipped default changes, the update report names the key it kept, never
  the value.
- To drop one key, delete it. A key CodeFlow wrote that the project has
  since deleted stays deleted.
- To opt out entirely, delete both keys or the whole `env` object. It stays
  deleted.
- Every other entry in `env` belongs to the project and is never changed.

Update adds a missing key only when the recorded baseline shows CodeFlow never
wrote it, as in a project installed before this default. With no recorded
baseline, update adds the keys only when the project has no `env` at all; an
existing `env` is left as it is and the update report names the keys it did
not add. Add them by hand if the project wants them. An `env` that is not an
object of strings is a reported configuration error, and update leaves it
untouched. Update reports key names, never values. The export that
`codeflow update --diff` writes never shows a value, key name or line from
the project's files or from the recorded baseline; it is judged only
against the assets inside the binary. For JSON, TOML and YAML it lists each
key path the update changed, with CodeFlow's shipped value and a marker for
the file before and after (same as shipped, differs, added by the project,
absent); keys the project added are counted, not named. For Markdown and
other text it shows the shipped lines the update writes and counts the
rest. A file in any other format, or one that does not parse, is withheld.

## Inspect without exposing values

`env` may hold secrets, so inspect without printing sensitive values. Present
only the proposed non-secret context-key delta and any conflicts, and ask only
when the project's existing intent would change. A malformed object is a
reported configuration error, not permission to replace it.

## Verify without manufacturing evidence

A model alias, `[1m]` suffix, catalog entry, configured value, or requested
launch is not runtime evidence of the effective window. Verify the window and
the effective values through a public native Claude surface, and record
requested versus observed model. Do not burn the context window to trigger
automatic compaction.

If continuity evidence is useful, one bounded manual compaction/resume check
may verify that task authority and key decisions remain available. Label it
manual, since it does not prove the automatic threshold. A schema-v2 delegated
lifecycle deliberately poisons a compact/restart, so perform any post-compact
lifecycle check under a fresh run id and state directory, and never copy this
`env` into the immutable delegated task settings.
