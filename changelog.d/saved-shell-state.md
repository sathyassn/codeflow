### Security

<!-- codeflow:release-impact minor -->
- **The guards and presets protect the shell state a harness saves for
  its later commands.** Claude Code, Codex and herdr-core load saved
  shell state, such as environment files and shell snapshots, before
  later commands. A write to one changes what every later command runs,
  while each command still looks harmless to the guards.
  - These locations are now part of the shell startup class: Claude
    Code's `session-env` and `shell-snapshots`, Codex's `shell_snapshots`,
    herdr-core's `templates` and the file `CLAUDE_ENV_FILE` names.
    exec-guard and edit-guard refuse writes to them, the Claude presets
    carry `Edit` denies and sandbox write denies for them, and the Codex
    `cf-guard` profile keeps them read only.
  - The guards follow `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_CONFIG_HOME`
    and `APPDATA` when they are moved. Because the guard cannot see
    another profile's folder, a folder with one of these names is
    protected wherever it is, so a folder of your own with such a name is
    refused too.
  - exec-guard judges file operations in a separate pass that runs after
    every other check and only when no other check refused. Removing or
    moving a tree that holds saved state refuses, and so does copying,
    syncing or extracting a tree into a folder that is, holds or lies in
    saved state. The pass reads the target up to four levels down and
    stops at 65536 directories or 1048576 entries. A removal is judged by
    the node it removes and does not follow links below it; a placement
    judges a link in its target by where the link leads.
  - A target the pass cannot read in full refuses: a tree deeper than four
    levels, an unreadable folder or a scan that reaches its limit. So some
    ordinary lines now refuse, such as `rm -rf target` on a deep build
    folder or extracting an archive in a deep project. Name a narrower
    folder the guard can read.
  - A command whose target the guard cannot bind exactly, such as one of
    several words in a loop or a path behind a variable it cannot read,
    keeps the judgment it had before.
  - edit-guard now also checks Claude Code's `MultiEdit` and
    `NotebookEdit` when a settings file sends them to it. The shipped hook
    entries send `Edit` and `Write`; the preset `Edit` denies cover the
    other edit tools at the default locations.
  - A line that names one of these locations is held to the closed rule,
    so some read-only lines that pipe a listing of them into another
    program now refuse; read them in a call of their own.
  - `codeflow doctor --check startup-files` reports settings that lack
    the new entries and a moved configuration folder, and
    `codeflow update` adds the entries to an existing project. Each
    refusal says to ask the operator to make the change by hand.
