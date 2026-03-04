package githooks

import (
	"fmt"
	"os"
	"os/exec"
	"regexp"
	"strings"
)

// branchTypeRe matches branch names like feat/something, fix/something, etc.
// The types list is not hardcoded here — it comes from the policy. But we
// need a broad match to extract type and scope, then validate the type.
var branchTypeRe = regexp.MustCompile(`^([a-z]+)/(.+)$`)

// scopeRe extracts the first component from the slug part of the branch name.
// E.g., "inf-tsk-021-035-git-hooks" -> "inf".
var scopeRe = regexp.MustCompile(`^([a-z0-9]+)-`)

// RunPrepareCommitMsg prepares the commit message template by extracting
// the commit type and scope from the current branch name.
//
// It skips template generation when source is "message", "merge", "commit",
// or "squash".
func RunPrepareCommitMsg(msgFile, source string, policy *EnforcementPolicy) error {
	// Skip conditions.
	switch source {
	case "message", "merge", "commit", "squash":
		return nil
	}

	branch, _ := getCurrentBranch()
	// Branch may be empty in detached HEAD (e.g., CI merge refs). That's fine —
	// we still generate a generic template without type/scope prefill.

	commitType, commitScope := extractTypeAndScope(branch, policy)

	// Read current file content.
	content, err := os.ReadFile(msgFile)
	if err != nil {
		return fmt.Errorf("read commit message file: %w", err)
	}

	currentMsg := string(content)

	// Only generate template if file is empty or contains only git comments.
	if !isEmptyOrComments(currentMsg) {
		return nil
	}

	return writeTemplate(msgFile, branch, commitType, commitScope, policy)
}

func getCurrentBranch() (string, error) {
	cmd := exec.Command("git", "symbolic-ref", "HEAD")
	out, err := cmd.Output()
	if err != nil {
		return "", err
	}
	ref := strings.TrimSpace(string(out))
	return strings.TrimPrefix(ref, "refs/heads/"), nil
}

func extractTypeAndScope(branch string, policy *EnforcementPolicy) (string, string) {
	m := branchTypeRe.FindStringSubmatch(branch)
	if m == nil {
		return "", ""
	}

	branchType := m[1]
	slug := m[2]

	// Validate that the extracted type is a known commit type.
	validType := false
	for _, t := range policy.GitFormat.CommitTypes {
		if branchType == t {
			validType = true
			break
		}
	}
	if !validType {
		return "", ""
	}

	// Extract scope from slug.
	scope := ""
	sm := scopeRe.FindStringSubmatch(slug)
	if sm != nil {
		scope = sm[1]
	}

	return branchType, scope
}

func isEmptyOrComments(msg string) bool {
	for _, line := range strings.Split(msg, "\n") {
		trimmed := strings.TrimSpace(line)
		if trimmed != "" && !strings.HasPrefix(trimmed, "#") {
			return false
		}
	}
	return true
}

func writeTemplate(msgFile, branch, commitType, commitScope string, policy *EnforcementPolicy) error {
	var buf strings.Builder

	if commitType != "" {
		if commitScope != "" {
			fmt.Fprintf(&buf, "%s(%s): \n", commitType, commitScope)
		} else {
			fmt.Fprintf(&buf, "%s: \n", commitType)
		}
	} else {
		buf.WriteString("type: description\n")
	}

	buf.WriteString("\n")
	buf.WriteString("# Conventional commit format:\n")
	buf.WriteString("#   type(scope): description\n")
	buf.WriteString("#\n")

	types := policy.GitFormat.CommitTypes
	if len(types) > 0 {
		buf.WriteString("# Types: ")
		buf.WriteString(strings.Join(types, ", "))
		buf.WriteString("\n")
	}

	buf.WriteString("#\n")
	buf.WriteString("# Examples:\n")
	buf.WriteString("#   feat: add user authentication\n")
	buf.WriteString("#   fix(api): resolve null pointer exception\n")
	buf.WriteString("#   docs: update README\n")
	buf.WriteString("#\n")

	if branch != "" {
		fmt.Fprintf(&buf, "# Branch: %s\n", branch)
	}
	buf.WriteString("#\n")

	return os.WriteFile(msgFile, []byte(buf.String()), 0o644)
}
