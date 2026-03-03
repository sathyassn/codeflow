package githooks

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// PrePushResult describes the outcome of the pre-push check.
type PrePushResult struct {
	Allowed bool
	Message string
}

// RunPrePush reads pushed refs from stdin (git's pipe format), checks each
// against protected branches, detects force-pushes, validates branch naming,
// and supports TTY override for interactive users.
//
// It returns an error if the push should be blocked.
func RunPrePush(stdin io.Reader, stdout, stderr io.Writer, remote, url, projectDir string, policy *EnforcementPolicy) error {
	scanner := bufio.NewScanner(stdin)
	for scanner.Scan() {
		line := scanner.Text()
		fields := strings.Fields(line)
		if len(fields) < 4 {
			continue
		}

		localRef := fields[0]
		localSHA := fields[1]
		remoteRef := fields[2]
		remoteSHA := fields[3]

		_ = localRef // Part of git protocol, not used directly.

		// Skip delete operations.
		if localSHA == zeroSHA {
			continue
		}

		branch := strings.TrimPrefix(remoteRef, "refs/heads/")

		if !isProtectedBranch(branch, policy.ProtectedBranches) {
			continue
		}

		// Protected branch — block push.
		fmt.Fprintln(stderr)
		fmt.Fprintf(stderr, "\033[0;31mERROR: Push to protected branch '%s' blocked!\033[0m\n", branch)
		fmt.Fprintln(stderr)
		fmt.Fprintln(stderr, "This repository uses a PR-only workflow.")
		fmt.Fprintf(stderr, "Direct pushes to '%s' are not allowed.\n", branch)
		fmt.Fprintln(stderr)

		if isForcePush(localSHA, remoteSHA) {
			fmt.Fprintf(stderr, "\033[0;31mFORCE PUSH DETECTED\033[0m\n")
			fmt.Fprintln(stderr)
			fmt.Fprintf(stderr, "Force pushing to '%s' would rewrite history on a protected branch.\n", branch)
			fmt.Fprintln(stderr, "This breaks collaboration and causes data loss for other users.")
			fmt.Fprintln(stderr)
		}

		// Provide remediation guidance.
		if isPathFlowActive(projectDir) {
			fmt.Fprintln(stderr, "MUST: Message the cf-git-operations teammate to create a feature branch and pull request.")
			fmt.Fprintln(stderr, "Do NOT push directly to protected branches.")
		} else {
			fmt.Fprintln(stderr, "MUST: Delegate to cf-git-operations teammate for branch and PR creation.")
			fmt.Fprintln(stderr, `SendMessage(recipient="cf-git-operations", content="create-branch then create-pull-request")`)
			fmt.Fprintln(stderr, "Do NOT push directly to protected branches.")
		}
		fmt.Fprintln(stderr)

		// TTY override for interactive users.
		if tryTTYOverride(stderr, branch, projectDir) {
			continue // Override accepted for this branch.
		}

		return fmt.Errorf("push to protected branch '%s' blocked", branch)
	}

	if err := scanner.Err(); err != nil {
		return fmt.Errorf("read push refs: %w", err)
	}

	// Branch name validation.
	currentBranch, err := gitOutput("branch", "--show-current")
	if err != nil {
		// Detached HEAD or other issue — skip validation.
		fmt.Fprintf(stdout, "\033[0;32mPre-push checks passed!\033[0m\n")
		return nil
	}

	if currentBranch == "main" || currentBranch == "master" {
		fmt.Fprintf(stdout, "\033[0;32mPre-push checks passed!\033[0m\n")
		return nil
	}

	validPrefix := false
	for _, prefix := range policy.GitFormat.BranchPrefixes {
		if strings.HasPrefix(currentBranch, prefix) {
			validPrefix = true
			break
		}
	}

	if !validPrefix {
		fmt.Fprintf(stderr, "\033[1;33mWarning: Branch name '%s' doesn't follow naming convention.\033[0m\n", currentBranch)
		fmt.Fprintln(stderr)
		fmt.Fprintln(stderr, "Recommended prefixes:")
		for _, p := range policy.GitFormat.BranchPrefixes {
			fmt.Fprintf(stderr, "  - %s\n", p)
		}
		fmt.Fprintln(stderr)
		// Warning only — don't block.
	}

	fmt.Fprintf(stdout, "\033[0;32mPre-push checks passed!\033[0m\n")
	return nil
}

const zeroSHA = "0000000000000000000000000000000000000000"

func isProtectedBranch(branch string, protected []string) bool {
	for _, pattern := range protected {
		if matchBranchPattern(branch, pattern) {
			return true
		}
	}
	return false
}

// matchBranchPattern matches a branch name against a pattern that may
// contain a trailing wildcard (e.g., "release/*").
func matchBranchPattern(branch, pattern string) bool {
	if strings.HasSuffix(pattern, "/*") {
		prefix := strings.TrimSuffix(pattern, "/*")
		return strings.HasPrefix(branch, prefix+"/")
	}
	return branch == pattern
}

func isForcePush(localSHA, remoteSHA string) bool {
	if localSHA == zeroSHA || remoteSHA == zeroSHA {
		return false
	}
	cmd := exec.Command("git", "merge-base", "--is-ancestor", remoteSHA, localSHA)
	err := cmd.Run()
	// If merge-base exits 0, it IS an ancestor (fast-forward, NOT force push).
	// If it exits non-zero, it is NOT an ancestor (force push).
	return err != nil
}

func isPathFlowActive(projectDir string) bool {
	pattern := filepath.Join(projectDir, ".state", "session", "*", "pathflow", "is-pathflow-active")
	matches, _ := filepath.Glob(pattern)
	return len(matches) > 0
}

// tryTTYOverride attempts to open /dev/tty for interactive override.
// Returns true if the user confirmed the override.
func tryTTYOverride(stderr io.Writer, branch, projectDir string) bool {
	tty, err := os.Open("/dev/tty")
	if err != nil {
		return false // Not interactive.
	}
	defer tty.Close()

	fmt.Fprintf(stderr, "\033[1;33m===================================================\033[0m\n")
	fmt.Fprintf(stderr, "\033[1;33m  Emergency Override Available\033[0m\n")
	fmt.Fprintf(stderr, "\033[1;33m===================================================\033[0m\n")
	fmt.Fprintln(stderr)
	fmt.Fprintf(stderr, "If you ABSOLUTELY must push to '%s' (e.g., emergency hotfix):\n", branch)
	fmt.Fprintln(stderr)
	fmt.Fprint(stderr, "Type 'I understand the risks' to proceed: ")

	reader := bufio.NewReader(tty)
	reply, _ := reader.ReadString('\n')
	reply = strings.TrimSpace(reply)
	fmt.Fprintln(stderr)

	if reply == "I understand the risks" {
		fmt.Fprintf(stderr, "\033[1;33mOverride confirmed\033[0m\n")
		fmt.Fprintln(stderr)
		fmt.Fprintf(stderr, "Proceeding with push to '%s'...\n", branch)
		fmt.Fprintln(stderr, "This action has been logged.")
		fmt.Fprintln(stderr)
		logOverride(branch, projectDir)
		return true
	}

	fmt.Fprintf(stderr, "\033[0;31mOverride phrase incorrect or declined.\033[0m\n")
	fmt.Fprintln(stderr)
	return false
}

func logOverride(branch, projectDir string) {
	overrideLog := filepath.Join(projectDir, ".git", "push-overrides.log")
	f, err := os.OpenFile(overrideLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()

	user, _ := gitOutput("config", "user.name")
	email, _ := gitOutput("config", "user.email")
	commit, _ := gitOutput("rev-parse", "HEAD")

	fmt.Fprintln(f, "===================================================")
	fmt.Fprintln(f, "Emergency Push Override")
	fmt.Fprintf(f, "Date: %s\n", time.Now().Format("2006-01-02 15:04:05 MST"))
	fmt.Fprintf(f, "User: %s <%s>\n", user, email)
	fmt.Fprintf(f, "Branch: %s\n", branch)
	fmt.Fprintf(f, "Commit: %s\n", commit)
	fmt.Fprintln(f, "===================================================")
	fmt.Fprintln(f)
}
