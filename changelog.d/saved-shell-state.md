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
  - The guards refuse a native edit that writes that state, in Claude
    Code, Codex and Grok. The Claude presets now send Claude Code's
    `Edit`, `MultiEdit`, `Write` and `NotebookEdit` calls to edit-guard,
    as the Codex and Grok hook files already send theirs. A Claude native
    edit also gets edit-guard's refusals for the enforcement paths and
    for a moved startup folder.
  - The presets deny the same writes, `codeflow update` adds the entries
    and the edit hook to an existing project, and `codeflow doctor`
    reports settings that lack them.
  - Removals and extractions in a project folder, and a folder of your
    own with a similar name, run as before.
  - The operator accepted the new refusals near that state, including a
    lone `cd` into one of its folders when the path is quoted, which
    includes any such path that holds a space.
