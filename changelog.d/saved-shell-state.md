### Security

<!-- codeflow:release-impact minor -->
- **The guards and presets protect the shell state a harness saves for
  its later commands.** A harness loads this state before each later
  command, so a write to it changes what those commands run.
  - The guards refuse a command that writes, replaces, links, moves,
    removes or extracts into that state, wherever the harness keeps it,
    including a configuration folder its own setting moves.
  - Where the guard cannot tell what a command writes, it refuses when
    the command names that state or runs in a folder that holds it.
  - edit-guard refuses a native edit of that state in Codex and Grok. In
    Claude Code the presets deny native edits at the default folders
    only, so a native edit below a moved configuration folder is not
    refused.
  - The presets deny the same writes, `codeflow update` adds the entries
    to an existing project, and `codeflow doctor` reports settings that
    lack them.
  - Removals and extractions in a project folder, and a folder of your
    own with a similar name, run as before.
