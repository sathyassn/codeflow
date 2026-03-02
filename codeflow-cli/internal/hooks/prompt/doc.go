// Package prompt provides user-prompt-submit validation for CodeFlow hooks.
//
// PromptValidator outputs context reminders when a user submits a prompt,
// including:
//
//   - Uncommitted git changes count and branch name
//   - Protected branch warnings
//   - Active task reminders (from .state/runtime/active-task.json)
//   - PathFlow mode coordination reminders
//
// Each reminder is wrapped in <user-prompt-submit-hook> tags for the
// Claude Code hook framework.
//
// This hook never blocks (always exits 0). It is the Go equivalent of
// cf-user-prompt-submit.sh (validation/context sections only; logging
// is handled separately by the logging package).
package prompt
