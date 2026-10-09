### Changed

<!-- codeflow:release-impact minor -->
- **Claude sessions compact at half the context window by default.**
  `codeflow init` now writes `CLAUDE_CODE_AUTO_COMPACT_WINDOW` = `"1000000"`
  and `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` = `"50"` into the `env` of
  `.claude/settings.json` at every tier and permission preset, and
  `codeflow update` adds them to existing projects. A session on a model
  with a native 1M window then compacts at about 500K tokens instead of
  about 967K, and a session limited to 200K compacts at about 100K instead
  of at the 200K boundary. The status line's `used_percentage` then
  measures against the full model window and no longer shows when
  compaction runs. To opt out, delete the two keys or the whole `env`
  object; to use other values, edit them. Update keeps a value the project
  changed and does not restore a key the project deleted after CodeFlow
  wrote it. A project with no recorded baseline and its own `env` gets no
  keys added; the update report names them. The settings of delegated runs
  do not change.
