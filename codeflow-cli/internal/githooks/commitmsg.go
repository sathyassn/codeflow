package githooks

import (
	"errors"
	"fmt"
	"os"
	"regexp"
	"strings"
	"unicode"
)

// CommitMsgErrors collects all validation failures for a commit message.
type CommitMsgErrors struct {
	Errors []string
}

func (e *CommitMsgErrors) Error() string {
	return fmt.Sprintf("commit message validation failed with %d error(s)", len(e.Errors))
}

// add appends an error message.
func (e *CommitMsgErrors) add(msg string) {
	e.Errors = append(e.Errors, msg)
}

// RunCommitMsg validates the commit message file against enforcement policy.
// It returns nil if the message is valid, or a *CommitMsgErrors describing
// all validation failures.
func RunCommitMsg(msgFile string, policy *EnforcementPolicy) error {
	raw, err := os.ReadFile(msgFile)
	if err != nil {
		return fmt.Errorf("read commit message file: %w", err)
	}

	return ValidateCommitMsg(string(raw), policy)
}

// ValidateCommitMsg validates a commit message string against the policy.
func ValidateCommitMsg(rawMsg string, policy *EnforcementPolicy) error {
	// Strip git comment lines (lines starting with #).
	var cleaned []string
	for _, line := range strings.Split(rawMsg, "\n") {
		if !strings.HasPrefix(line, "#") {
			cleaned = append(cleaned, line)
		}
	}
	commitMsg := strings.Join(cleaned, "\n")

	if strings.TrimSpace(commitMsg) == "" {
		return errors.New("commit message is empty")
	}

	lines := strings.Split(commitMsg, "\n")
	subject := lines[0]

	// --- Skip conditions ---
	if isMergeCommit(subject) || isRevertCommit(subject) || isFixupSquash(subject) {
		return nil
	}

	errs := &CommitMsgErrors{}

	// --- AI attribution blocking ---
	checkAIAttribution(commitMsg, policy, errs)

	// --- Subject validation ---
	checkSubjectLength(subject, policy, errs)
	checkSubjectFormat(subject, policy, errs)
	checkTrailingPeriod(subject, policy, errs)
	checkLowercaseType(subject, policy, errs)

	// --- Body validation ---
	if len(lines) > 1 {
		checkBody(lines, policy, errs)
	}

	if len(errs.Errors) > 0 {
		return errs
	}
	return nil
}

func isMergeCommit(subject string) bool {
	return strings.HasPrefix(subject, "Merge pull request #") ||
		strings.HasPrefix(subject, "Merge branch ")
}

func isRevertCommit(subject string) bool {
	return strings.HasPrefix(subject, "Revert ")
}

func isFixupSquash(subject string) bool {
	return strings.HasPrefix(subject, "fixup!") || strings.HasPrefix(subject, "squash!")
}

func checkAIAttribution(msg string, policy *EnforcementPolicy, errs *CommitMsgErrors) {
	lower := strings.ToLower(msg)
	for _, pattern := range policy.GitFormat.AIAttributionPatterns {
		if strings.Contains(lower, strings.ToLower(pattern)) {
			errs.add(fmt.Sprintf("AI attribution detected: pattern '%s'", pattern))
			return // Report only the first match.
		}
	}
}

func checkSubjectLength(subject string, policy *EnforcementPolicy, errs *CommitMsgErrors) {
	maxLen := policy.GitFormat.Subject.MaxLength
	if maxLen <= 0 {
		maxLen = 50
	}
	if len(subject) > maxLen {
		errs.add(fmt.Sprintf("subject line too long: %d chars (max: %d)", len(subject), maxLen))
	}
}

func checkSubjectFormat(subject string, policy *EnforcementPolicy, errs *CommitMsgErrors) {
	types := policy.GitFormat.CommitTypes
	if len(types) == 0 {
		return
	}
	// Build regex: ^(type1|type2|...): .+$
	// Scoped types like feat(api) are intentionally rejected to match original shell behavior.
	pattern := "^(" + strings.Join(escapeRegexTypes(types), "|") + "): .+$"
	re, err := regexp.Compile(pattern)
	if err != nil {
		return
	}
	if !re.MatchString(subject) {
		errs.add(fmt.Sprintf("invalid commit format: '%s'. Required: type: description. Valid types: %s",
			subject, strings.Join(types, ", ")))
	}
}

func escapeRegexTypes(types []string) []string {
	out := make([]string, len(types))
	for i, t := range types {
		out[i] = regexp.QuoteMeta(t)
	}
	return out
}

func checkTrailingPeriod(subject string, policy *EnforcementPolicy, errs *CommitMsgErrors) {
	if policy.GitFormat.Subject.ForbidTrailingPeriod && strings.HasSuffix(subject, ".") {
		errs.add("subject line should not end with a period")
	}
}

func checkLowercaseType(subject string, policy *EnforcementPolicy, errs *CommitMsgErrors) {
	if !policy.GitFormat.Subject.RequireLowercaseType {
		return
	}
	if len(subject) > 0 && unicode.IsUpper(rune(subject[0])) {
		errs.add("type should be lowercase")
	}
}

func checkBody(lines []string, policy *EnforcementPolicy, errs *CommitMsgErrors) {
	// Extract body lines starting from line index 1.
	bodyLines := lines[1:]

	// Check blank line after subject.
	if len(bodyLines) > 0 && strings.TrimSpace(bodyLines[0]) != "" {
		errs.add("missing blank line after subject")
	}

	// Collect non-empty body content (skip blank lines for content checks).
	var contentLines []string
	for _, l := range bodyLines {
		if strings.TrimSpace(l) != "" {
			contentLines = append(contentLines, l)
		}
	}

	if len(contentLines) == 0 {
		return
	}

	// Detect GitHub squash-merge asterisk bullets.
	for _, l := range contentLines {
		if strings.HasPrefix(l, "* ") {
			errs.add("GitHub squash-merge format detected ('* ' prefix). Use '- ' prefix bullets")
			break
		}
	}

	// Check bullets-only format.
	var nonBullet []string
	for _, l := range contentLines {
		if !strings.HasPrefix(l, "- ") {
			nonBullet = append(nonBullet, l)
		}
	}
	if len(nonBullet) > 0 {
		errs.add(fmt.Sprintf("body must contain only bullet points (lines starting with '- '). Found non-bullet lines: %s",
			strings.Join(nonBullet, "; ")))
	}

	// Count bullets.
	maxBullets := policy.GitFormat.Body.MaxBullets
	if maxBullets <= 0 {
		maxBullets = 3
	}
	bulletCount := 0
	for _, l := range contentLines {
		if strings.HasPrefix(l, "- ") {
			bulletCount++
		}
	}
	if bulletCount > maxBullets {
		errs.add(fmt.Sprintf("too many bullet points: %d (max: %d)", bulletCount, maxBullets))
	}

	// Check blank lines between bullets.
	checkBlankBetweenBullets(bodyLines, errs)

	// Check body line length.
	maxLineLen := policy.GitFormat.Body.LineMaxLength
	if maxLineLen <= 0 {
		maxLineLen = 72
	}
	for _, l := range contentLines {
		if len(l) > maxLineLen {
			errs.add(fmt.Sprintf("body line too long: %d chars (max: %d): %s",
				len(l), maxLineLen, l))
		}
	}
}

func checkBlankBetweenBullets(bodyLines []string, errs *CommitMsgErrors) {
	inBullets := false
	foundBlank := false
	for _, line := range bodyLines {
		if strings.HasPrefix(line, "- ") {
			if foundBlank && inBullets {
				errs.add("blank lines between bullets: bullets must be contiguous")
				return
			}
			inBullets = true
			foundBlank = false
		} else if strings.TrimSpace(line) == "" && inBullets {
			foundBlank = true
		}
	}
}
