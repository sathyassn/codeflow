package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewConfigCmd(t *testing.T) {
	t.Parallel()

	cmd := newConfigCmd()
	if cmd.Use != "config" {
		t.Errorf("Use = %q, want %q", cmd.Use, "config")
	}
	if cmd.Short == "" {
		t.Error("config command should have a short description")
	}

	// Verify --global persistent flag.
	globalFlag := cmd.PersistentFlags().Lookup("global")
	if globalFlag == nil {
		t.Fatal("config command should have --global persistent flag")
	}
	if globalFlag.DefValue != "false" {
		t.Errorf("--global default should be 'false', got %q", globalFlag.DefValue)
	}
}

func TestConfigCmd_Subcommands(t *testing.T) {
	t.Parallel()

	cmd := newConfigCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}

	for _, name := range []string{"list", "get", "set"} {
		if !subcommands[name] {
			t.Errorf("expected subcommand %q to be registered on config cmd", name)
		}
	}
}

func TestConfigCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["config"] {
		t.Error("expected 'config' subcommand to be registered in root cmd")
	}
}

func TestConfigCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"config", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("config --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"config", "list", "get", "set", "--global"} {
		if !strings.Contains(got, want) {
			t.Errorf("config help missing %q:\n%s", want, got)
		}
	}
}

func TestConfigGetCmd_RequiresExactlyOneArg(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"config", "get"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for config get with no args")
	}
}

func TestConfigGetCmd_RejectsExtraArgs(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"config", "get", "key1", "key2"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for config get with extra args")
	}
}

func TestConfigSetCmd_RequiresExactlyTwoArgs(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"config", "set"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for config set with no args")
	}
}

func TestConfigSetCmd_RejectsOneArg(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"config", "set", "key"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for config set with only one arg")
	}
}

func TestConfigListCmd_RejectsArgs(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"config", "list", "extra"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for config list with args")
	}
}

func TestConfigOpts(t *testing.T) {
	t.Parallel()

	opts := configOpts()
	if opts.ProjectDir != "." {
		t.Errorf("configOpts().ProjectDir = %q, want %q", opts.ProjectDir, ".")
	}
	if opts.GlobalDir == "" {
		t.Error("configOpts().GlobalDir should not be empty")
	}
}

func TestRunConfigList_EmptyConfig(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is not parallel-safe
	tmpDir := t.TempDir()
	// Create the .codeflow/config directory (local) and a global dir.
	localDir := filepath.Join(tmpDir, ".codeflow", "config")
	globalDir := filepath.Join(tmpDir, "global-config")
	if err := os.MkdirAll(localDir, 0o755); err != nil {
		t.Fatalf("creating local config dir: %v", err)
	}
	if err := os.MkdirAll(globalDir, 0o755); err != nil {
		t.Fatalf("creating global config dir: %v", err)
	}

	// Override configOpts by running from within tmpDir.
	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"config", "list", "--global"})

	// With empty dirs, this should succeed with no output (or empty output).
	err := cmd.Execute()
	if err != nil {
		t.Fatalf("config list --global returned error: %v", err)
	}
}

func TestRunConfigSet_ThenGet(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is not parallel-safe
	tmpDir := t.TempDir()
	localDir := filepath.Join(tmpDir, ".codeflow", "config")
	globalDir := filepath.Join(tmpDir, "global-config")
	for _, dir := range []string{localDir, globalDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatalf("creating dir: %v", err)
		}
	}

	// Pre-populate a config file so the key exists for Set to validate.
	configJSON := `{"key": "original"}`
	if err := os.WriteFile(filepath.Join(localDir, "test.json"), []byte(configJSON), 0o644); err != nil {
		t.Fatalf("writing config file: %v", err)
	}

	// Change to tmpDir so configOpts uses it as ProjectDir.
	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	// Set a value (must use a key that already exists: "test.key" = file "test" + key "key").
	cmd := newRootCmd()
	var setBuf strings.Builder
	cmd.SetOut(&setBuf)
	cmd.SetArgs([]string{"config", "set", "test.key", "hello"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("config set returned error: %v", err)
	}

	setGot := setBuf.String()
	if !strings.Contains(setGot, "test.key = hello") {
		t.Errorf("output missing %q: %q", "test.key = hello", setGot)
	}

	// Get the value back.
	cmd2 := newRootCmd()
	var getBuf strings.Builder
	cmd2.SetOut(&getBuf)
	cmd2.SetArgs([]string{"config", "get", "test.key"})

	if err := cmd2.Execute(); err != nil {
		t.Fatalf("config get returned error: %v", err)
	}

	getGot := strings.TrimSpace(getBuf.String())
	if getGot != "hello" {
		t.Errorf("config get = %q, want %q", getGot, "hello")
	}
}

func TestRunConfigList_WithValues(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is not parallel-safe
	tmpDir := t.TempDir()
	localDir := filepath.Join(tmpDir, ".codeflow", "config")
	if err := os.MkdirAll(localDir, 0o755); err != nil {
		t.Fatalf("creating local config dir: %v", err)
	}

	// Write a config file.
	configJSON := `{"alpha": "one", "beta": "two"}`
	if err := os.WriteFile(filepath.Join(localDir, "test.json"), []byte(configJSON), 0o644); err != nil {
		t.Fatalf("writing config file: %v", err)
	}

	oldWd, _ := os.Getwd()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"config", "list"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("config list returned error: %v", err)
	}

	got := buf.String()
	// Keys are namespaced: file "test.json" + key "alpha" = "test.alpha".
	if !strings.Contains(got, "test.alpha") {
		t.Errorf("output missing %q: %q", "test.alpha", got)
	}
	if !strings.Contains(got, "test.beta") {
		t.Errorf("output missing %q: %q", "test.beta", got)
	}
}
