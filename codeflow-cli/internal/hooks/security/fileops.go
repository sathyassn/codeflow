package security

import (
	"regexp"
	"strings"
)

// FileOpsModule validates file operations for security violations.
// Go equivalent of cf-file-operations.sh (Sections 9-11).
type FileOpsModule struct{}

// Name returns the module name.
func (m *FileOpsModule) Name() string { return "file-operations" }

// Indirect write commands pattern.
var indirectWriteCmds = regexp.MustCompile(`(^|\s|/)(cp|dd|tee|rsync|scp|install|ln)\s`)

// Interpreter patterns for inline code detection.
var interpreterCmd = regexp.MustCompile(`(^|\s)(python3?|perl|ruby|node)\s+-[ce]\s`)

// Constant patterns for indirect file ops checks.
var (
	tmpClaudeFinalDest = regexp.MustCompile(`\s/tmp/claude($|/)\S*\s*$`)
	tmpClaudeRedirect  = regexp.MustCompile(`>/tmp/claude($|/)`)
	ddCmd              = regexp.MustCompile(`(^|\s)(dd)\s`)
	pipedTee           = regexp.MustCompile(`\|\s*(tee)\s`)
	catAppend          = regexp.MustCompile(`cat\s.*>>\s*`)
)

// Interpreter-protected patterns (paths that shouldn't be written via interpreters).
var interpreterProtectedPatterns = []string{
	".claude/",
	".state/",
	".codeflow/config/",
	"settings.json",
	"enforcement-policy",
}

// Check evaluates the command for file operation violations.
func (m *FileOpsModule) Check(ctx *CheckContext) *Verdict {
	cmd := ctx.Command

	paths := protectedPaths(ctx.Policy)
	if len(paths) == 0 {
		return nil
	}

	// Section 9: Indirect file operations
	if v := checkIndirectFileOps(cmd, paths); v != nil {
		return v
	}

	// Section 10: Glob pattern bypass prevention
	if v := checkGlobBypass(cmd, paths); v != nil {
		return v
	}

	// Section 11: Interpreter-based file write detection
	if v := checkInterpreterWrite(cmd); v != nil {
		return v
	}

	return nil
}

func checkIndirectFileOps(cmd string, paths []string) *Verdict {
	// Skip Section 9 if the final destination is /tmp/claude
	skipSection9 := false
	if tmpClaudeFinalDest.MatchString(cmd) &&
		!strings.Contains(cmd, "&&") && !strings.Contains(cmd, "||") && !strings.Contains(cmd, ";") {
		skipSection9 = true
	}
	if tmpClaudeRedirect.MatchString(cmd) {
		skipSection9 = true
	}

	if !skipSection9 {
		for _, path := range paths {
			if isPathOrGlobTargeted(cmd, path) {
				if indirectWriteCmds.MatchString(cmd) {
					return block("Protected Path Write", "Indirect write operation targeting protected path", path)
				}
			}
		}
	}

	// dd command - special handling for of= parameter
	if ddCmd.MatchString(cmd) {
		for _, path := range paths {
			if strings.Contains(cmd, "of=") && strings.Contains(cmd, path) {
				return block("Protected Path Copy", "dd operation targeting protected path", path)
			}
		}
	}

	// Piped tee to protected paths
	if pipedTee.MatchString(cmd) {
		for _, path := range paths {
			if isPathOrGlobTargeted(cmd, path) {
				return block("Protected Path Pipe", "Piped tee to protected path", path)
			}
		}
	}

	// Cat append redirection to protected paths
	if catAppend.MatchString(cmd) {
		for _, path := range paths {
			if appendRe, err := regexp.Compile(`>>\s*` + regexp.QuoteMeta(path) + `($|[\s"'/])`); err == nil && appendRe.MatchString(cmd) {
				return block("Protected Path Append", "cat append redirection to protected path", path)
			}
		}
	}

	return nil
}

func checkGlobBypass(cmd string, paths []string) *Verdict {
	if !indirectWriteCmds.MatchString(cmd) {
		return nil
	}
	if !strings.Contains(cmd, "*") && !strings.Contains(cmd, "?") {
		return nil
	}

	for _, path := range paths {
		// Check ? glob matching
		if len(path) >= 2 {
			pathMinusOne := path[:len(path)-1]
			if strings.Contains(cmd, pathMinusOne+"?") {
				return block("Glob Bypass Attempt", "Glob pattern '?' could match protected path", path)
			}
		}

		// Check * glob matching
		prefix := path
		for len(prefix) >= 5 {
			if strings.Contains(cmd, prefix) {
				// Check if * follows the prefix
				re, err := regexp.Compile(regexp.QuoteMeta(prefix) + `[^"/\s]*\*`)
				if err == nil && re.MatchString(cmd) {
					return block("Glob Bypass Attempt", "Glob pattern '*' could match protected path", path)
				}
			}
			prefix = prefix[:len(prefix)-1]
		}
	}

	return nil
}

func checkInterpreterWrite(cmd string) *Verdict {
	if !interpreterCmd.MatchString(cmd) {
		return nil
	}

	for _, pattern := range interpreterProtectedPatterns {
		if strings.Contains(cmd, pattern) {
			return block("Interpreter File Write",
				"Interpreter-based file write to protected path detected. Use Edit/Write tools instead.",
				pattern)
		}
	}

	return nil
}
