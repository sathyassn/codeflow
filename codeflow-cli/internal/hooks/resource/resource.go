package resource

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/codeflow/codeflow-cli/internal/hooks/security"
)

// ProtectionTier represents the protection level of a resource.
type ProtectionTier string

const (
	// TierCritical blocks the operation and requires explicit approval.
	TierCritical ProtectionTier = "critical"
	// TierHigh blocks the operation with a caution message.
	TierHigh ProtectionTier = "high"
	// TierModerate allows the operation with a warning.
	TierModerate ProtectionTier = "moderate"
	// TierNone indicates the resource is not protected.
	TierNone ProtectionTier = ""
)

// Verdict is the result of a protection guard check.
type Verdict struct {
	Allow   bool
	Tier    ProtectionTier
	Path    string
	Message string
}

// hookInput is the Claude Code hook JSON structure.
type hookInput struct {
	ToolName  string         `json:"tool_name"`
	ToolInput map[string]any `json:"tool_input"`
}

// ProtectionGuard checks file paths against protection tiers and handles
// the staging workflow for protected edits.
type ProtectionGuard struct {
	Policy       *security.EnforcementPolicy
	ProjectDir   string
	ProjectRoot  string // basename of project dir for staging paths
	StagingBase  string // override for staging base path (testing)
}

// NewProtectionGuard creates a guard from enforcement-policy.json at the
// given project directory.
func NewProtectionGuard(projectDir string) *ProtectionGuard {
	policy, err := security.ReadEnforcementPolicy(projectDir)
	if err != nil {
		policy = security.DefaultPolicy()
	}

	projectRoot := filepath.Base(projectDir)
	if envRoot := os.Getenv("CF_PROJECT_ROOT"); envRoot != "" {
		projectRoot = envRoot
	}

	return &ProtectionGuard{
		Policy:      policy,
		ProjectDir:  projectDir,
		ProjectRoot: projectRoot,
	}
}

// Check reads hook JSON from stdin, determines if the target file is protected,
// and returns a verdict.
func (g *ProtectionGuard) Check(stdin io.Reader) (*Verdict, error) {
	data, err := io.ReadAll(stdin)
	if err != nil {
		return allowVerdict(""), nil
	}

	if len(data) == 0 {
		return allowVerdict(""), nil
	}

	var input hookInput
	if err := json.Unmarshal(data, &input); err != nil {
		return allowVerdict(""), nil
	}

	// Only check Edit and Write tools.
	if input.ToolName != "Edit" && input.ToolName != "Write" {
		return allowVerdict(""), nil
	}

	// Extract file path.
	filePath, _ := input.ToolInput["file_path"].(string)
	if filePath == "" {
		return allowVerdict(""), nil
	}

	// Normalize to relative path.
	relPath := g.normalizePath(filePath)

	// Check staging area exception.
	if g.isStagingPath(filePath) {
		return allowVerdict(relPath), nil
	}

	return g.checkTier(relPath), nil
}

// CheckPath checks a single file path directly (no stdin parsing).
func (g *ProtectionGuard) CheckPath(filePath string) *Verdict {
	relPath := g.normalizePath(filePath)

	if g.isStagingPath(filePath) {
		return allowVerdict(relPath)
	}

	return g.checkTier(relPath)
}

// checkTier determines the protection tier for a relative path and returns
// the appropriate verdict.
func (g *ProtectionGuard) checkTier(relPath string) *Verdict {
	// Check critical tier.
	for _, pattern := range g.Policy.ProtectedResources.Critical {
		if matchesPattern(relPath, pattern) {
			return g.blockVerdict(TierCritical, relPath)
		}
	}

	// Check high tier.
	for _, pattern := range g.Policy.ProtectedResources.High {
		if matchesPattern(relPath, pattern) {
			return g.blockVerdict(TierHigh, relPath)
		}
	}

	// Check moderate tier -- warn but allow.
	for _, pattern := range g.Policy.ProtectedResources.Moderate {
		if matchesPattern(relPath, pattern) {
			return &Verdict{
				Allow:   true,
				Tier:    TierModerate,
				Path:    relPath,
				Message: fmt.Sprintf("Note: Editing moderately protected file: %s", relPath),
			}
		}
	}

	return allowVerdict(relPath)
}

// blockVerdict creates a block verdict with staging guidance.
func (g *ProtectionGuard) blockVerdict(tier ProtectionTier, relPath string) *Verdict {
	stagingDir := g.stagingBase()
	stagedPath := filepath.Join(stagingDir, relPath)
	stagedDir := filepath.Dir(stagedPath)

	var msg strings.Builder
	msg.WriteString(fmt.Sprintf("BLOCKED: %s resource protection\n\n", capitalizeFirst(string(tier))))
	msg.WriteString(fmt.Sprintf("Path: %s\n", relPath))
	msg.WriteString(fmt.Sprintf("Tier: %s\n\n", strings.ToUpper(string(tier))))

	msg.WriteString("AUTO-STAGING WORKFLOW:\n")
	msg.WriteString(fmt.Sprintf("1. mkdir -p %s\n", stagedDir))
	msg.WriteString(fmt.Sprintf("2. cp %s %s\n", relPath, stagedPath))
	msg.WriteString(fmt.Sprintf("3. Edit the staged copy at: %s\n", stagedPath))
	msg.WriteString(fmt.Sprintf("4. When ready, provide user: cp %s %s\n", stagedPath, relPath))
	msg.WriteString(fmt.Sprintf("5. After user applies: rm %s\n", stagedPath))

	return &Verdict{
		Allow:   false,
		Tier:    tier,
		Path:    relPath,
		Message: msg.String(),
	}
}

// FormatBlockJSON returns structured JSON for the hook framework when blocking.
func FormatBlockJSON(v *Verdict) string {
	output := struct {
		ProtectionGuard struct {
			Tier    string `json:"tier"`
			Path    string `json:"path"`
			Message string `json:"message"`
		} `json:"protectionGuard"`
	}{}
	output.ProtectionGuard.Tier = string(v.Tier)
	output.ProtectionGuard.Path = v.Path
	output.ProtectionGuard.Message = v.Message
	data, err := json.Marshal(output)
	if err != nil {
		return fmt.Sprintf(`{"protectionGuard":{"error":%q}}`, err)
	}
	return string(data)
}

// normalizePath converts an absolute path to relative if it starts with projectDir.
func (g *ProtectionGuard) normalizePath(filePath string) string {
	if strings.HasPrefix(filePath, g.ProjectDir+"/") {
		return strings.TrimPrefix(filePath, g.ProjectDir+"/")
	}
	return filePath
}

// isStagingPath checks if a path is in the staging area.
func (g *ProtectionGuard) isStagingPath(path string) bool {
	base := g.stagingBase()
	return strings.HasPrefix(path, base+"/") || strings.HasPrefix(path, "/tmp/claude/") && strings.Contains(path, "/managed/protected-edits/")
}

// stagingBase returns the base path for the staging area.
func (g *ProtectionGuard) stagingBase() string {
	if g.StagingBase != "" {
		return g.StagingBase
	}
	return fmt.Sprintf("/tmp/claude/%s/managed/protected-edits", g.ProjectRoot)
}

// matchesPattern checks if a path matches a glob pattern supporting ** syntax.
func matchesPattern(path, pattern string) bool {
	// Exact match.
	if path == pattern {
		return true
	}

	// Handle ** recursive glob.
	if strings.Contains(pattern, "**") {
		// Convert ** glob to prefix match.
		prefix := strings.Split(pattern, "**")[0]
		if strings.HasPrefix(path, prefix) {
			return true
		}
	}

	// Handle single * glob (matches within one directory level).
	if strings.Contains(pattern, "*") && !strings.Contains(pattern, "**") {
		matched, err := filepath.Match(pattern, path)
		if err == nil && matched {
			return true
		}
	}

	return false
}

// capitalizeFirst returns the string with the first character uppercased.
func capitalizeFirst(s string) string {
	if s == "" {
		return s
	}
	return strings.ToUpper(s[:1]) + s[1:]
}

func allowVerdict(path string) *Verdict {
	return &Verdict{
		Allow: true,
		Tier:  TierNone,
		Path:  path,
	}
}
