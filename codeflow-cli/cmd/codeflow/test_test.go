package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewTestCmd(t *testing.T) {
	t.Parallel()

	cmd := newTestCmd()
	if cmd.Use != "test" {
		t.Errorf("Use = %q, want %q", cmd.Use, "test")
	}
	if cmd.Short == "" {
		t.Error("test command should have a short description")
	}
}

func TestTestCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newTestCmd()

	for _, tc := range []struct {
		name     string
		defValue string
	}{
		{"coverage", "false"},
		{"mode", ""},
		{"verbose", "false"},
	} {
		flag := cmd.Flags().Lookup(tc.name)
		if flag == nil {
			t.Errorf("test command should have --%s flag", tc.name)
			continue
		}
		if flag.DefValue != tc.defValue {
			t.Errorf("--%s default should be %q, got %q", tc.name, tc.defValue, flag.DefValue)
		}
	}
}

func TestTestCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["test"] {
		t.Error("expected 'test' subcommand to be registered in root cmd")
	}
}

func TestTestCmd_RejectsExtraArgs(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"test", "extra"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for test with extra args")
	}
}

func TestTestCmd_InvalidMode(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir
	tmpDir := t.TempDir()

	// Create minimal project structure.
	if err := os.MkdirAll(filepath.Join(tmpDir, ".codeflow", "testing"), 0o755); err != nil {
		t.Fatal(err)
	}
	// Create a minimal test runner script.
	script := filepath.Join(tmpDir, ".codeflow", "testing", "run-all-tests.sh")
	if err := os.WriteFile(script, []byte("#!/bin/bash\nexit 0\n"), 0o755); err != nil {
		t.Fatal(err)
	}

	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"test", "--mode", "bogus"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for invalid mode")
	}
}

func TestTestCmd_ScriptNotFound(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir
	tmpDir := t.TempDir()

	// Create .codeflow/ but no testing/run-all-tests.sh.
	if err := os.MkdirAll(filepath.Join(tmpDir, ".codeflow"), 0o755); err != nil {
		t.Fatal(err)
	}

	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"test"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error when test runner script missing")
	}
}

func TestTestCmd_RunsScript(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir
	tmpDir := t.TempDir()

	// Create project structure with a script that writes a marker file.
	testingDir := filepath.Join(tmpDir, ".codeflow", "testing")
	if err := os.MkdirAll(testingDir, 0o755); err != nil {
		t.Fatal(err)
	}

	markerPath := filepath.Join(tmpDir, "test-ran.marker")
	scriptContent := "#!/bin/bash\ntouch " + markerPath + "\n"
	script := filepath.Join(testingDir, "run-all-tests.sh")
	if err := os.WriteFile(script, []byte(scriptContent), 0o755); err != nil {
		t.Fatal(err)
	}

	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"test"})

	err := cmd.Execute()
	if err != nil {
		t.Fatalf("test command returned error: %v", err)
	}

	if _, err := os.Stat(markerPath); os.IsNotExist(err) {
		t.Error("test runner script was not executed (marker file missing)")
	}
}

func TestTestCmd_PassesFlags(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir
	tmpDir := t.TempDir()

	testingDir := filepath.Join(tmpDir, ".codeflow", "testing")
	if err := os.MkdirAll(testingDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Script records its arguments to a file.
	argsPath := filepath.Join(tmpDir, "test-args.txt")
	scriptContent := `#!/bin/bash
echo "$@" > ` + argsPath + `
`
	script := filepath.Join(testingDir, "run-all-tests.sh")
	if err := os.WriteFile(script, []byte(scriptContent), 0o755); err != nil {
		t.Fatal(err)
	}

	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"test", "--coverage", "--mode", "full", "--verbose"})

	err := cmd.Execute()
	if err != nil {
		t.Fatalf("test command returned error: %v", err)
	}

	argsData, err := os.ReadFile(argsPath)
	if err != nil {
		t.Fatalf("reading args file: %v", err)
	}

	args := strings.TrimSpace(string(argsData))
	if !strings.Contains(args, "--coverage") {
		t.Errorf("args missing --coverage: %q", args)
	}
	if !strings.Contains(args, "--mode full") {
		t.Errorf("args missing '--mode full': %q", args)
	}
	if !strings.Contains(args, "--verbose") {
		t.Errorf("args missing --verbose: %q", args)
	}
}

func TestFindProjectRoot(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir
	tmpDir := t.TempDir()

	// Create .codeflow/ in the temp dir.
	if err := os.MkdirAll(filepath.Join(tmpDir, ".codeflow"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create a subdirectory to chdir into.
	subDir := filepath.Join(tmpDir, "sub", "deep")
	if err := os.MkdirAll(subDir, 0o755); err != nil {
		t.Fatal(err)
	}

	oldWd, _ := os.Getwd()
	if err := os.Chdir(subDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	root, err := findProjectRoot()
	if err != nil {
		t.Fatalf("findProjectRoot returned error: %v", err)
	}

	// Resolve symlinks for comparison.
	expectedRoot, _ := filepath.EvalSymlinks(tmpDir)
	actualRoot, _ := filepath.EvalSymlinks(root)
	if actualRoot != expectedRoot {
		t.Errorf("findProjectRoot = %q, want %q", actualRoot, expectedRoot)
	}
}

func TestFindProjectRoot_NotFound(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir
	tmpDir := t.TempDir()
	// No .codeflow/ anywhere.

	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	_, err := findProjectRoot()
	if err == nil {
		t.Error("expected error when no project root found")
	}
}

func TestValidTestModes(t *testing.T) {
	t.Parallel()

	expected := []string{"standard", "quick", "full", "essential"}
	for _, mode := range expected {
		if !validTestModes[mode] {
			t.Errorf("mode %q should be valid", mode)
		}
	}

	if validTestModes["bogus"] {
		t.Error("mode 'bogus' should not be valid")
	}
}
