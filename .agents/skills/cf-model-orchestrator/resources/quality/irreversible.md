## Catastrophic and irreversible actions

For a catastrophic or irreversible action, the ledger also records independent
Claude and Codex risk assessments, the authenticated human approval, exact
scope and command/tool input, preview or dry-run evidence when supported, the
current checkpoint/backup and tested restore path, execution result, and
postcondition verification. Model consensus and automatic safety review never
stand in for the human approval. Ordinary recoverable worktree edits and
deletions do not require this ceremony. An operation in CodeFlow's
non-relaxable deterministic class is performed by the human operator through a
separate controlled channel; the models record the operator's result and verify
the postcondition without weakening the guard.
