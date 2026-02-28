package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewHooksCmd(t *testing.T) {
	t.Parallel()

	cmd := newHooksCmd()
	if cmd.Use != "hooks" {
		t.Errorf("newHooksCmd().Use = %q, want %q", cmd.Use, "hooks")
	}
	if !cmd.HasSubCommands() {
		t.Error("newHooksCmd() has no subcommands, want pre-tool-use")
	}

	// Verify subcommand names.
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["pre-tool-use"] {
		t.Error("newHooksCmd() missing subcommand 'pre-tool-use'")
	}
}

func TestNewPreToolUseCmd(t *testing.T) {
	t.Parallel()

	cmd := newPreToolUseCmd()
	if cmd.Use != "pre-tool-use" {
		t.Errorf("newPreToolUseCmd().Use = %q, want %q", cmd.Use, "pre-tool-use")
	}
	if !cmd.HasSubCommands() {
		t.Error("newPreToolUseCmd() has no subcommands, want security")
	}

	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["security"] {
		t.Error("newPreToolUseCmd() missing subcommand 'security'")
	}
}

func TestNewSecurityCmd(t *testing.T) {
	t.Parallel()

	cmd := newSecurityCmd()
	if cmd.Use != "security" {
		t.Errorf("newSecurityCmd().Use = %q, want %q", cmd.Use, "security")
	}
}

func TestRunSecurity(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		stdin     string
		wantErr   bool
		wantBlock bool
		wantStderr string
	}{
		{
			name:  "non-bash tool allowed",
			stdin: `{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`,
		},
		{
			name:  "empty command allowed",
			stdin: `{"tool_name":"Bash","tool_input":{"command":""}}`,
		},
		{
			name:  "safe command allowed",
			stdin: `{"tool_name":"Bash","tool_input":{"command":"ls -la"}}`,
		},
		{
			name:  "git status allowed",
			stdin: `{"tool_name":"Bash","tool_input":{"command":"git status"}}`,
		},
		{
			name:      "dangerous command blocked",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"rm -rf /"}}`,
			wantErr:   true,
			wantBlock: true,
			wantStderr: "BLOCKED",
		},
		{
			name:      "sudo blocked",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"sudo apt install"}}`,
			wantErr:   true,
			wantBlock: true,
			wantStderr: "Privilege Escalation",
		},
		{
			name:  "empty stdin allowed",
			stdin: "",
		},
		{
			name:  "invalid json allowed",
			stdin: "not json",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			stdin := strings.NewReader(tt.stdin)
			var stdout, stderr bytes.Buffer
			err := runSecurity(stdin, &stdout, &stderr)

			if tt.wantErr && err == nil {
				t.Error("runSecurity() error = nil, want error")
			}
			if !tt.wantErr && err != nil {
				t.Errorf("runSecurity() unexpected error: %v", err)
			}
			if tt.wantBlock {
				var ee *exitError
				if err == nil {
					t.Fatal("expected exitError for blocked command")
				}
				if ok := isExitError(err, &ee); !ok {
					t.Errorf("error type = %T, want *exitError", err)
				} else if ee.code != ExitHookBlock {
					t.Errorf("exit code = %d, want %d", ee.code, ExitHookBlock)
				}
			}
			if tt.wantStderr != "" && !strings.Contains(stderr.String(), tt.wantStderr) {
				t.Errorf("stderr = %q, want substring %q", stderr.String(), tt.wantStderr)
			}
		})
	}
}

// isExitError checks if err is an *exitError and sets target.
func isExitError(err error, target **exitError) bool {
	if e, ok := err.(*exitError); ok {
		*target = e
		return true
	}
	return false
}

func TestDetectProjectDir(t *testing.T) {
	t.Parallel()

	dir := detectProjectDir()
	// We're running inside a git repo, so it should return a non-empty path.
	if dir == "" || dir == "." {
		t.Errorf("detectProjectDir() = %q, want non-empty git root", dir)
	}
}

func TestDetectCurrentBranch(t *testing.T) {
	t.Parallel()

	branch := detectCurrentBranch()
	// We're running inside a git repo, so it should return a branch name.
	if branch == "" {
		t.Log("detectCurrentBranch() returned empty (detached HEAD or non-git), skipping assertion")
	}
}

func TestDetectPathFlowActive(t *testing.T) {
	t.Parallel()

	t.Run("empty session ID", func(t *testing.T) {
		t.Parallel()
		if detectPathFlowActive("/tmp", "") {
			t.Error("detectPathFlowActive() = true with empty sessionID, want false")
		}
	})

	t.Run("non-existent flag", func(t *testing.T) {
		t.Parallel()
		if detectPathFlowActive(t.TempDir(), "ses-test123") {
			t.Error("detectPathFlowActive() = true for non-existent flag, want false")
		}
	})

	t.Run("existing flag", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		sid := "ses-test123"
		flagDir := filepath.Join(dir, ".state", "session", sid, "pathflow")
		if err := os.MkdirAll(flagDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("1"), 0o644); err != nil {
			t.Fatal(err)
		}
		if !detectPathFlowActive(dir, sid) {
			t.Error("detectPathFlowActive() = false with existing flag, want true")
		}
	})
}

func TestExitHookBlockConstant(t *testing.T) {
	t.Parallel()
	if ExitHookBlock != 2 {
		t.Errorf("ExitHookBlock = %d, want 2", ExitHookBlock)
	}
}

func TestHooksCmdHelp(t *testing.T) {
	t.Parallel()

	// Execute the hooks command with no subcommand — triggers RunE which calls Help().
	cmd := newHooksCmd()
	cmd.SetArgs([]string{})
	var out bytes.Buffer
	cmd.SetOut(&out)
	err := cmd.Execute()
	if err != nil {
		t.Errorf("hooks help returned error: %v", err)
	}
	if !strings.Contains(out.String(), "Subcommands invoked by Claude Code hooks") {
		t.Errorf("hooks help output = %q, want to contain 'Subcommands invoked by Claude Code hooks'", out.String())
	}
}

func TestPreToolUseCmdHelp(t *testing.T) {
	t.Parallel()

	cmd := newPreToolUseCmd()
	cmd.SetArgs([]string{})
	var out bytes.Buffer
	cmd.SetOut(&out)
	err := cmd.Execute()
	if err != nil {
		t.Errorf("pre-tool-use help returned error: %v", err)
	}
	if !strings.Contains(out.String(), "Enforcement modules that run before tool execution") {
		t.Errorf("pre-tool-use help output = %q, want to contain 'Enforcement modules'", out.String())
	}
}

func TestSecurityCmdViaRoot(t *testing.T) {
	t.Parallel()

	// Execute the full command path to cover newSecurityCmd's RunE.
	root := newHooksCmd()
	root.SetArgs([]string{"pre-tool-use", "security"})
	// Provide safe stdin (non-Bash tool) so it doesn't block.
	root.SetIn(strings.NewReader(`{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`))
	var stdout, stderr bytes.Buffer
	root.SetOut(&stdout)
	root.SetErr(&stderr)
	err := root.Execute()
	if err != nil {
		t.Errorf("security command returned error: %v", err)
	}
}

func TestDetectProjectDirFallback(t *testing.T) {
	t.Parallel()

	// Run detectProjectDir in a non-git temp directory.
	// Save and restore cwd.
	origDir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	tmpDir := t.TempDir()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(origDir) })

	dir := detectProjectDir()
	// Should fallback to cwd since tmpDir is not a git repo.
	if dir == "" {
		t.Error("detectProjectDir() returned empty in non-git dir, want cwd fallback")
	}
}

func TestDetectCurrentBranchNonGit(t *testing.T) {
	t.Parallel()

	// In a non-git directory, detectCurrentBranch returns empty.
	origDir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	tmpDir := t.TempDir()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(origDir) })

	branch := detectCurrentBranch()
	if branch != "" {
		t.Errorf("detectCurrentBranch() = %q in non-git dir, want empty", branch)
	}
}

func TestNewGateCheckCmd(t *testing.T) {
	t.Parallel()

	cmd := newGateCheckCmd()
	if cmd.Use != "gate-check" {
		t.Errorf("newGateCheckCmd().Use = %q, want %q", cmd.Use, "gate-check")
	}

	// Verify --event flag exists with default.
	f := cmd.Flags().Lookup("event")
	if f == nil {
		t.Fatal("gate-check command missing --event flag")
	}
	if f.DefValue != "PreToolUse" {
		t.Errorf("--event default = %q, want %q", f.DefValue, "PreToolUse")
	}
}

func TestGateCheckCmdViaRoot(t *testing.T) {
	t.Parallel()

	// Execute the full command path: hooks pre-tool-use gate-check
	// No PathFlow active (CODEFLOW_SESSION_ID not set) → should allow.
	root := newHooksCmd()
	root.SetArgs([]string{"pre-tool-use", "gate-check"})
	root.SetIn(strings.NewReader(`{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}`))
	var stdout, stderr bytes.Buffer
	root.SetOut(&stdout)
	root.SetErr(&stderr)
	err := root.Execute()
	if err != nil {
		t.Errorf("gate-check command returned error: %v", err)
	}
}

func TestPreToolUseCmdHasGateCheck(t *testing.T) {
	t.Parallel()

	cmd := newPreToolUseCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["gate-check"] {
		t.Error("newPreToolUseCmd() missing subcommand 'gate-check'")
	}
}

func TestRunGateCheck(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		stdin      string
		event      string
		wantErr    bool
		wantBlock  bool
		wantStderr string
	}{
		{
			name:  "Stop event always allows",
			stdin: `{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}`,
			event: "Stop",
		},
		{
			name:  "SubagentStop event always allows",
			stdin: `{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}`,
			event: "SubagentStop",
		},
		{
			name:  "empty stdin allows",
			stdin: "",
			event: "PreToolUse",
		},
		{
			name:  "invalid JSON allows (graceful degradation)",
			stdin: "not json at all",
			event: "PreToolUse",
		},
		{
			name:  "non-gated tool allows (no PathFlow check needed)",
			stdin: `{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`,
			event: "PreToolUse",
		},
		{
			name:  "Edit tool allows when no PathFlow session active",
			stdin: `{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}`,
			event: "PreToolUse",
		},
		{
			name:  "Bash git status allows (ungated command)",
			stdin: `{"tool_name":"Bash","tool_input":{"command":"git status"}}`,
			event: "PreToolUse",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			stdin := strings.NewReader(tt.stdin)
			var stdout, stderr bytes.Buffer
			err := runGateCheck(stdin, &stdout, &stderr, tt.event)

			if tt.wantErr && err == nil {
				t.Error("runGateCheck() error = nil, want error")
			}
			if !tt.wantErr && err != nil {
				t.Errorf("runGateCheck() unexpected error: %v", err)
			}
			if tt.wantBlock {
				var ee *exitError
				if err == nil {
					t.Fatal("expected exitError for blocked operation")
				}
				if ok := isExitError(err, &ee); !ok {
					t.Errorf("error type = %T, want *exitError", err)
				} else if ee.code != ExitHookBlock {
					t.Errorf("exit code = %d, want %d", ee.code, ExitHookBlock)
				}
			}
			if tt.wantStderr != "" && !strings.Contains(stderr.String(), tt.wantStderr) {
				t.Errorf("stderr = %q, want substring %q", stderr.String(), tt.wantStderr)
			}
		})
	}
}

func TestRunGateCheckWithPathFlow(t *testing.T) {
	// NOTE: no t.Parallel -- modifies environment variables via t.Setenv and creates
	// real filesystem state in the project's .state/ directory.

	// Get the project root (same as detectProjectDir will find).
	projectDir := detectProjectDir()
	sessionID := "ses-test-gate-check-" + t.Name()

	// Create pathflow-active flag at the real project dir.
	flagDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	flagFile := filepath.Join(flagDir, "is-pathflow-active")
	if err := os.WriteFile(flagFile, []byte("1"), 0o644); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(filepath.Join(projectDir, ".state", "session", sessionID)) })

	// Create sentinel directory.
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(sentinelDir) })

	// Set CODEFLOW_SESSION_ID for detectPathFlowActive.
	t.Setenv("CODEFLOW_SESSION_ID", sessionID)

	t.Run("Edit blocked without pf-3", func(t *testing.T) {
		stdin := strings.NewReader(`{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}`)
		var stdout, stderr bytes.Buffer
		err := runGateCheck(stdin, &stdout, &stderr, "PreToolUse")
		if err == nil {
			t.Fatal("expected error for Edit without pf-3")
		}
		code := exitCode(err)
		if code != ExitHookBlock {
			t.Errorf("exit code = %d, want %d", code, ExitHookBlock)
		}
		if !strings.Contains(stderr.String(), "BLOCKED") {
			t.Errorf("stderr = %q, want substring 'BLOCKED'", stderr.String())
		}
	})

	t.Run("Edit allowed with pf-3", func(t *testing.T) {
		// Create pf-3 sentinel.
		if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-3"), []byte("1"), 0o644); err != nil {
			t.Fatal(err)
		}

		stdin := strings.NewReader(`{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}`)
		var stdout, stderr bytes.Buffer
		err := runGateCheck(stdin, &stdout, &stderr, "PreToolUse")
		if err != nil {
			t.Errorf("runGateCheck() unexpected error: %v", err)
		}
	})

	t.Run("git push blocked without pf-5 and ws-rev", func(t *testing.T) {
		stdin := strings.NewReader(`{"tool_name":"Bash","tool_input":{"command":"git push origin main"}}`)
		var stdout, stderr bytes.Buffer
		err := runGateCheck(stdin, &stdout, &stderr, "PreToolUse")
		if err == nil {
			t.Fatal("expected error for git push without pf-5")
		}
		code := exitCode(err)
		if code != ExitHookBlock {
			t.Errorf("exit code = %d, want %d", code, ExitHookBlock)
		}
	})

	t.Run("git push allowed with pf-5 and ws-rev", func(t *testing.T) {
		// Create both sentinels.
		if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-5"), []byte("1"), 0o644); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-ws-rev"), []byte("1"), 0o644); err != nil {
			t.Fatal(err)
		}

		stdin := strings.NewReader(`{"tool_name":"Bash","tool_input":{"command":"git push origin main"}}`)
		var stdout, stderr bytes.Buffer
		err := runGateCheck(stdin, &stdout, &stderr, "PreToolUse")
		if err != nil {
			t.Errorf("runGateCheck() unexpected error: %v", err)
		}
	})

	t.Run("ungated command allowed", func(t *testing.T) {
		stdin := strings.NewReader(`{"tool_name":"Bash","tool_input":{"command":"ls -la"}}`)
		var stdout, stderr bytes.Buffer
		err := runGateCheck(stdin, &stdout, &stderr, "PreToolUse")
		if err != nil {
			t.Errorf("runGateCheck() unexpected error: %v", err)
		}
	})
}

func TestRunGateCheckUnknownSession(t *testing.T) {
	// NOTE: no t.Parallel -- modifies environment variables via t.Setenv and creates
	// real filesystem state in the project's .state/ directory.

	projectDir := detectProjectDir()
	sessionID := "unknown"

	// Create pathflow-active flag for "unknown" session.
	flagDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	flagFile := filepath.Join(flagDir, "is-pathflow-active")
	if err := os.WriteFile(flagFile, []byte("1"), 0o644); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(filepath.Join(projectDir, ".state", "session", sessionID)) })

	t.Setenv("CODEFLOW_SESSION_ID", sessionID)

	t.Run("critical gate blocks with unknown session", func(t *testing.T) {
		stdin := strings.NewReader(`{"tool_name":"Bash","tool_input":{"command":"git push origin main"}}`)
		var stdout, stderr bytes.Buffer
		err := runGateCheck(stdin, &stdout, &stderr, "PreToolUse")
		if err == nil {
			t.Fatal("expected error for git push with unknown session")
		}
		code := exitCode(err)
		if code != ExitHookBlock {
			t.Errorf("exit code = %d, want %d", code, ExitHookBlock)
		}
		if !strings.Contains(stderr.String(), "session state unknown") {
			t.Errorf("stderr = %q, want substring 'session state unknown'", stderr.String())
		}
	})

	t.Run("non-critical gate with unknown session falls to sentinel check", func(t *testing.T) {
		// Edit with sessionID="unknown": after detectPathFlowActive returns true,
		// we classify gate type (edit_write), then check sessionID == "unknown".
		// Only GateGitPushPR and GateRoleTeammateSpawn trigger the early block.
		// Edit falls through to sentinel check, which fails (no sentinel dir).
		stdin := strings.NewReader(`{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}`)
		var stdout, stderr bytes.Buffer
		err := runGateCheck(stdin, &stdout, &stderr, "PreToolUse")
		// Edit with unknown session falls through to sentinel check, no pf-3 sentinel → block.
		if err == nil {
			t.Fatal("expected error for Edit with unknown session (no sentinels)")
		}
		code := exitCode(err)
		if code != ExitHookBlock {
			t.Errorf("exit code = %d, want %d", code, ExitHookBlock)
		}
	})
}

func TestRunGateCheckEventDefault(t *testing.T) {
	t.Parallel()

	// Verify empty event string (default) behaves like PreToolUse.
	stdin := strings.NewReader(`{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`)
	var stdout, stderr bytes.Buffer
	err := runGateCheck(stdin, &stdout, &stderr, "")
	if err != nil {
		t.Errorf("runGateCheck() with empty event: unexpected error: %v", err)
	}
}

func TestRunSecurityBlockedExitCode(t *testing.T) {
	t.Parallel()

	// Verify that a blocked command produces the correct exit code.
	stdin := strings.NewReader(`{"tool_name":"Bash","tool_input":{"command":"rm -rf /"}}`)
	var stdout, stderr bytes.Buffer
	err := runSecurity(stdin, &stdout, &stderr)

	if err == nil {
		t.Fatal("expected error for dangerous command")
	}
	code := exitCode(err)
	if code != ExitHookBlock {
		t.Errorf("exit code = %d, want %d", code, ExitHookBlock)
	}
}
