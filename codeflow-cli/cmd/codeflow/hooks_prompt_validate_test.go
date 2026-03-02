package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/hooks/security"
)

func TestNewPromptValidateCmd(t *testing.T) {
	t.Parallel()

	cmd := newPromptValidateCmd()
	if cmd.Use != "validate" {
		t.Errorf("Use = %q, want %q", cmd.Use, "validate")
	}
}

func TestRunPromptValidate_EmptyStdin(t *testing.T) {
	t.Parallel()

	stdin := strings.NewReader("")
	var outBuf, errBuf bytes.Buffer

	err := runPromptValidate(stdin, &outBuf, &errBuf)
	if err != nil {
		t.Errorf("runPromptValidate() error = %v, want nil for empty stdin", err)
	}
}

func TestRunPromptValidate_NeverBlocks(t *testing.T) {
	t.Parallel()

	// Even with invalid JSON, should never return an error (never block).
	stdin := strings.NewReader("{invalid json}")
	var outBuf, errBuf bytes.Buffer

	err := runPromptValidate(stdin, &outBuf, &errBuf)
	if err != nil {
		t.Errorf("runPromptValidate() error = %v, want nil (never blocks)", err)
	}
}

func TestBuildPromptValidator_NoPolicy(t *testing.T) {
	t.Parallel()

	v := buildPromptValidator(t.TempDir())
	if v == nil {
		t.Fatal("buildPromptValidator() returned nil")
	}
	if len(v.ProtectedBranches) == 0 {
		t.Error("should have default protected branches")
	}
}

func TestBuildPromptValidator_WithPolicy(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}

	policy := security.EnforcementPolicy{
		ProtectedBranches: []string{"main", "staging"},
	}

	data, _ := json.Marshal(policy)
	if err := os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	v := buildPromptValidator(dir)
	if len(v.ProtectedBranches) != 2 {
		t.Errorf("ProtectedBranches length = %d, want 2", len(v.ProtectedBranches))
	}
	if v.ProtectedBranches[0] != "main" || v.ProtectedBranches[1] != "staging" {
		t.Errorf("ProtectedBranches = %v, want [main staging]", v.ProtectedBranches)
	}
}

func TestRunPromptValidate_ViaCobraCommand(t *testing.T) {
	t.Parallel()

	// Execute via the Cobra command's RunE to cover the closure path.
	cmd := newPromptValidateCmd()
	cmd.SetIn(strings.NewReader("{}"))
	var outBuf, errBuf bytes.Buffer
	cmd.SetOut(&outBuf)
	cmd.SetErr(&errBuf)

	err := cmd.Execute()
	if err != nil {
		t.Errorf("cmd.Execute() error = %v, want nil (never blocks)", err)
	}
}

func TestRunPromptValidate_OutputsReminders(t *testing.T) {
	t.Parallel()

	// Run with valid session input — should produce at least the
	// "no active task" reminder since we're in a temp dir with no state.
	stdin := strings.NewReader(`{"session_id":"test-session-123"}`)
	var outBuf, errBuf bytes.Buffer

	err := runPromptValidate(stdin, &outBuf, &errBuf)
	if err != nil {
		t.Fatalf("runPromptValidate() error = %v, want nil", err)
	}

	// Should have at least one reminder in output (no active task).
	output := outBuf.String()
	if !strings.Contains(output, "user-prompt-submit-hook") {
		t.Errorf("output should contain hook tags; got: %s", output)
	}
}

func TestBuildPromptValidator_PolicyWithEmptyBranches(t *testing.T) {
	t.Parallel()

	// Policy exists but has no protected branches — should use defaults.
	dir := t.TempDir()
	configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}

	policy := security.EnforcementPolicy{} // No ProtectedBranches set.
	data, _ := json.Marshal(policy)
	if err := os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	v := buildPromptValidator(dir)
	if len(v.ProtectedBranches) == 0 {
		t.Error("should have default protected branches when policy has empty list")
	}
}

func TestPromptValidateSubcommand(t *testing.T) {
	t.Parallel()

	// Verify it's registered under user-prompt-submit.
	cmd := newUserPromptSubmitCmd()
	subs := cmd.Commands()
	found := false
	for _, sub := range subs {
		if sub.Name() == "validate" {
			found = true
			break
		}
	}
	if !found {
		t.Error("newUserPromptSubmitCmd() missing subcommand 'validate'")
	}
}
