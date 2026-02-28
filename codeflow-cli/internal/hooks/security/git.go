package security

import (
	"fmt"
	"regexp"
	"strings"
)

// GitModule validates git commands for security violations.
// Go equivalent of cf-git-protection.sh (Sections 1-5).
type GitModule struct{}

// Name returns the module name.
func (m *GitModule) Name() string { return "git-protection" }

// Section 1: Git hook bypass patterns.
var (
	// --no-verify on git subcommands
	gitNoVerify = regexp.MustCompile(`git\s+(commit|push|rebase|cherry-pick|merge|am)\s+.*--no-verify`)
	// --no-verify at git level (before subcommand)
	gitLevelNoVerify = regexp.MustCompile(`git\s+--no-verify`)
	// git commit start
	gitCommit = regexp.MustCompile(`^git\s+commit\s`)
	// -n flag patterns for git commit
	standaloneN = regexp.MustCompile(`\s-n($|\s)`)
	combinedN   = regexp.MustCompile(`\s-[a-mo-z]*n[a-mo-z]*($|\s)`)
)

// Section 2: Force push patterns.
var (
	gitPushForce     = regexp.MustCompile(`git\s+push\s+.*--force($|\s)`)
	gitPushForceLease = regexp.MustCompile(`git\s+push\s+.*--force-with-lease($|\s)`)
	gitPushF         = regexp.MustCompile(`git\s+push\s+(.*\s)?-f($|\s)`)
)

// Section 3: Hook path manipulation.
var (
	gitConfigHooksPath = regexp.MustCompile(`git\s+config.*core\.hooksPath`)
	gitCHooksPath      = regexp.MustCompile(`git\s+-c\s+core\.hooksPath`)
	gitUnsetHooksPath  = regexp.MustCompile(`git\s+config.*--unset.*core\.hooksPath`)
	huskyBypass        = regexp.MustCompile(`HUSKY\s*=\s*0`)
)

// Section 4: .git/hooks directory protection.
var (
	gitHooksModify   = regexp.MustCompile(`(rm|mv|chmod|chown)\s.*\.git/hooks`)
	gitHooksRedirect = regexp.MustCompile(`>\s*\.git/hooks`)
)

// Section 5: Protected branch operations.
var (
	gitMerge      = regexp.MustCompile(`^git\s+merge(\s|$)`)
	gitCherryPick = regexp.MustCompile(`^git\s+cherry-pick(\s|$)`)
	gitRebase     = regexp.MustCompile(`^git\s+rebase(\s|$)`)
	gitReset      = regexp.MustCompile(`^git\s+reset(\s|$)`)

	// Chained checkout/switch + merge to protected branch
	gitCheckoutMerge = regexp.MustCompile(`git\s+checkout\s+(main|master|production)\s*(&&|;|\|)\s*git\s+(merge|cherry-pick|rebase|reset)`)
	gitSwitchMerge   = regexp.MustCompile(`git\s+switch\s+(main|master|production)\s*(&&|;|\|)\s*git\s+(merge|cherry-pick|rebase|reset)`)
)

// Check evaluates git commands for security violations.
func (m *GitModule) Check(ctx *CheckContext) *Verdict {
	cmd := ctx.Command

	// Section 1: Hook bypass prevention
	if v := checkHookBypass(cmd); v != nil {
		return v
	}

	// Section 2: Force push prevention (protected branches only)
	if v := checkForcePush(cmd, ctx); v != nil {
		return v
	}

	// Section 3: Hook path manipulation
	if v := checkHookPathManipulation(cmd); v != nil {
		return v
	}

	// Section 4: .git/hooks directory protection
	if v := checkGitHooksDir(cmd); v != nil {
		return v
	}

	// Section 5: Protected branch operations
	if v := checkProtectedBranchOps(cmd, ctx); v != nil {
		return v
	}

	return nil
}

func checkHookBypass(cmd string) *Verdict {
	if gitNoVerify.MatchString(cmd) {
		return block("Git Hook Bypass", "Hook bypass flag detected", "--no-verify")
	}
	if gitLevelNoVerify.MatchString(cmd) {
		return block("Git Hook Bypass", "Hook bypass flag at git level", "git --no-verify")
	}

	// -n flag for git commit only (NOT git push where -n = --dry-run)
	if gitCommit.MatchString(cmd) {
		flags := getFlagsPortion(cmd)
		if standaloneN.MatchString(flags) {
			return block("Git Hook Bypass", "Hook bypass flag (short form)", "-n")
		}
		if combinedN.MatchString(flags) {
			return block("Git Hook Bypass", "Hook bypass in combined flags", "-n in combined flags")
		}
	}

	return nil
}

func checkForcePush(cmd string, ctx *CheckContext) *Verdict {
	branch := ctx.CurrentBranch
	if !isOnProtectedBranch(branch, ctx.Policy) {
		return nil
	}

	if gitPushForce.MatchString(cmd) {
		return block("Force Push", fmt.Sprintf("Force push to protected branch '%s'", branch), "--force")
	}
	if gitPushForceLease.MatchString(cmd) {
		return block("Force Push", fmt.Sprintf("Force push (with lease) to protected branch '%s'", branch), "--force-with-lease")
	}
	if gitPushF.MatchString(cmd) {
		return block("Force Push", fmt.Sprintf("Force push (short form) to protected branch '%s'", branch), "-f")
	}

	return nil
}

func checkHookPathManipulation(cmd string) *Verdict {
	if gitConfigHooksPath.MatchString(cmd) {
		return block("Hook Manipulation", "Hook path modification attempt", "core.hooksPath")
	}
	if gitCHooksPath.MatchString(cmd) {
		return block("Hook Manipulation", "Hook path override via -c flag", "-c core.hooksPath")
	}
	if gitUnsetHooksPath.MatchString(cmd) {
		return block("Hook Manipulation", "Hook path unset attempt", "--unset core.hooksPath")
	}

	if strings.Contains(cmd, "GIT_HOOKS_PATH") {
		return block("Hook Manipulation", "Hook path env var", "GIT_HOOKS_PATH")
	}
	if strings.Contains(cmd, "SKIP_HOOKS") {
		return block("Hook Manipulation", "Hook skip env var", "SKIP_HOOKS")
	}
	if strings.Contains(cmd, "GIT_SKIP_HOOKS") {
		return block("Hook Manipulation", "Hook skip env var", "GIT_SKIP_HOOKS")
	}

	if huskyBypass.MatchString(cmd) {
		return block("Hook Manipulation", "Husky bypass attempt", "HUSKY=0")
	}
	if strings.Contains(cmd, "PRE_COMMIT_ALLOW_NO_CONFIG") {
		return block("Hook Manipulation", "pre-commit bypass attempt", "PRE_COMMIT_ALLOW_NO_CONFIG")
	}

	return nil
}

func checkGitHooksDir(cmd string) *Verdict {
	if gitHooksModify.MatchString(cmd) {
		return block("Hook Manipulation", "Direct .git/hooks modification", ".git/hooks")
	}
	if gitHooksRedirect.MatchString(cmd) {
		return block("Hook Manipulation", "Redirect to .git/hooks", "> .git/hooks")
	}
	return nil
}

func checkProtectedBranchOps(cmd string, ctx *CheckContext) *Verdict {
	branch := ctx.CurrentBranch

	if isOnProtectedBranch(branch, ctx.Policy) {
		if gitMerge.MatchString(cmd) {
			return block("Protected Branch", fmt.Sprintf("Merge to protected branch '%s' blocked", branch), "git merge on "+branch)
		}
		if gitCherryPick.MatchString(cmd) {
			return block("Protected Branch", fmt.Sprintf("Cherry-pick to protected branch '%s' blocked", branch), "git cherry-pick on "+branch)
		}
		if gitRebase.MatchString(cmd) {
			return block("Protected Branch", fmt.Sprintf("Rebase on protected branch '%s' blocked", branch), "git rebase on "+branch)
		}
		if gitReset.MatchString(cmd) {
			return block("Protected Branch", fmt.Sprintf("Reset on protected branch '%s' blocked", branch), "git reset on "+branch)
		}
	}

	// Chained checkout/switch + merge (checks for specific branch names)
	if gitCheckoutMerge.MatchString(cmd) {
		return block("Protected Branch", "Chained checkout+merge to protected branch blocked", "checkout && merge")
	}
	if gitSwitchMerge.MatchString(cmd) {
		return block("Protected Branch", "Chained switch+merge to protected branch blocked", "switch && merge")
	}

	return nil
}

// isOnProtectedBranch checks if the given branch is in the protected list.
func isOnProtectedBranch(branch string, policy *EnforcementPolicy) bool {
	if branch == "" {
		return false
	}

	var branches []string
	if policy != nil {
		branches = policy.ProtectedBranchList()
	} else {
		branches = []string{"main", "master", "release/*", "production"}
	}

	for _, pb := range branches {
		if strings.Contains(pb, "*") {
			// Wildcard pattern: convert to regex
			pattern := "^" + strings.ReplaceAll(regexp.QuoteMeta(pb), `\*`, `.*`) + "$"
			if matched, _ := regexp.MatchString(pattern, branch); matched {
				return true
			}
		} else if branch == pb {
			return true
		}
	}
	return false
}
