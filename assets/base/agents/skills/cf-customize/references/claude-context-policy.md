# Claude context-window and compaction policy

Read this only when a consuming project asks to change Claude's context window
or automatic compaction threshold. This is project customization, not a generic
CodeFlow default.

## Decide the scope

First verify the effective context window through a public native Claude
surface. A model alias, `[1m]` suffix, catalog entry, configured value, or
requested launch is not runtime evidence. Retain the qualified model selector;
do not promote a binding or rewrite every selector merely to obtain a window.

For a project that explicitly chooses the policy after verifying effective 1M
support, add these string values to the existing top-level `env` object in
`.claude/settings.json`:

```json
"env": {
  "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "1000000",
  "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "50"
}
```

The supported window setting is bounded by the model's actual context. With an
effective 1M window, 50 percent targets automatic compaction at approximately
500K tokens. A 200K-limited session remains 200K and would compact at roughly
100K. The environment controls outrank corresponding command, flag, or settings
choices, and the percentage override applies to qualifying main and subagent
sessions. The status-line percentage still uses the full model context. This is
a compaction policy, not a billing cap, a guarantee that detail will survive,
or evidence of time/token savings.

This choice is repository-local and opt-in. Do not add it to CodeFlow's generic
settings presets or to personal/global/managed settings without separate
explicit approval. If effective 1M support is absent, unsupported, or
unobserved, leave the keys absent and explain the smaller-window behavior.

## Preserve consumer settings

Inspect the existing `env` object before proposing an edit. It must remain a
JSON object whose values are strings; preserve every intentional entry. The
CodeFlow settings merge treats an existing top-level `env` as consumer-owned:
it preserves the whole value and does not deep-merge incoming keys. Therefore,
inspect without printing sensitive values. Present only the proposed non-secret
context-key delta and any conflicts; clarify only if existing intent would
change. Do not rely on `codeflow update` to inject or repair these two entries.
A malformed object is a reported configuration error, not permission to replace
it.

Keep absent settings absent for projects that do not opt in. Repeated
`codeflow update` runs must preserve an existing valid or intentionally
different `env` value-for-value and must not add the
policy to generic presets.

## Verify without manufacturing evidence

Parse the edited JSON, run the normal update twice, and inspect the diff for
preserved `env` and a no-op second update. Start one bounded native session and
record requested versus observed model, effective window, and effective values
from public surfaces. Do not burn the context window to trigger automatic
compaction.

If continuity evidence is useful, one bounded manual compaction/resume check may
verify that task authority and key decisions remain available. Label it manual:
it does not prove the automatic threshold. A schema-v2 delegated lifecycle
deliberately poisons a compact/restart; perform any post-compact lifecycle check
under a fresh run id and state directory.
