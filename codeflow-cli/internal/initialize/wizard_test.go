package initialize

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// lookPathAll returns a LookPath function that always finds executables.
func lookPathAll(file string) (string, error) {
	return "/usr/local/bin/" + file, nil
}

// lookPathMissing returns a LookPath function where the given tools are missing.
func lookPathMissing(missing ...string) func(string) (string, error) {
	set := make(map[string]bool, len(missing))
	for _, m := range missing {
		set[m] = true
	}
	return func(file string) (string, error) {
		if set[file] {
			return "", fmt.Errorf("executable %q not found in PATH", file)
		}
		return "/usr/local/bin/" + file, nil
	}
}

// runCmdOK returns a RunCmd function that always succeeds with a given output.
func runCmdOK(output string) func(string, ...string) ([]byte, error) {
	return func(_ string, _ ...string) ([]byte, error) {
		return []byte(output), nil
	}
}

// runCmdFail returns a RunCmd function that always fails.
func runCmdFail(msg string) func(string, ...string) ([]byte, error) {
	return func(_ string, _ ...string) ([]byte, error) {
		return nil, errors.New(msg)
	}
}

// runCmdByName dispatches by command name, supporting different results per tool.
func runCmdByName(handlers map[string]func(string, ...string) ([]byte, error)) func(string, ...string) ([]byte, error) {
	return func(name string, args ...string) ([]byte, error) {
		if h, ok := handlers[name]; ok {
			return h(name, args...)
		}
		return []byte("ok"), nil
	}
}

// newTestWizard creates a Wizard configured for testing with sane defaults.
// The returned buffer captures all output. Input is provided via the in parameter.
func newTestWizard(t *testing.T, in string) (*Wizard, *bytes.Buffer) {
	t.Helper()
	dir := t.TempDir()
	out := &bytes.Buffer{}

	w := &Wizard{
		In:       strings.NewReader(in),
		Out:      out,
		Dir:      dir,
		LookPath: lookPathAll,
		RunCmd: runCmdByName(map[string]func(string, ...string) ([]byte, error){
			"claude": runCmdOK("claude-code 1.0.0"),
			"git":    runCmdOK("https://github.com/example/repo.git"),
		}),
	}

	return w, out
}

func TestFullWizardFlow(t *testing.T) {
	t.Parallel()

	// When git remote is detected (github), step 4 skips prompting.
	// Input for: Step 5 name, Step 5 description, Step 6b PathFlow.
	input := "test-project\nA test project\nn\n"

	w, out := newTestWizard(t, input)
	ctx := t.Context()

	cfg, err := w.Run(ctx)
	if err != nil {
		t.Fatalf("wizard failed: %v\nOutput:\n%s", err, out.String())
	}

	// Verify config populated correctly.
	if cfg.ProjectName != "test-project" {
		t.Errorf("ProjectName = %q, want %q", cfg.ProjectName, "test-project")
	}
	if cfg.Description != "A test project" {
		t.Errorf("Description = %q, want %q", cfg.Description, "A test project")
	}
	if cfg.PathFlowMode {
		t.Error("PathFlowMode should be false")
	}
	if cfg.ProjectState != StateNew {
		t.Errorf("ProjectState = %v, want StateNew", cfg.ProjectState)
	}

	// Verify directories were created.
	expectedDirs := []string{
		".codeflow/config",
		".codeflow/scripts",
		".claude",
		".state/db",
		".state/ledger",
		".state/session",
		".state/runtime",
	}
	for _, d := range expectedDirs {
		fullPath := filepath.Join(cfg.Dir, d)
		info, err := os.Stat(fullPath)
		if err != nil {
			t.Errorf("expected directory %s to exist: %v", d, err)
		} else if !info.IsDir() {
			t.Errorf("expected %s to be a directory", d)
		}
	}

	// Verify database was created.
	dbPath := filepath.Join(cfg.Dir, ".state", "db", "codeflow.db")
	if _, err := os.Stat(dbPath); err != nil {
		t.Errorf("expected database at %s: %v", dbPath, err)
	}

	// Verify output contains key step markers.
	output := out.String()
	steps := []string{
		"Step 1/7: Project Location",
		"Step 2/7: Prerequisites Check",
		"Step 3/7: Claude Code Authentication",
		"Step 4/7: Git Provider Setup",
		"Step 5/7: Project Configuration",
		"Step 6/7: CodeFlow Setup",
		"Step 7/7: Verification",
		"Project initialized successfully!",
	}
	for _, step := range steps {
		if !strings.Contains(output, step) {
			t.Errorf("output missing step marker: %q", step)
		}
	}
}

func TestPrerequisiteFailure(t *testing.T) {
	t.Parallel()

	w, out := newTestWizard(t, "")
	w.LookPath = lookPathMissing("git", "sqlite3")
	ctx := t.Context()

	_, err := w.Run(ctx)
	if err == nil {
		t.Fatal("expected error for missing prerequisites")
	}

	if !errors.Is(err, ErrPrereqMissing) {
		t.Errorf("expected ErrPrereqMissing, got: %v", err)
	}

	output := out.String()
	if !strings.Contains(output, "[FAIL] git") {
		t.Error("output should show git as FAIL")
	}
	if !strings.Contains(output, "[FAIL] sqlite3") {
		t.Error("output should show sqlite3 as FAIL")
	}
}

func TestAuthFailure(t *testing.T) {
	t.Parallel()

	// All prereqs pass, but claude --version fails.
	w, _ := newTestWizard(t, "")
	w.RunCmd = runCmdByName(map[string]func(string, ...string) ([]byte, error){
		"claude": runCmdFail("connection refused"),
		"git":    runCmdOK("https://github.com/example/repo.git"),
	})
	ctx := t.Context()

	_, err := w.Run(ctx)
	if err == nil {
		t.Fatal("expected error for auth failure")
	}

	if !errors.Is(err, ErrAuthFailed) {
		t.Errorf("expected ErrAuthFailed, got: %v", err)
	}
}

func TestExistingProject(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	// Create both .codeflow/ and .state/ to simulate an existing project.
	if err := os.MkdirAll(filepath.Join(dir, ".codeflow"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(dir, ".state"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Git remote auto-detected, so no provider prompt.
	// Input: name (empty=default), description, PathFlow.
	input := "\nExisting desc\nn\n"
	out := &bytes.Buffer{}

	w := &Wizard{
		In:       strings.NewReader(input),
		Out:      out,
		Dir:      dir,
		LookPath: lookPathAll,
		RunCmd: runCmdByName(map[string]func(string, ...string) ([]byte, error){
			"claude": runCmdOK("claude-code 1.0.0"),
			"git":    runCmdOK("https://github.com/example/repo.git"),
		}),
	}

	ctx := t.Context()
	cfg, err := w.Run(ctx)
	if err != nil {
		t.Fatalf("wizard failed on existing project: %v", err)
	}

	if cfg.ProjectState != StateExisting {
		t.Errorf("ProjectState = %v, want StateExisting", cfg.ProjectState)
	}

	output := out.String()
	if !strings.Contains(output, "existing project") {
		t.Error("output should mention existing project")
	}
}

func TestCancellation(t *testing.T) {
	t.Parallel()

	// EOF on stdin simulates cancellation during a prompt.
	// Prereqs all pass, auth passes, but git provider prompt gets EOF.
	w, _ := newTestWizard(t, "")
	// Empty reader means prompt hits EOF immediately at step 4.
	w.In = strings.NewReader("")
	// Provide a RunCmd that will succeed for claude but the prompt for git provider will EOF.
	w.RunCmd = runCmdByName(map[string]func(string, ...string) ([]byte, error){
		"claude": runCmdOK("claude-code 1.0.0"),
		"git":    runCmdFail("no remote"), // No remote means prompt for provider.
	})

	ctx := t.Context()
	_, err := w.Run(ctx)

	if err == nil {
		t.Fatal("expected cancellation error")
	}

	if !errors.Is(err, ErrCancelled) {
		t.Errorf("expected ErrCancelled, got: %v", err)
	}
}

func TestDBInitIntegration(t *testing.T) {
	t.Parallel()

	// Git remote auto-detected, so no provider prompt.
	input := "db-test\nDB init test\nn\n"
	w, _ := newTestWizard(t, input)
	ctx := t.Context()

	cfg, err := w.Run(ctx)
	if err != nil {
		t.Fatalf("wizard failed: %v", err)
	}

	// Verify the database file exists and is non-empty.
	dbPath := filepath.Join(cfg.Dir, ".state", "db", "codeflow.db")
	info, err := os.Stat(dbPath)
	if err != nil {
		t.Fatalf("database not created: %v", err)
	}
	if info.Size() == 0 {
		t.Error("database file is empty")
	}
}

func TestPathFlowOptIn(t *testing.T) {
	t.Parallel()

	// Git remote auto-detected, so no provider prompt.
	// PathFlow enabled (y).
	input := "pf-project\nPathFlow project\ny\n"
	w, out := newTestWizard(t, input)
	ctx := t.Context()

	cfg, err := w.Run(ctx)
	if err != nil {
		t.Fatalf("wizard failed: %v", err)
	}

	if !cfg.PathFlowMode {
		t.Error("PathFlowMode should be true when user answers 'y'")
	}

	output := out.String()
	if !strings.Contains(output, "PathFlow mode: enabled") {
		t.Error("output should confirm PathFlow mode enabled")
	}
}

func TestVerificationPassAndFail(t *testing.T) {
	t.Parallel()

	t.Run("verification passes", func(t *testing.T) {
		t.Parallel()

		input := "verify-ok\nVerify pass\nn\n"
		w, out := newTestWizard(t, input)
		ctx := t.Context()

		_, err := w.Run(ctx)
		if err != nil {
			t.Fatalf("wizard failed: %v", err)
		}

		output := out.String()
		if !strings.Contains(output, "Step 7/7: Verification") {
			t.Error("should have reached verification step")
		}
	})

	t.Run("verification warns but passes", func(t *testing.T) {
		t.Parallel()

		input := "verify-warn\nVerify with warnings\nn\n"
		w, out := newTestWizard(t, input)
		// Make .git/ check fail (warning level, not critical).
		// This happens naturally since TempDir has no .git/.
		ctx := t.Context()

		_, err := w.Run(ctx)
		if err != nil {
			t.Fatalf("wizard should not fail on warnings: %v", err)
		}

		output := out.String()
		if !strings.Contains(output, "[WARN]") {
			t.Error("output should contain warning markers")
		}
	})
}

func TestDetectProjectState(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		setup    func(t *testing.T) string
		expected ProjectState
	}{
		{
			name: "new project",
			setup: func(t *testing.T) string {
				t.Helper()
				return t.TempDir()
			},
			expected: StateNew,
		},
		{
			name: "existing project",
			setup: func(t *testing.T) string {
				t.Helper()
				dir := t.TempDir()
				if err := os.MkdirAll(filepath.Join(dir, ".codeflow"), 0o755); err != nil {
					t.Fatal(err)
				}
				if err := os.MkdirAll(filepath.Join(dir, ".state"), 0o755); err != nil {
					t.Fatal(err)
				}
				return dir
			},
			expected: StateExisting,
		},
		{
			name: "join project",
			setup: func(t *testing.T) string {
				t.Helper()
				dir := t.TempDir()
				if err := os.MkdirAll(filepath.Join(dir, ".codeflow"), 0o755); err != nil {
					t.Fatal(err)
				}
				return dir
			},
			expected: StateJoin,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			dir := tc.setup(t)
			got := detectProjectState(dir)
			if got != tc.expected {
				t.Errorf("detectProjectState() = %v, want %v", got, tc.expected)
			}
		})
	}
}

func TestProjectStateString(t *testing.T) {
	t.Parallel()

	tests := []struct {
		state    ProjectState
		expected string
	}{
		{StateNew, "new"},
		{StateExisting, "existing"},
		{StateJoin, "join"},
		{ProjectState(99), "unknown"},
	}

	for _, tc := range tests {
		t.Run(tc.expected, func(t *testing.T) {
			t.Parallel()
			if got := tc.state.String(); got != tc.expected {
				t.Errorf("String() = %q, want %q", got, tc.expected)
			}
		})
	}
}

func TestGitProviderString(t *testing.T) {
	t.Parallel()

	tests := []struct {
		provider GitProvider
		expected string
	}{
		{ProviderGitHub, "github"},
		{ProviderGitLab, "gitlab"},
		{ProviderBitbucket, "bitbucket"},
		{GitProvider(99), "unknown"},
	}

	for _, tc := range tests {
		t.Run(tc.expected, func(t *testing.T) {
			t.Parallel()
			if got := tc.provider.String(); got != tc.expected {
				t.Errorf("String() = %q, want %q", got, tc.expected)
			}
		})
	}
}

func TestDetectGitProvider(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		url      string
		expected GitProvider
		detected bool
	}{
		{"github", "https://github.com/user/repo.git", ProviderGitHub, true},
		{"gitlab", "https://gitlab.com/user/repo.git", ProviderGitLab, true},
		{"bitbucket", "https://bitbucket.org/user/repo.git", ProviderBitbucket, true},
		{"unknown", "https://example.com/repo.git", ProviderGitHub, false},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			w := &Wizard{
				RunCmd: func(name string, args ...string) ([]byte, error) {
					if name == "git" {
						return []byte(tc.url), nil
					}
					return nil, errors.New("not git")
				},
			}

			provider, detected := w.detectGitProvider()
			if detected != tc.detected {
				t.Errorf("detected = %v, want %v", detected, tc.detected)
			}
			if detected && provider != tc.expected {
				t.Errorf("provider = %v, want %v", provider, tc.expected)
			}
		})
	}
}

func TestJoinProject(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	// Create .codeflow/ but NOT .state/ to simulate a cloned project.
	if err := os.MkdirAll(filepath.Join(dir, ".codeflow"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Git remote auto-detected, so no provider prompt.
	input := "join-project\nJoin desc\nn\n"
	out := &bytes.Buffer{}

	w := &Wizard{
		In:       strings.NewReader(input),
		Out:      out,
		Dir:      dir,
		LookPath: lookPathAll,
		RunCmd: runCmdByName(map[string]func(string, ...string) ([]byte, error){
			"claude": runCmdOK("claude-code 1.0.0"),
			"git":    runCmdOK("https://github.com/example/repo.git"),
		}),
	}

	ctx := t.Context()
	cfg, err := w.Run(ctx)
	if err != nil {
		t.Fatalf("wizard failed on join project: %v", err)
	}

	if cfg.ProjectState != StateJoin {
		t.Errorf("ProjectState = %v, want StateJoin", cfg.ProjectState)
	}

	output := out.String()
	if !strings.Contains(output, "joining project") {
		t.Error("output should mention joining project")
	}
}

func TestCleanupOnError(t *testing.T) {
	t.Parallel()

	w := &Wizard{
		createdDirs: []string{
			filepath.Join(t.TempDir(), "cleanup-test", "a"),
			filepath.Join(t.TempDir(), "cleanup-test", "b"),
		},
	}

	// Create the dirs first.
	for _, d := range w.createdDirs {
		if err := os.MkdirAll(d, 0o755); err != nil {
			t.Fatal(err)
		}
	}

	// Run cleanup.
	w.cleanup()

	// Verify dirs are removed.
	for _, d := range w.createdDirs {
		if _, err := os.Stat(d); !os.IsNotExist(err) {
			t.Errorf("directory %s should have been removed after cleanup", d)
		}
	}
}
