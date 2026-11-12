### Security

<!-- codeflow:release-impact minor -->
- **The guards and presets protect the shell state a harness saves for
  its later commands.** Claude Code, Codex and herdr-core load saved
  shell state, such as environment files and shell snapshots, before
  later commands. A write to one changes what every later command runs,
  while each command still looks harmless to the guards.
  - The locations are a new entry of the action table,
    `saved_state_paths`: Claude Code's `session-env` and
    `shell-snapshots`, Codex's `shell_snapshots`, herdr-core's
    `templates`, and the file `CLAUDE_ENV_FILE` names. The guards follow
    `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_CONFIG_HOME` and, on Windows,
    `APPDATA` when the hook process has them.
  - exec-guard and git-guard judge these locations in a pass of their own
    that runs after every other check and only adds refusals. A redirect,
    `tee`, `cp`, `mv` or `ln` that writes one refuses, and so does a
    removal or move of a location or of a folder that holds one, a copy or
    sync into one, and an extraction into one.
  - An extraction into a folder above a location, such as the home,
    `~/.claude` or `~/.config`, refuses because the guard cannot see what
    the archive holds. Extract into a named sub-folder instead, with
    `tar -C <dir>` or `unzip -d <dir>`. Extracting in a project folder, or
    in a folder of the home that holds no location, runs as before.
  - The pass compares a target with each location by the node it reaches
    on disk, so a link or `..` on the way to it does not hide it. A folder
    of your own that only has the same name as a saved-state folder, such
    as `notes/session-env` in a project, is ordinary.
  - `rm -rf target` and other removals in a project run as before.
  - edit-guard refuses a native edit of a file in these locations, and now
    also checks Claude Code's `MultiEdit` and `NotebookEdit` when a
    settings file sends them to it. The shipped hook entries send `Edit`
    and `Write`; the preset `Edit` denies cover the other edit tools at
    the default locations.
  - The Claude presets carry `Edit` denies and sandbox write denies for
    the default locations, and the Codex `cf-guard` profile keeps them
    read only.
  - `codeflow doctor --check startup-files` reports settings that lack
    the new entries and a moved configuration folder, and
    `codeflow update` adds the entries to an existing project. Each
    refusal says to ask the operator to make the change by hand.
