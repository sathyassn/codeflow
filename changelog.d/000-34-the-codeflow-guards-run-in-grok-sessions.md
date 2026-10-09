### Fixed

<!-- codeflow:release-impact patch -->
- **The CodeFlow guards run in Grok sessions.** Grok expands `$name` and
  `${...}` in a hook command itself and skips the hook, letting the tool
  call through, when a name is unset. Every CodeFlow hook command in 3.0.0
  carried shell variables, so in a Grok session git-guard, exec-guard,
  edit-guard and session-orient never ran, and doctor reported only folder
  trust. After `codeflow update`, the hook commands in
  `.grok/hooks/codeflow.json`, `.claude/settings.json` (which Grok also
  reads) and `.codex/hooks.json` carry no `$`: each runs
  `codeflow hook <name> --contract 3` and exits 2 with a reason whenever
  the hook fails, since Codex lets a call through on exit 2 with no
  reason, naming the installer and `codeflow update` when the binary is
  missing. A
  binary older than 3.0.0 still blocks, now with its own usage error in
  place of the install line. Grok 1.0.46 also sends each payload field
  under both spellings (`toolName` and `tool_name`), which the guards took
  for an unreadable payload and allowed; they now read it, and a payload
  whose two spellings disagree is still reported as unreadable. Grok shows
  only the first line of a hook's error output as the reason it denied a
  call, so a guard refusing a Grok call also returns Grok's deny decision
  with the whole refusal, the rule and its sanctioned path included.
  `codeflow update` keeps your own edits to these files. It merges Claude
  settings by their hook entries, and merges a Grok or Codex hook file
  you edited by a 3-way merge that applies on its own when your edit does
  not overlap the new commands. When it overlaps, update leaves the file
  as it is and writes a `.new` file beside it holding the merge: resolve
  its conflict markers in favour of the shipped CodeFlow hook commands,
  replace your file with it and delete the `.new` file. Update never
  touches a hook file it does not manage, such as
  `.claude/settings.local.json`, which Grok also reads, nor one that is a
  symlink, and it keeps an edit when the shipped version has not changed:
  in such a file, replace any CodeFlow hook command that carries a `$`
  with the shipped one, as `codeflow doctor --check grok` names.
  Then run `codeflow doctor --check grok` and confirm that a live Grok
  session refuses a dangerous shell command.
