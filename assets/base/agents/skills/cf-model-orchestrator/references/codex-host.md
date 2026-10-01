# Codex host

From a Codex host, this test-running review uses a separate interactive Claude
session in auto mode under the same fail-closed sandbox (not plan or bypass
mode) so Bash/UI verification can proceed without an unattended permission
stall. Keep shell classification enabled, grant only the scoped test and
inspection actions, explicitly prohibit source edits, and require the worktree
diff to remain unchanged after review. This is verification authority, not an
implementation handoff.
