package main

import (
	"strings"
	"testing"
)

func TestNewUpdateCmd(t *testing.T) {
	t.Parallel()

	cmd := newUpdateCmd()
	if cmd.Use != "update" {
		t.Errorf("Use = %q, want %q", cmd.Use, "update")
	}
	if cmd.Short == "" {
		t.Error("update command should have a short description")
	}
}

func TestUpdateCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newUpdateCmd()

	checkFlag := cmd.Flags().Lookup("check")
	if checkFlag == nil {
		t.Fatal("update command should have --check flag")
	}
	if checkFlag.DefValue != "false" {
		t.Errorf("--check default should be 'false', got %q", checkFlag.DefValue)
	}

	forceFlag := cmd.Flags().Lookup("force")
	if forceFlag == nil {
		t.Fatal("update command should have --force flag")
	}
	if forceFlag.DefValue != "false" {
		t.Errorf("--force default should be 'false', got %q", forceFlag.DefValue)
	}
}

func TestUpdateCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["update"] {
		t.Error("expected 'update' subcommand to be registered in root cmd")
	}
}

func TestUpdateCmd_RejectsExtraArgs(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"update", "extra"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for update with extra args")
	}
}

func TestUpdateCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"update", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("update --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"update", "--check", "--force"} {
		if !strings.Contains(got, want) {
			t.Errorf("update help missing %q:\n%s", want, got)
		}
	}
}
