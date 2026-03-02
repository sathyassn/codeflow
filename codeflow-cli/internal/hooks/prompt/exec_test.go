package prompt

import (
	"os/exec"
	"testing"
)

func TestNewExecCmd_Default(t *testing.T) {
	t.Parallel()

	// Verify newExecCmd defaults to exec.Command.
	cmd := newExecCmd("echo", "hello")
	if cmd == nil {
		t.Fatal("newExecCmd returned nil")
	}
	if cmd.Path == "" {
		t.Error("newExecCmd should produce a valid Cmd with a non-empty Path")
	}
}

func TestNewExecCmd_Replaceable(t *testing.T) {
	// NOTE: no t.Parallel() -- mutates package-level var newExecCmd.
	orig := newExecCmd
	t.Cleanup(func() { newExecCmd = orig })

	called := false
	newExecCmd = func(name string, args ...string) *exec.Cmd {
		called = true
		return exec.Command(name, args...)
	}

	cmd := newExecCmd("echo", "test")
	if !called {
		t.Error("replacement function was not called")
	}
	if cmd == nil {
		t.Fatal("replacement returned nil")
	}
}
