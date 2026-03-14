package preflight

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"time"

	"github.com/codeflow/codeflow-cli/internal/hooks/session"
)

// Sentinel errors for preflight check failures.
var (
	// ErrNoCodeflowDir indicates the .codeflow/ directory was not found.
	ErrNoCodeflowDir = errors.New("preflight: .codeflow directory not found")

	// ErrClaudeNotFound indicates the Claude Code CLI is not accessible.
	ErrClaudeNotFound = errors.New("preflight: claude CLI not found in PATH")
)

// Default paths relative to the project root.
const (
	CodeflowDir       = ".codeflow"
	GitDir            = ".git"
	SessionDir        = ".state/session"
	PathflowActiveFile = "pathflow/" + session.PathflowSessionStatusFile
)

// Default stale session threshold.
const DefaultStaleThreshold = 24 * time.Hour

// Level classifies the severity of a preflight check.
type Level int

const (
	// Critical checks must pass for the CLI to proceed.
	Critical Level = iota
	// Warning checks report issues but do not block execution.
	Warning
)

// String returns the human-readable name for the level.
func (l Level) String() string {
	switch l {
	case Critical:
		return "critical"
	case Warning:
		return "warning"
	default:
		return "unknown"
	}
}

// Result holds the outcome of a single preflight check.
type Result struct {
	Name     string
	Level    Level
	Passed   bool
	Message  string
	Duration time.Duration
}

// Options configures the preflight check behavior.
type Options struct {
	// StaleThreshold defines how old a pathflow-active flag must be to be
	// considered stale. Defaults to DefaultStaleThreshold if zero.
	StaleThreshold time.Duration

	// LookPath is the function used to locate executables. Defaults to
	// exec.LookPath. Inject a custom function for testing.
	LookPath func(file string) (string, error)
}

// applyDefaults fills in zero-value fields with sensible defaults.
func (o *Options) applyDefaults() {
	if o.StaleThreshold == 0 {
		o.StaleThreshold = DefaultStaleThreshold
	}
	if o.LookPath == nil {
		o.LookPath = exec.LookPath
	}
}

// Run executes all preflight checks sequentially against the given project
// root directory. It returns a result for every check regardless of pass/fail.
//
// Checks executed (in order):
//  1. .codeflow/ directory exists (critical)
//  2. Claude Code CLI accessible via PATH (critical)
//  3. .git/ directory exists (warning)
//  4. No stale sessions (warning)
func Run(_ context.Context, projectRoot string, opts *Options) []Result {
	if opts == nil {
		opts = &Options{}
	}
	opts.applyDefaults()

	checks := []func(string, *Options) Result{
		checkCodeflowDir,
		checkClaudeCLI,
		checkGitRepo,
		checkStaleSessions,
	}

	results := make([]Result, 0, len(checks))
	for _, check := range checks {
		results = append(results, check(projectRoot, opts))
	}
	return results
}

// HasErrors returns true if any critical check failed.
func HasErrors(results []Result) bool {
	for _, r := range results {
		if r.Level == Critical && !r.Passed {
			return true
		}
	}
	return false
}

// HasWarnings returns true if any warning-level check failed.
func HasWarnings(results []Result) bool {
	for _, r := range results {
		if r.Level == Warning && !r.Passed {
			return true
		}
	}
	return false
}

// checkCodeflowDir verifies the .codeflow/ directory exists at the project root.
func checkCodeflowDir(projectRoot string, _ *Options) Result {
	start := time.Now()
	dir := filepath.Join(projectRoot, CodeflowDir)

	info, err := os.Stat(dir)
	if err != nil || !info.IsDir() {
		return Result{
			Name:     "codeflow-dir",
			Level:    Critical,
			Passed:   false,
			Message:  fmt.Sprintf("%v", ErrNoCodeflowDir),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "codeflow-dir",
		Level:    Critical,
		Passed:   true,
		Message:  ".codeflow directory found",
		Duration: time.Since(start),
	}
}

// checkClaudeCLI verifies the claude CLI is accessible via PATH.
func checkClaudeCLI(_ string, opts *Options) Result {
	start := time.Now()

	_, err := opts.LookPath("claude")
	if err != nil {
		return Result{
			Name:     "claude-cli",
			Level:    Critical,
			Passed:   false,
			Message:  fmt.Sprintf("%v", ErrClaudeNotFound),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "claude-cli",
		Level:    Critical,
		Passed:   true,
		Message:  "claude CLI found in PATH",
		Duration: time.Since(start),
	}
}

// checkGitRepo verifies the .git/ directory exists at the project root.
func checkGitRepo(projectRoot string, _ *Options) Result {
	start := time.Now()
	dir := filepath.Join(projectRoot, GitDir)

	info, err := os.Stat(dir)
	if err != nil || !info.IsDir() {
		return Result{
			Name:     "git-repo",
			Level:    Warning,
			Passed:   false,
			Message:  "git repository not initialized (.git/ not found)",
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "git-repo",
		Level:    Warning,
		Passed:   true,
		Message:  "git repository initialized",
		Duration: time.Since(start),
	}
}

// checkStaleSessions scans the .state/session/ directory for pathflow-active
// flags older than the configured threshold.
func checkStaleSessions(projectRoot string, opts *Options) Result {
	start := time.Now()
	sessionDir := filepath.Join(projectRoot, SessionDir)

	entries, err := os.ReadDir(sessionDir)
	if err != nil {
		// No session directory means no stale sessions.
		return Result{
			Name:     "stale-sessions",
			Level:    Warning,
			Passed:   true,
			Message:  "no session directory found (no stale sessions)",
			Duration: time.Since(start),
		}
	}

	var staleCount int
	now := time.Now()

	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}

		pfDir := filepath.Join(sessionDir, entry.Name(), "pathflow")
		if !session.IsPathflowActive(pfDir) {
			continue // No active session.
		}

		statusPath := filepath.Join(pfDir, session.PathflowSessionStatusFile)
		info, err := os.Stat(statusPath)
		if err != nil {
			continue
		}

		age := now.Sub(info.ModTime())
		if age > opts.StaleThreshold {
			staleCount++
		}
	}

	if staleCount > 0 {
		return Result{
			Name:     "stale-sessions",
			Level:    Warning,
			Passed:   false,
			Message:  fmt.Sprintf("%d stale session(s) detected (older than %s)", staleCount, opts.StaleThreshold),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "stale-sessions",
		Level:    Warning,
		Passed:   true,
		Message:  "no stale sessions detected",
		Duration: time.Since(start),
	}
}
