// Package edit provides Edit/Write tool scope enforcement for CodeFlow
// PreToolUse hooks.
//
// ScopeChecker validates file paths from Claude Code hook JSON against
// enforcement-policy.json rules:
//
//   - Blocked directories (e.g., .git, node_modules)
//   - Allowed temporary prefixes (e.g., /tmp/claude/)
//   - Protected branch restrictions (no writes on main/master)
//   - Dangerous file extension warnings (binary, credential, archive)
//   - Project directory containment (no writes outside repo)
//
// Exit codes follow the Claude Code hook convention:
//
//   - 0: operation allowed
//   - 2: operation blocked
//
// This is the Go equivalent of cf-pre-tool-use-edit-write.sh.
package edit
