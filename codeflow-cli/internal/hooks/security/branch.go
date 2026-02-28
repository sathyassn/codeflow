package security

import (
	"regexp"
	"strings"
)

// BranchModule blocks file writes on protected branches.
// Go equivalent of cf-branch-file-protection.sh.
type BranchModule struct{}

// Name returns the module name.
func (m *BranchModule) Name() string { return "branch-file-protection" }

// Branch file write patterns.
var (
	// Redirect to file (not fd redirect like 2>&1)
	redirectToFile = regexp.MustCompile(`([^0-9&>]|^)>\s*[^>&\s]`)
	// Append redirect
	appendToFile = regexp.MustCompile(`([^0-9&]|^)>>\s*[^>&\s]`)
	// sed -i (in-place edit)
	sedInplace = regexp.MustCompile(`sed\s.*-i`)
	// touch for in-project files (relative path)
	touchRelative = regexp.MustCompile(`(^|\s)touch\s+[^-/]`)
	// tee for in-project files (piped)
	teeRelative = regexp.MustCompile(`\|\s*tee\s+[^-/]`)
	// cp command
	cpCommand = regexp.MustCompile(`(^|\s)cp\s`)

	// Tmp exclusion patterns for branch protection
	tmpRedirect = regexp.MustCompile(`>\s*/tmp/`)
	tmpAppend   = regexp.MustCompile(`>>\s*/tmp/`)
	touchTmp    = regexp.MustCompile(`touch\s+/tmp`)
	teeTmp      = regexp.MustCompile(`tee\s+/tmp`)
)

// Check evaluates the command for branch file protection violations.
func (m *BranchModule) Check(ctx *CheckContext) *Verdict {
	cmd := ctx.Command
	branch := ctx.CurrentBranch

	// Skip if not on a protected branch
	if !isOnProtectedBranch(branch, ctx.Policy) {
		return nil
	}

	// Skip if all write targets are under /tmp/claude/ (safe scratch space)
	if strings.Contains(cmd, "/tmp/claude/") && !cpCommand.MatchString(cmd) {
		return nil
	}

	// Skip if targeting a protected path (let PathModule handle it)
	paths := protectedPaths(ctx.Policy)
	for _, path := range paths {
		if strings.Contains(cmd, path) {
			return nil
		}
	}

	// Block redirect operations (skip if redirecting to /tmp/)
	if redirectToFile.MatchString(cmd) {
		if !tmpRedirect.MatchString(cmd) {
			return block("Branch Protection", "File redirect on protected branch ("+branch+").", ">")
		}
	}
	if appendToFile.MatchString(cmd) {
		if !tmpAppend.MatchString(cmd) {
			return block("Branch Protection", "File append on protected branch ("+branch+").", ">>")
		}
	}

	// Block sed -i
	if sedInplace.MatchString(cmd) {
		return block("Branch Protection", "In-place file edit on protected branch ("+branch+").", "sed -i")
	}

	// Block touch for in-project files
	if touchRelative.MatchString(cmd) {
		if !touchTmp.MatchString(cmd) {
			return block("Branch Protection", "File creation on protected branch ("+branch+").", "touch")
		}
	}

	// Block tee for in-project files
	if teeRelative.MatchString(cmd) {
		if !teeTmp.MatchString(cmd) {
			return block("Branch Protection", "Piped file write on protected branch ("+branch+").", "tee")
		}
	}

	// Block cp when destination is relative (in-project)
	if cpCommand.MatchString(cmd) {
		// Get last space-delimited token as destination
		fields := strings.Fields(cmd)
		if len(fields) > 0 {
			dest := fields[len(fields)-1]
			if !strings.HasPrefix(dest, "/") && dest != "" {
				return block("Branch Protection", "File copy on protected branch ("+branch+").", "cp")
			}
		}
	}

	return nil
}
