package edit

import (
	"encoding/json"
	"fmt"
	"io"
	"path/filepath"
	"strings"
)

// Verdict is the result of a scope check.
type Verdict struct {
	Allow   bool
	Message string // stderr message (block reason or warning)
}

// DangerousExtensions groups file extensions by risk category.
type DangerousExtensions struct {
	Binary     []string
	Credential []string
	Archive    []string
}

// ScopeChecker validates Edit/Write file paths against enforcement rules.
type ScopeChecker struct {
	ProjectDir         string
	BlockedDirs        []string
	AllowedTmpPrefixes []string
	DangerousExts      DangerousExtensions
	WarnOnDangerous    bool
	ProtectedBranches  []string
	CurrentBranch      string
}

// hookInput is the Claude Code hook JSON structure.
type hookInput struct {
	ToolName  string          `json:"tool_name"`
	ToolInput json.RawMessage `json:"tool_input"`
}

// toolInput extracts the file_path from tool_input.
type toolInput struct {
	FilePath string `json:"file_path"`
}

// DefaultBlockedDirs returns the default list of blocked directories.
func DefaultBlockedDirs() []string {
	return []string{".git", "node_modules", "__pycache__", ".venv", "venv"}
}

// DefaultAllowedTmpPrefixes returns the default allowed temp prefixes.
func DefaultAllowedTmpPrefixes() []string {
	return []string{"/tmp/claude/", "/tmp/"}
}

// DefaultDangerousExtensions returns the default dangerous extension lists.
func DefaultDangerousExtensions() DangerousExtensions {
	return DangerousExtensions{
		Binary:     []string{"exe", "dll", "so", "dylib", "bin", "o", "a"},
		Credential: []string{"pem", "key", "crt", "p12", "pfx", "keystore", "jks"},
		Archive:    []string{"zip", "tar", "gz", "rar", "7z"},
	}
}

// DefaultProtectedBranches returns the default protected branch list.
func DefaultProtectedBranches() []string {
	return []string{"main", "master"}
}

// Check reads hook JSON from stdin and returns a verdict.
func (c *ScopeChecker) Check(stdin io.Reader) (*Verdict, error) {
	data, err := io.ReadAll(stdin)
	if err != nil {
		return allow(), nil
	}

	if len(data) == 0 {
		return allow(), nil
	}

	var input hookInput
	if err := json.Unmarshal(data, &input); err != nil {
		return allow(), nil
	}

	// Only check Edit and Write tools.
	if input.ToolName != "Edit" && input.ToolName != "Write" {
		return allow(), nil
	}

	var ti toolInput
	if err := json.Unmarshal(input.ToolInput, &ti); err != nil {
		return allow(), nil
	}

	if ti.FilePath == "" {
		return allow(), nil
	}

	return c.checkPath(input.ToolName, ti.FilePath), nil
}

// checkPath validates a file path against all scope rules.
func (c *ScopeChecker) checkPath(toolName, filePath string) *Verdict {
	// 1. Check allowed tmp prefixes first (bypass all other checks).
	if c.isAllowedTmp(filePath) {
		return allow()
	}

	// 2. Check protected branch.
	if c.CurrentBranch != "" && c.isProtectedBranch(c.CurrentBranch) {
		return block(toolName, filePath,
			fmt.Sprintf("Cannot write files directly on protected branch '%s'. Create a feature branch first.", c.CurrentBranch))
	}

	// 3. Resolve to absolute path for project containment check.
	absPath := filePath
	if !filepath.IsAbs(filePath) {
		absPath = filepath.Join(c.ProjectDir, filePath)
	}

	// 4. Check project containment.
	if absPath != c.ProjectDir && !strings.HasPrefix(absPath, c.ProjectDir+"/") {
		return block(toolName, filePath,
			"Write operations must be within the project directory or allowed temp directories")
	}

	// 5. Check blocked directories (both relative and absolute forms).
	if dir := c.matchBlockedDir(filePath); dir != "" {
		return block(toolName, filePath,
			fmt.Sprintf("Cannot write to '%s' directory", dir))
	}
	if dir := c.matchBlockedDir(absPath); dir != "" {
		return block(toolName, filePath,
			fmt.Sprintf("Cannot write to '%s' directory", dir))
	}

	// 6. Dangerous extension warnings.
	if c.WarnOnDangerous {
		if warn := c.checkDangerousExtension(toolName, filePath); warn != "" {
			return &Verdict{Allow: true, Message: warn}
		}
	}

	return allow()
}

// isAllowedTmp checks if a path matches any allowed tmp prefix.
func (c *ScopeChecker) isAllowedTmp(path string) bool {
	for _, prefix := range c.AllowedTmpPrefixes {
		if strings.HasPrefix(path, prefix) {
			return true
		}
	}
	return false
}

// isProtectedBranch checks if a branch name matches any protected branch pattern.
func (c *ScopeChecker) isProtectedBranch(branch string) bool {
	for _, protected := range c.ProtectedBranches {
		if branch == protected {
			return true
		}
		// Glob match (e.g., release/*).
		if strings.Contains(protected, "*") {
			matched, err := filepath.Match(protected, branch)
			if err == nil && matched {
				return true
			}
		}
	}
	return false
}

// matchBlockedDir returns the matching blocked directory name, or empty string.
func (c *ScopeChecker) matchBlockedDir(path string) string {
	for _, blocked := range c.BlockedDirs {
		// Check: starts with "blocked/", contains "/blocked/", or ends with "/blocked".
		if strings.HasPrefix(path, blocked+"/") ||
			strings.Contains(path, "/"+blocked+"/") ||
			strings.HasSuffix(path, "/"+blocked) {
			return blocked
		}
	}
	return ""
}

// checkDangerousExtension returns a warning message if the file has a
// dangerous extension, or empty string otherwise.
func (c *ScopeChecker) checkDangerousExtension(toolName, filePath string) string {
	ext := strings.TrimPrefix(filepath.Ext(filePath), ".")
	ext = strings.ToLower(ext)
	if ext == "" {
		return ""
	}

	if containsExt(c.DangerousExts.Binary, ext) {
		return fmt.Sprintf("WARNING: %s targets binary file: %s", toolName, filePath)
	}
	if containsExt(c.DangerousExts.Credential, ext) {
		return fmt.Sprintf("WARNING: %s targets credential file: %s", toolName, filePath)
	}
	if containsExt(c.DangerousExts.Archive, ext) {
		return fmt.Sprintf("WARNING: %s targets archive file: %s", toolName, filePath)
	}

	return ""
}

// containsExt checks if an extension is in the list.
func containsExt(exts []string, ext string) bool {
	for _, e := range exts {
		if e == ext {
			return true
		}
	}
	return false
}

func allow() *Verdict {
	return &Verdict{Allow: true}
}

func block(toolName, filePath, reason string) *Verdict {
	var msg strings.Builder
	msg.WriteString(fmt.Sprintf("BLOCKED: %s operation not allowed\n\n", toolName))
	msg.WriteString(fmt.Sprintf("Path: %s\n", filePath))
	msg.WriteString(fmt.Sprintf("Reason: %s\n", reason))

	if strings.Contains(reason, "protected branch") {
		msg.WriteString("\n")
		msg.WriteString("Do NOT bypass by using Bash redirects, interpreter writes, or dangerouslyDisableSandbox.\n")
		msg.WriteString("Delegate to cf-git-operations teammate: SendMessage(recipient=\"cf-git-operations\", content=\"create-branch {prefix}/{name}\")\n")
	}

	msg.WriteString("\nThis is a safety restriction enforced by CodeFlow.\n")

	return &Verdict{Allow: false, Message: msg.String()}
}
