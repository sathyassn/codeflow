package prompt

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func newTestValidator(t *testing.T) *PromptValidator {
	t.Helper()
	return &PromptValidator{
		ProjectDir:        t.TempDir(),
		ProtectedBranches: DefaultProtectedBranches(),
		GitRunner: func(_ string) (string, int) {
			return "feat/my-feature", 0
		},
		PathFlowChecker: func(_ string) bool {
			return false
		},
	}
}

func TestValidate_NoReminders(t *testing.T) {
	t.Parallel()

	v := newTestValidator(t)

	// Create active task file.
	stateDir := filepath.Join(v.ProjectDir, ".state", "runtime")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	task := activeTaskJSON{FormatID: "INF-TSK-021-018"}
	data, _ := json.Marshal(task)
	if err := os.WriteFile(filepath.Join(stateDir, "active-task.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	var buf bytes.Buffer
	err := v.Validate(strings.NewReader("{}"), &buf)
	if err != nil {
		t.Fatalf("Validate() error = %v", err)
	}

	output := buf.String()
	// Should have active work reminder.
	if !strings.Contains(output, "Active work: INF-TSK-021-018") {
		t.Errorf("should contain active work; got: %s", output)
	}
	// Should NOT have uncommitted changes.
	if strings.Contains(output, "uncommitted changes") {
		t.Errorf("should not have uncommitted changes; got: %s", output)
	}
}

func TestValidate_UncommittedChanges(t *testing.T) {
	t.Parallel()

	v := newTestValidator(t)
	v.GitRunner = func(_ string) (string, int) {
		return "feat/my-feature", 5
	}

	var buf bytes.Buffer
	_ = v.Validate(strings.NewReader("{}"), &buf)

	output := buf.String()
	if !strings.Contains(output, "5 uncommitted changes on branch 'feat/my-feature'") {
		t.Errorf("should report uncommitted changes; got: %s", output)
	}
}

func TestValidate_ProtectedBranch(t *testing.T) {
	t.Parallel()

	v := newTestValidator(t)
	v.GitRunner = func(_ string) (string, int) {
		return "main", 0
	}

	var buf bytes.Buffer
	_ = v.Validate(strings.NewReader("{}"), &buf)

	output := buf.String()
	if !strings.Contains(output, "On protected branch 'main'") {
		t.Errorf("should warn about protected branch; got: %s", output)
	}
	if !strings.Contains(output, "cf-git-operations") {
		t.Errorf("should include delegation guidance; got: %s", output)
	}
}

func TestValidate_MasterBranch(t *testing.T) {
	t.Parallel()

	v := newTestValidator(t)
	v.GitRunner = func(_ string) (string, int) {
		return "master", 0
	}

	var buf bytes.Buffer
	_ = v.Validate(strings.NewReader("{}"), &buf)

	if !strings.Contains(buf.String(), "On protected branch 'master'") {
		t.Errorf("should warn about master branch; got: %s", buf.String())
	}
}

func TestValidate_NoActiveTask(t *testing.T) {
	t.Parallel()

	v := newTestValidator(t)

	var buf bytes.Buffer
	_ = v.Validate(strings.NewReader("{}"), &buf)

	output := buf.String()
	if !strings.Contains(output, "No active task registered") {
		t.Errorf("should note missing active task; got: %s", output)
	}
	if !strings.Contains(output, "cf-knowledge-layer") {
		t.Errorf("should include delegation guidance; got: %s", output)
	}
}

func TestValidate_ActiveTaskWithTaskID(t *testing.T) {
	t.Parallel()

	v := newTestValidator(t)

	// Create active task with task_id only (no format_id).
	stateDir := filepath.Join(v.ProjectDir, ".state", "runtime")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	task := activeTaskJSON{TaskID: "task-abc123"}
	data, _ := json.Marshal(task)
	if err := os.WriteFile(filepath.Join(stateDir, "active-task.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	var buf bytes.Buffer
	_ = v.Validate(strings.NewReader("{}"), &buf)

	if !strings.Contains(buf.String(), "Active work: task-abc123") {
		t.Errorf("should show task_id fallback; got: %s", buf.String())
	}
}

func TestValidate_PathFlowActive(t *testing.T) {
	t.Parallel()

	v := newTestValidator(t)
	v.PathFlowChecker = func(_ string) bool {
		return true
	}

	var buf bytes.Buffer
	_ = v.Validate(strings.NewReader("{}"), &buf)

	output := buf.String()
	if !strings.Contains(output, "PathFlow mode active") {
		t.Errorf("should report PathFlow active; got: %s", output)
	}
	if !strings.Contains(output, "cf-review") {
		t.Errorf("should include team coordination guidance; got: %s", output)
	}
}

func TestValidate_TagWrapping(t *testing.T) {
	t.Parallel()

	v := newTestValidator(t)
	v.GitRunner = func(_ string) (string, int) {
		return "feat/test", 3
	}

	var buf bytes.Buffer
	_ = v.Validate(strings.NewReader("{}"), &buf)

	output := buf.String()
	// Each reminder should be wrapped in tags.
	openCount := strings.Count(output, "<user-prompt-submit-hook>")
	closeCount := strings.Count(output, "</user-prompt-submit-hook>")
	if openCount != closeCount {
		t.Errorf("mismatched tags: %d open, %d close", openCount, closeCount)
	}
	if openCount < 2 {
		t.Errorf("should have at least 2 tagged reminders (changes + no-task); got %d", openCount)
	}
}

func TestValidate_AllReminders(t *testing.T) {
	t.Parallel()

	v := newTestValidator(t)
	v.GitRunner = func(_ string) (string, int) {
		return "main", 2
	}
	v.PathFlowChecker = func(_ string) bool {
		return true
	}

	var buf bytes.Buffer
	_ = v.Validate(strings.NewReader("{}"), &buf)

	output := buf.String()
	// Should have 4 reminders: uncommitted, protected branch, no task, pathflow.
	tagCount := strings.Count(output, "<user-prompt-submit-hook>")
	if tagCount != 4 {
		t.Errorf("should have 4 reminders; got %d; output: %s", tagCount, output)
	}
}

func TestIsProtectedBranch(t *testing.T) {
	t.Parallel()

	v := &PromptValidator{
		ProtectedBranches: []string{"main", "master", "release/*"},
	}

	tests := []struct {
		branch string
		want   bool
	}{
		{"main", true},
		{"master", true},
		{"release/v1.0", true},
		{"feat/test", false},
		{"fix/bug", false},
	}

	for _, tt := range tests {
		t.Run(tt.branch, func(t *testing.T) {
			t.Parallel()
			if got := v.isProtectedBranch(tt.branch); got != tt.want {
				t.Errorf("isProtectedBranch(%q) = %v, want %v", tt.branch, got, tt.want)
			}
		})
	}
}

func TestActiveTaskID_MissingFile(t *testing.T) {
	t.Parallel()

	v := &PromptValidator{ProjectDir: t.TempDir()}
	if got := v.activeTaskID(); got != "" {
		t.Errorf("activeTaskID() = %q, want empty for missing file", got)
	}
}

func TestActiveTaskID_InvalidJSON(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	stateDir := filepath.Join(dir, ".state", "runtime")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(stateDir, "active-task.json"), []byte("not json"), 0o644); err != nil {
		t.Fatal(err)
	}

	v := &PromptValidator{ProjectDir: dir}
	if got := v.activeTaskID(); got != "" {
		t.Errorf("activeTaskID() = %q, want empty for invalid JSON", got)
	}
}

// TestExecCmdMutations groups all tests that mutate the package-level newExecCmd
// variable under a single parallel parent. Subtests run sequentially to avoid races.
func TestExecCmdMutations(t *testing.T) {
	// NOTE: no t.Parallel() -- all subtests mutate shared package-level var newExecCmd.

	// --- defaultGitContext ---

	t.Run("defaultGitContext/git commands succeed", func(t *testing.T) {
		orig := newExecCmd
		t.Cleanup(func() { newExecCmd = orig })

		newExecCmd = func(_ string, args ...string) *exec.Cmd {
			for _, arg := range args {
				if arg == "--show-current" {
					return exec.Command("echo", "feat/test-branch")
				}
				if arg == "--porcelain" {
					return exec.Command("printf", "M file1.go\nM file2.go\n")
				}
			}
			return exec.Command("echo", "")
		}

		branch, count := defaultGitContext("/tmp/fake-project")
		if branch != "feat/test-branch" {
			t.Errorf("branch = %q, want %q", branch, "feat/test-branch")
		}
		if count != 2 {
			t.Errorf("count = %d, want 2", count)
		}
	})

	t.Run("defaultGitContext/git commands fail", func(t *testing.T) {
		orig := newExecCmd
		t.Cleanup(func() { newExecCmd = orig })

		newExecCmd = func(_ string, _ ...string) *exec.Cmd {
			return exec.Command("false")
		}

		branch, count := defaultGitContext("/tmp/fake-project")
		if branch != "unknown" {
			t.Errorf("branch = %q, want %q", branch, "unknown")
		}
		if count != 0 {
			t.Errorf("count = %d, want 0", count)
		}
	})

	t.Run("defaultGitContext/no uncommitted changes", func(t *testing.T) {
		orig := newExecCmd
		t.Cleanup(func() { newExecCmd = orig })

		newExecCmd = func(_ string, args ...string) *exec.Cmd {
			for _, arg := range args {
				if arg == "--show-current" {
					return exec.Command("echo", "main")
				}
				if arg == "--porcelain" {
					return exec.Command("printf", "")
				}
			}
			return exec.Command("echo", "")
		}

		branch, count := defaultGitContext("/tmp/fake-project")
		if branch != "main" {
			t.Errorf("branch = %q, want %q", branch, "main")
		}
		if count != 0 {
			t.Errorf("count = %d, want 0", count)
		}
	})

	// --- runGitCmd ---

	t.Run("runGitCmd/success", func(t *testing.T) {
		orig := newExecCmd
		t.Cleanup(func() { newExecCmd = orig })

		newExecCmd = func(_ string, _ ...string) *exec.Cmd {
			return exec.Command("echo", "test-output")
		}

		result := runGitCmd("/tmp/fake", "branch", "--show-current")
		if result != "test-output" {
			t.Errorf("runGitCmd() = %q, want %q", result, "test-output")
		}
	})

	t.Run("runGitCmd/failure returns empty", func(t *testing.T) {
		orig := newExecCmd
		t.Cleanup(func() { newExecCmd = orig })

		newExecCmd = func(_ string, _ ...string) *exec.Cmd {
			return exec.Command("false")
		}

		result := runGitCmd("/tmp/fake", "branch", "--show-current")
		if result != "" {
			t.Errorf("runGitCmd() = %q, want empty on failure", result)
		}
	})

	// --- gitContext fallback ---

	t.Run("gitContextFallback/fallback to defaultGitContext", func(t *testing.T) {
		orig := newExecCmd
		t.Cleanup(func() { newExecCmd = orig })

		newExecCmd = func(_ string, args ...string) *exec.Cmd {
			for _, arg := range args {
				if arg == "--show-current" {
					return exec.Command("echo", "feat/fallback")
				}
				if arg == "--porcelain" {
					return exec.Command("printf", "M file.go\n")
				}
			}
			return exec.Command("echo", "")
		}

		v := &PromptValidator{
			ProjectDir:        "/tmp/fake-project",
			ProtectedBranches: DefaultProtectedBranches(),
			// No GitRunner — force fallback.
		}

		branch, count := v.gitContext()
		if branch != "feat/fallback" {
			t.Errorf("branch = %q, want %q", branch, "feat/fallback")
		}
		if count != 1 {
			t.Errorf("count = %d, want 1", count)
		}
	})
}

func TestDefaultPathFlowCheck(t *testing.T) {
	t.Parallel()

	t.Run("no session dir", func(t *testing.T) {
		t.Parallel()
		if defaultPathFlowCheck(t.TempDir()) {
			t.Error("should return false when session dir missing")
		}
	})

	t.Run("pathflow active", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		pfDir := filepath.Join(dir, ".state", "session", "ses-123", "pathflow")
		if err := os.MkdirAll(pfDir, 0o755); err != nil {
			t.Fatal(err)
		}
		statusJSON := `{"session_id":"ses-123","status":"pf-in-progress","team_name":"test"}`
		if err := os.WriteFile(filepath.Join(pfDir, "pathflow-session-status.json"), []byte(statusJSON), 0o644); err != nil {
			t.Fatal(err)
		}

		if !defaultPathFlowCheck(dir) {
			t.Error("should return true when session status is active")
		}
	})

	t.Run("no status file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		sessionDir := filepath.Join(dir, ".state", "session", "ses-123", "pathflow")
		if err := os.MkdirAll(sessionDir, 0o755); err != nil {
			t.Fatal(err)
		}
		// No status file.
		if defaultPathFlowCheck(dir) {
			t.Error("should return false when no status file")
		}
	})
}
