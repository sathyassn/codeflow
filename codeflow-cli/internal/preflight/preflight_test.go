package preflight

import (
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// setupProject creates a temp directory with the standard project layout
// (.codeflow/, .git/) for testing. Returns the project root path.
func setupProject(t *testing.T) string {
	t.Helper()
	root := t.TempDir()

	if err := os.MkdirAll(filepath.Join(root, CodeflowDir), 0o755); err != nil {
		t.Fatalf("creating .codeflow: %v", err)
	}
	if err := os.MkdirAll(filepath.Join(root, GitDir), 0o755); err != nil {
		t.Fatalf("creating .git: %v", err)
	}

	return root
}

// lookPathFound returns a LookPath function that always succeeds.
func lookPathFound(file string) (string, error) {
	return "/usr/local/bin/" + file, nil
}

// lookPathNotFound returns a LookPath function that always fails.
func lookPathNotFound(file string) (string, error) {
	return "", fmt.Errorf("executable %q not found in PATH", file)
}

func TestRunAllChecksPass(t *testing.T) {
	root := setupProject(t)
	ctx := t.Context()

	opts := &Options{LookPath: lookPathFound}
	results := Run(ctx, root, opts)

	if len(results) != 4 {
		t.Fatalf("expected 4 results, got %d", len(results))
	}

	for _, r := range results {
		if !r.Passed {
			t.Errorf("check %q should have passed, got message: %s", r.Name, r.Message)
		}
		if r.Duration <= 0 {
			t.Errorf("check %q should have positive duration, got %v", r.Name, r.Duration)
		}
	}

	if HasErrors(results) {
		t.Error("HasErrors should be false when all checks pass")
	}
	if HasWarnings(results) {
		t.Error("HasWarnings should be false when all checks pass")
	}
}

func TestRunMissingCodeflowDir(t *testing.T) {
	root := t.TempDir()
	// Create .git/ but not .codeflow/.
	if err := os.MkdirAll(filepath.Join(root, GitDir), 0o755); err != nil {
		t.Fatalf("creating .git: %v", err)
	}

	ctx := t.Context()
	opts := &Options{LookPath: lookPathFound}
	results := Run(ctx, root, opts)

	// Find the codeflow-dir check.
	var found bool
	for _, r := range results {
		if r.Name == "codeflow-dir" {
			found = true
			if r.Passed {
				t.Error("codeflow-dir check should have failed")
			}
			if r.Level != Critical {
				t.Errorf("codeflow-dir level = %v, want Critical", r.Level)
			}
			break
		}
	}
	if !found {
		t.Error("codeflow-dir check not found in results")
	}

	if !HasErrors(results) {
		t.Error("HasErrors should be true when .codeflow/ is missing")
	}
}

func TestRunMissingClaudeCLI(t *testing.T) {
	root := setupProject(t)
	ctx := t.Context()

	opts := &Options{LookPath: lookPathNotFound}
	results := Run(ctx, root, opts)

	var found bool
	for _, r := range results {
		if r.Name == "claude-cli" {
			found = true
			if r.Passed {
				t.Error("claude-cli check should have failed")
			}
			if r.Level != Critical {
				t.Errorf("claude-cli level = %v, want Critical", r.Level)
			}
			break
		}
	}
	if !found {
		t.Error("claude-cli check not found in results")
	}

	if !HasErrors(results) {
		t.Error("HasErrors should be true when claude CLI is missing")
	}
}

func TestRunNoGitRepo(t *testing.T) {
	root := t.TempDir()
	// Create .codeflow/ but not .git/.
	if err := os.MkdirAll(filepath.Join(root, CodeflowDir), 0o755); err != nil {
		t.Fatalf("creating .codeflow: %v", err)
	}

	ctx := t.Context()
	opts := &Options{LookPath: lookPathFound}
	results := Run(ctx, root, opts)

	var found bool
	for _, r := range results {
		if r.Name == "git-repo" {
			found = true
			if r.Passed {
				t.Error("git-repo check should have failed")
			}
			if r.Level != Warning {
				t.Errorf("git-repo level = %v, want Warning", r.Level)
			}
			break
		}
	}
	if !found {
		t.Error("git-repo check not found in results")
	}

	// Missing git is a warning, not an error.
	if HasErrors(results) {
		t.Error("HasErrors should be false (git is warning-level)")
	}
	if !HasWarnings(results) {
		t.Error("HasWarnings should be true when .git/ is missing")
	}
}

func TestRunStaleSessionDetected(t *testing.T) {
	root := setupProject(t)

	// Create a stale session with an old pathflow-active flag.
	sessionDir := filepath.Join(root, SessionDir, "ses-oldsession123")
	pfDir := filepath.Join(sessionDir, "pathflow")
	if err := os.MkdirAll(pfDir, 0o755); err != nil {
		t.Fatalf("creating session dir: %v", err)
	}

	flagPath := filepath.Join(pfDir, "is-pathflow-active")
	if err := os.WriteFile(flagPath, []byte("1"), 0o644); err != nil {
		t.Fatalf("writing flag: %v", err)
	}

	// Set the file modification time to 48 hours ago to make it stale.
	staleTime := time.Now().Add(-48 * time.Hour)
	if err := os.Chtimes(flagPath, staleTime, staleTime); err != nil {
		t.Fatalf("setting file time: %v", err)
	}

	ctx := t.Context()
	opts := &Options{
		LookPath:       lookPathFound,
		StaleThreshold: 24 * time.Hour,
	}
	results := Run(ctx, root, opts)

	var found bool
	for _, r := range results {
		if r.Name == "stale-sessions" {
			found = true
			if r.Passed {
				t.Error("stale-sessions check should have failed")
			}
			if r.Level != Warning {
				t.Errorf("stale-sessions level = %v, want Warning", r.Level)
			}
			break
		}
	}
	if !found {
		t.Error("stale-sessions check not found in results")
	}
}

func TestRunTimingConstraint(t *testing.T) {
	root := setupProject(t)
	ctx := t.Context()

	opts := &Options{LookPath: lookPathFound}

	start := time.Now()
	results := Run(ctx, root, opts)
	elapsed := time.Since(start)

	if elapsed > 100*time.Millisecond {
		t.Errorf("preflight checks took %v, want <= 100ms", elapsed)
	}

	// Also verify individual check durations are reasonable.
	var totalDuration time.Duration
	for _, r := range results {
		totalDuration += r.Duration
	}
	if totalDuration > 100*time.Millisecond {
		t.Errorf("sum of check durations = %v, want <= 100ms", totalDuration)
	}

	_ = results // Verify we got results back.
}

func TestRunNilOptions(t *testing.T) {
	root := setupProject(t)
	ctx := t.Context()

	// Run with nil options -- should apply defaults and not panic.
	results := Run(ctx, root, nil)

	if len(results) != 4 {
		t.Fatalf("expected 4 results with nil options, got %d", len(results))
	}
}

func TestRunReturnsStructuredResults(t *testing.T) {
	root := setupProject(t)
	ctx := t.Context()

	opts := &Options{LookPath: lookPathFound}
	results := Run(ctx, root, opts)

	expectedNames := []string{"codeflow-dir", "claude-cli", "git-repo", "stale-sessions"}
	if len(results) != len(expectedNames) {
		t.Fatalf("expected %d results, got %d", len(expectedNames), len(results))
	}

	for i, r := range results {
		if r.Name != expectedNames[i] {
			t.Errorf("results[%d].Name = %q, want %q", i, r.Name, expectedNames[i])
		}
		if r.Name == "" {
			t.Errorf("results[%d].Name should not be empty", i)
		}
		if r.Message == "" {
			t.Errorf("results[%d].Message should not be empty", i)
		}
	}
}

func TestRunStaleSessionFreshNotDetected(t *testing.T) {
	root := setupProject(t)

	// Create a fresh session with a recent pathflow-active flag.
	sessionDir := filepath.Join(root, SessionDir, "ses-freshsession123")
	pfDir := filepath.Join(sessionDir, "pathflow")
	if err := os.MkdirAll(pfDir, 0o755); err != nil {
		t.Fatalf("creating session dir: %v", err)
	}

	flagPath := filepath.Join(pfDir, "is-pathflow-active")
	if err := os.WriteFile(flagPath, []byte("1"), 0o644); err != nil {
		t.Fatalf("writing flag: %v", err)
	}
	// File mod time is "now" -- it should NOT be detected as stale.

	ctx := t.Context()
	opts := &Options{
		LookPath:       lookPathFound,
		StaleThreshold: 24 * time.Hour,
	}
	results := Run(ctx, root, opts)

	for _, r := range results {
		if r.Name == "stale-sessions" {
			if !r.Passed {
				t.Error("stale-sessions should pass for fresh sessions")
			}
			return
		}
	}
	t.Error("stale-sessions check not found in results")
}

func TestRunNoSessionDirectory(t *testing.T) {
	root := setupProject(t)
	// Do NOT create .state/session/ -- it should not exist.

	ctx := t.Context()
	opts := &Options{LookPath: lookPathFound}
	results := Run(ctx, root, opts)

	for _, r := range results {
		if r.Name == "stale-sessions" {
			if !r.Passed {
				t.Error("stale-sessions should pass when session directory does not exist")
			}
			return
		}
	}
	t.Error("stale-sessions check not found in results")
}

func TestRunCodeflowDirIsFile(t *testing.T) {
	root := t.TempDir()
	// Create .codeflow as a file (not a directory) -- should fail the check.
	if err := os.WriteFile(filepath.Join(root, CodeflowDir), []byte("not a dir"), 0o644); err != nil {
		t.Fatalf("creating .codeflow file: %v", err)
	}
	if err := os.MkdirAll(filepath.Join(root, GitDir), 0o755); err != nil {
		t.Fatalf("creating .git: %v", err)
	}

	ctx := t.Context()
	opts := &Options{LookPath: lookPathFound}
	results := Run(ctx, root, opts)

	for _, r := range results {
		if r.Name == "codeflow-dir" {
			if r.Passed {
				t.Error("codeflow-dir check should fail when .codeflow is a file, not a directory")
			}
			return
		}
	}
	t.Error("codeflow-dir check not found in results")
}

func TestLevelString(t *testing.T) {
	tests := []struct {
		level Level
		want  string
	}{
		{Critical, "critical"},
		{Warning, "warning"},
		{Level(99), "unknown"},
	}

	for _, tt := range tests {
		t.Run(tt.want, func(t *testing.T) {
			got := tt.level.String()
			if got != tt.want {
				t.Errorf("Level(%d).String() = %q, want %q", tt.level, got, tt.want)
			}
		})
	}
}

func TestHasErrorsAndWarnings(t *testing.T) {
	t.Run("no results", func(t *testing.T) {
		if HasErrors(nil) {
			t.Error("HasErrors should be false for nil slice")
		}
		if HasWarnings(nil) {
			t.Error("HasWarnings should be false for nil slice")
		}
	})

	t.Run("all pass", func(t *testing.T) {
		results := []Result{
			{Name: "a", Level: Critical, Passed: true},
			{Name: "b", Level: Warning, Passed: true},
		}
		if HasErrors(results) {
			t.Error("HasErrors should be false when all pass")
		}
		if HasWarnings(results) {
			t.Error("HasWarnings should be false when all pass")
		}
	})

	t.Run("critical failure", func(t *testing.T) {
		results := []Result{
			{Name: "a", Level: Critical, Passed: false},
			{Name: "b", Level: Warning, Passed: true},
		}
		if !HasErrors(results) {
			t.Error("HasErrors should be true for critical failure")
		}
		if HasWarnings(results) {
			t.Error("HasWarnings should be false when only critical fails")
		}
	})

	t.Run("warning failure", func(t *testing.T) {
		results := []Result{
			{Name: "a", Level: Critical, Passed: true},
			{Name: "b", Level: Warning, Passed: false},
		}
		if HasErrors(results) {
			t.Error("HasErrors should be false when only warning fails")
		}
		if !HasWarnings(results) {
			t.Error("HasWarnings should be true for warning failure")
		}
	})
}

func TestRunMultipleStaleSessionsMixed(t *testing.T) {
	root := setupProject(t)

	// Create one stale and one fresh session.
	for _, tc := range []struct {
		name string
		age  time.Duration
	}{
		{"ses-stale001", 48 * time.Hour},
		{"ses-fresh001", 1 * time.Hour},
	} {
		pfDir := filepath.Join(root, SessionDir, tc.name, "pathflow")
		if err := os.MkdirAll(pfDir, 0o755); err != nil {
			t.Fatalf("creating session dir %s: %v", tc.name, err)
		}
		flagPath := filepath.Join(pfDir, "is-pathflow-active")
		if err := os.WriteFile(flagPath, []byte("1"), 0o644); err != nil {
			t.Fatalf("writing flag %s: %v", tc.name, err)
		}
		modTime := time.Now().Add(-tc.age)
		if err := os.Chtimes(flagPath, modTime, modTime); err != nil {
			t.Fatalf("setting time %s: %v", tc.name, err)
		}
	}

	ctx := t.Context()
	opts := &Options{
		LookPath:       lookPathFound,
		StaleThreshold: 24 * time.Hour,
	}
	results := Run(ctx, root, opts)

	for _, r := range results {
		if r.Name == "stale-sessions" {
			if r.Passed {
				t.Error("stale-sessions should fail when at least one stale session exists")
			}
			// Should report exactly 1 stale session (the 48h one), not the 1h one.
			if r.Message != "1 stale session(s) detected (older than 24h0m0s)" {
				t.Errorf("unexpected message: %s", r.Message)
			}
			return
		}
	}
	t.Error("stale-sessions check not found in results")
}
