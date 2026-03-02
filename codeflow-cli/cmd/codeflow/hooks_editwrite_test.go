package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/hooks/security"
)

func TestNewEditWriteGuardCmd(t *testing.T) {
	t.Parallel()

	cmd := newEditWriteGuardCmd()
	if cmd.Use != "edit-write-guard" {
		t.Errorf("Use = %q, want %q", cmd.Use, "edit-write-guard")
	}
}

func TestRunEditWriteGuard_EmptyStdin(t *testing.T) {
	t.Parallel()

	stdin := strings.NewReader("")
	var outBuf, errBuf bytes.Buffer

	err := runEditWriteGuard(stdin, &outBuf, &errBuf)
	if err != nil {
		t.Errorf("runEditWriteGuard() error = %v, want nil for empty stdin", err)
	}
}

func TestRunEditWriteGuard_NonEditTool(t *testing.T) {
	t.Parallel()

	stdin := strings.NewReader(`{"tool_name":"Bash","tool_input":{"command":"ls"}}`)
	var outBuf, errBuf bytes.Buffer

	err := runEditWriteGuard(stdin, &outBuf, &errBuf)
	if err != nil {
		t.Errorf("runEditWriteGuard() error = %v, want nil for non-Edit tool", err)
	}
}

func TestBuildScopeChecker_NoPolicy(t *testing.T) {
	t.Parallel()

	checker := buildScopeChecker(t.TempDir())
	if checker == nil {
		t.Fatal("buildScopeChecker() returned nil")
	}
	if len(checker.BlockedDirs) == 0 {
		t.Error("should have default blocked dirs")
	}
	if len(checker.AllowedTmpPrefixes) == 0 {
		t.Error("should have default allowed tmp prefixes")
	}
	if len(checker.ProtectedBranches) == 0 {
		t.Error("should have default protected branches")
	}
}

func TestBuildScopeChecker_WithPolicy(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}

	policy := security.EnforcementPolicy{
		ProtectedBranches: []string{"main", "production"},
	}
	policy.EditWrite.BlockedDirectories = []string{".git", "vendor"}
	policy.EditWrite.AllowedTmpPrefixes = []string{"/tmp/test/"}
	policy.EditWrite.DangerousExtensions.Binary = []string{"exe"}
	policy.EditWrite.DangerousExtensions.Credential = []string{"pem"}
	policy.EditWrite.DangerousExtensions.Archive = []string{"zip"}
	policy.EditWrite.WarnOnDangerous = true

	data, _ := json.Marshal(policy)
	if err := os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	checker := buildScopeChecker(dir)
	if len(checker.BlockedDirs) != 2 || checker.BlockedDirs[0] != ".git" {
		t.Errorf("BlockedDirs = %v, want [.git vendor]", checker.BlockedDirs)
	}
	if len(checker.AllowedTmpPrefixes) != 1 || checker.AllowedTmpPrefixes[0] != "/tmp/test/" {
		t.Errorf("AllowedTmpPrefixes = %v, want [/tmp/test/]", checker.AllowedTmpPrefixes)
	}
	if !checker.WarnOnDangerous {
		t.Error("WarnOnDangerous should be true")
	}
}

func TestRunEditWriteGuard_BlockedPath(t *testing.T) {
	t.Parallel()

	// Edit a file in .git/ directory — should be blocked (exit 2).
	input := map[string]interface{}{
		"tool_name":  "Edit",
		"tool_input": map[string]string{"file_path": "/some/project/.git/config"},
	}
	data, _ := json.Marshal(input)
	stdin := bytes.NewReader(data)
	var outBuf, errBuf bytes.Buffer

	err := runEditWriteGuard(stdin, &outBuf, &errBuf)
	if err == nil {
		t.Fatal("runEditWriteGuard() error = nil, want exitError for blocked path")
	}

	var exitErr *exitError
	if !errors.As(err, &exitErr) {
		t.Fatalf("error type = %T, want *exitError", err)
	}
	if exitErr.code != ExitHookBlock {
		t.Errorf("exit code = %d, want %d", exitErr.code, ExitHookBlock)
	}
	if errBuf.Len() == 0 {
		t.Error("stderr should contain block message")
	}
	if !strings.Contains(errBuf.String(), "BLOCKED") {
		t.Errorf("stderr = %q, should contain 'BLOCKED'", errBuf.String())
	}
}

func TestRunEditWriteGuard_DangerousExtensionWarning(t *testing.T) {
	t.Parallel()

	// Write to a .exe file in a project directory — should allow with warning.
	// Use the actual project dir so containment check passes.
	projectDir := detectProjectDir()
	filePath := filepath.Join(projectDir, "test.exe")

	input := map[string]interface{}{
		"tool_name":  "Write",
		"tool_input": map[string]string{"file_path": filePath},
	}
	data, _ := json.Marshal(input)
	stdin := bytes.NewReader(data)
	var outBuf, errBuf bytes.Buffer

	err := runEditWriteGuard(stdin, &outBuf, &errBuf)
	if err != nil {
		t.Fatalf("runEditWriteGuard() error = %v, want nil for warned-but-allowed", err)
	}
	if errBuf.Len() == 0 {
		t.Error("stderr should contain warning for dangerous extension")
	}
	if !strings.Contains(errBuf.String(), "WARNING") {
		t.Errorf("stderr = %q, should contain 'WARNING'", errBuf.String())
	}
}

func TestRunEditWriteGuard_ViaCobraCommand(t *testing.T) {
	t.Parallel()

	// Execute via the Cobra command's RunE to cover the closure path.
	cmd := newEditWriteGuardCmd()
	cmd.SetIn(strings.NewReader(""))
	var outBuf, errBuf bytes.Buffer
	cmd.SetOut(&outBuf)
	cmd.SetErr(&errBuf)

	err := cmd.Execute()
	if err != nil {
		t.Errorf("cmd.Execute() error = %v, want nil for empty stdin", err)
	}
}

func TestBuildScopeChecker_PolicyWithDefaults(t *testing.T) {
	t.Parallel()

	// Policy exists but has empty fields — should fall back to defaults.
	dir := t.TempDir()
	configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Minimal policy: no blocked dirs, no tmp prefixes, no dangerous exts,
	// no protected branches — all should default.
	policy := security.EnforcementPolicy{}
	data, _ := json.Marshal(policy)
	if err := os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	checker := buildScopeChecker(dir)
	if len(checker.BlockedDirs) == 0 {
		t.Error("should have default blocked dirs when policy has empty list")
	}
	if len(checker.AllowedTmpPrefixes) == 0 {
		t.Error("should have default allowed tmp prefixes when policy has empty list")
	}
	if len(checker.DangerousExts.Binary) == 0 {
		t.Error("should have default dangerous extensions when policy has empty list")
	}
	if len(checker.ProtectedBranches) == 0 {
		t.Error("should have default protected branches when policy has empty list")
	}
}

func TestEditWriteGuardSubcommand(t *testing.T) {
	t.Parallel()

	// Verify it's registered under pre-tool-use.
	cmd := newPreToolUseCmd()
	subs := cmd.Commands()
	found := false
	for _, sub := range subs {
		if sub.Name() == "edit-write-guard" {
			found = true
			break
		}
	}
	if !found {
		t.Error("newPreToolUseCmd() missing subcommand 'edit-write-guard'")
	}
}
