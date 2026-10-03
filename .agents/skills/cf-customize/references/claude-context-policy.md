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
  "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "50"
}
```

The window variable takes a plain integer from 100000 to 1000000 and is
capped at the model's real context window. The percentage, from 1 to 100, can
only lower the compaction threshold, and a value above Claude Code's default
is ignored. The percentage applies to main sessions and to subagents.

| Session | Claude Code default | With these values |
|---|---|---|
| Model with a native 1M window | compacts at about 967K tokens | compacts at about 500K tokens |
| Session limited to 200K | compacts at the 200K boundary | compacts at about 100K tokens |

These values decide when automatic compaction runs. They are not a billing
cap, they do not guarantee that detail survives a compaction, and they are
not evidence of time or token savings.

Once the window variable is set, the status line's `used_percentage` measures
against the full model window, so it no longer shows when compaction runs.

## Change the values or opt out

`codeflow update` keeps the project's choices in `env`:

- To use other values, edit them. Update keeps every value the project set.
- To drop one key, delete it. Update records what it last wrote under
  `.codeflow/.baseline/`, and a key it wrote that the project has since
  deleted stays deleted.
- To opt out entirely, delete both keys or the whole `env` object. It stays
  deleted.
- Every other entry in `env` belongs to the project and is never changed.
- A developer who wants different values only for themselves sets them in
  `.claude/settings.local.json`, which takes precedence over the shared
  project file.

Update adds a missing key only when the recorded baseline shows CodeFlow never
wrote it, as in a project installed before this default. With no recorded
baseline, update adds the keys only when the project has no `env` at all; an
existing `env` is left as it is and the update report names the keys it did
not add. Add them by hand if the project wants them. An `env` that is not an
object of strings is a reported configuration error, and update leaves it
untouched. Update reports key names, never values.

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
