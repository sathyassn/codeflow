## Hook trust prompts

Read this when a new seat shows a hook trust prompt. It is the one case in
which the caller may answer such a prompt itself (CodeFlow ADR-0075,
amendment of 2026-10-03). Every other case goes to the operator.

The caller answers "trust" only when all of these hold:

1. **The seat is Codex.** Codex's prompt grants hooks only. Grok's project
   trust also grants MCP and LSP servers, and Grok also reads other
   harnesses' hook files, so a Grok trust prompt always goes to the
   operator.
2. **The prompt lists only the project's Codex hooks file in `.codex/`.**
   In a linked worktree, Codex reads the main checkout's file. A hook from
   inline config, a plugin, or a user or global file goes to the operator.
3. **That file is unchanged from the target's immutable tip.** The fetch of
   the pull request's target succeeds,
   `tip=$(git rev-parse --verify "origin/<target>^{commit}")` resolves, the
   file is a regular file and not a symlink, and
   `git show "$tip:<path>" | cmp - <path>` passes. A failed fetch, an
   unresolved target, a file missing at the tip or any byte of difference
   means no trust.
4. **Every hook command is a form CodeFlow ships.** The file is also
   byte-identical to CodeFlow's managed copy under `.codeflow/.baseline/`,
   which only `codeflow update` writes. Each command is then, as a whole
   string, `codeflow hook <name> --contract <N>` followed by CodeFlow's
   shared missing-binary probe. These forms run only the installed
   `codeflow` binary, which `command -v codeflow` must resolve outside the
   repository, and system tools. A command that runs a repository script,
   an interpreter or a relative path is never eligible, whatever else it
   contains.

When all four hold, the hooks the seat would trust are CodeFlow's own guard
commands as reviewed and landed on the target, and they run no code from the
repository. That is the whole claim; the check does not review hook
behaviour.

Otherwise, tell the operator the seat is waiting at a hook trust prompt, in
its own surface: its Herdr tab, or its tmux pane under the fallback. Brief
the seat only after the operator answers. Never trust a changed or extra
hook, and never pick "continue without trusting" to get past the prompt.
