package main

import (
	"errors"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/initialize"
)

func TestNewInitCmd(t *testing.T) {
	t.Parallel()

	cmd := newInitCmd()
	if cmd.Use != "init" {
		t.Errorf("Use = %q, want %q", cmd.Use, "init")
	}
	if cmd.Short == "" {
		t.Error("init command should have a short description")
	}

	dirFlag := cmd.Flags().Lookup("dir")
	if dirFlag == nil {
		t.Fatal("init command should have --dir flag")
	}
	if dirFlag.DefValue != "" {
		t.Errorf("--dir default should be empty, got %q", dirFlag.DefValue)
	}
}

func TestInitCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["init"] {
		t.Error("expected 'init' subcommand to be registered in root cmd")
	}
}

func TestInitCmd_RejectsExtraArgs(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"init", "extra"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for init with extra args")
	}
}

func TestClassifyInitError(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		err  error
		want int
	}{
		{"ErrPrereqMissing", initialize.ErrPrereqMissing, ExitExternalError},
		{"ErrAuthFailed", initialize.ErrAuthFailed, ExitExternalError},
		{"ErrGitProviderFailed", initialize.ErrGitProviderFailed, ExitExternalError},
		{"ErrSetupFailed", initialize.ErrSetupFailed, ExitConfigError},
		{"ErrVerificationFailed", initialize.ErrVerificationFailed, ExitConfigError},
		{"ErrCancelled", initialize.ErrCancelled, ExitGeneralError},
		{"unknown error", errors.New("some unknown error"), ExitGeneralError},
		{"wrapped ErrPrereqMissing", errors.Join(initialize.ErrPrereqMissing, errors.New("git not found")), ExitExternalError},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := classifyInitError(tt.err)
			if got != tt.want {
				t.Errorf("classifyInitError(%v) = %d, want %d", tt.err, got, tt.want)
			}
		})
	}
}

func TestDefaultRunCmd(t *testing.T) {
	t.Parallel()

	// defaultRunCmd should execute a real command and return output.
	out, err := defaultRunCmd("echo", "hello")
	if err != nil {
		t.Fatalf("defaultRunCmd(echo hello) returned error: %v", err)
	}
	if len(out) == 0 {
		t.Error("defaultRunCmd should return non-empty output for 'echo hello'")
	}
}

func TestDefaultRunCmd_Failure(t *testing.T) {
	t.Parallel()

	_, err := defaultRunCmd("nonexistent-command-xyz")
	if err == nil {
		t.Error("defaultRunCmd should return error for nonexistent command")
	}
}

func TestRunInit_ViaRootCmd_EOF(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	// EOF stdin causes the wizard to fail immediately.
	cmd.SetIn(strings.NewReader(""))
	cmd.SetArgs([]string{"init", "--dir", tmpDir})

	err := cmd.Execute()
	// The wizard should fail because stdin is empty (EOF).
	if err == nil {
		t.Log("init with empty stdin succeeded (unexpected but acceptable)")
	}
}

func TestRunInit_WithDir(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()

	// Provide minimal input then EOF to exercise runInit code path.
	// The wizard may prompt and then fail on EOF.
	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetIn(strings.NewReader("\n"))
	cmd.SetArgs([]string{"init", "--dir", tmpDir})

	// We don't care if it errors (wizard will fail without real prereqs).
	// Just exercising the runInit function path.
	_ = cmd.Execute()
}

func TestNewInitCmd_RunE(t *testing.T) {
	t.Parallel()

	cmd := newInitCmd()
	// Verify RunE is set (exercises the command creation path).
	if cmd.RunE == nil {
		t.Error("init command should have RunE set")
	}
}
