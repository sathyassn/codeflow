package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/hooks/sentinel"
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
	if !names["protection-guard"] {
		t.Error("newPreToolUseCmd() missing subcommand 'protection-guard'")
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

// --- WebFetch Guard tests ---

func TestNewWebFetchGuardCmd(t *testing.T) {
	t.Parallel()

	cmd := newWebFetchGuardCmd()
	if cmd.Use != "webfetch-guard" {
		t.Errorf("newWebFetchGuardCmd().Use = %q, want %q", cmd.Use, "webfetch-guard")
	}
}

func TestPreToolUseCmdHasWebFetchGuard(t *testing.T) {
	t.Parallel()

	cmd := newPreToolUseCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["webfetch-guard"] {
		t.Error("newPreToolUseCmd() missing subcommand 'webfetch-guard'")
	}
}

func TestRunWebFetchGuard(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		stdin      string
		wantErr    bool
		wantBlock  bool
		wantStderr string
	}{
		{
			name:  "non-network tool allowed",
			stdin: `{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`,
		},
		{
			name:  "empty stdin allowed",
			stdin: "",
		},
		{
			name:  "invalid JSON allowed (graceful degradation)",
			stdin: "not json",
		},
		{
			name:  "WebFetch empty URL allowed",
			stdin: `{"tool_name":"WebFetch","tool_input":{"url":""}}`,
		},
		{
			name:      "WebFetch file URL blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"file:///etc/passwd"}}`,
			wantErr:   true,
			wantBlock: true,
			wantStderr: "BLOCKED",
		},
		{
			name:      "WebFetch data URL blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"data:text/html,<h1>test</h1>"}}`,
			wantErr:   true,
			wantBlock: true,
			wantStderr: "BLOCKED",
		},
		{
			name:  "Bash non-network command allowed",
			stdin: `{"tool_name":"Bash","tool_input":{"command":"ls -la"}}`,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			stdin := strings.NewReader(tt.stdin)
			var stdout, stderr bytes.Buffer
			err := runWebFetchGuard(stdin, &stdout, &stderr)

			if tt.wantErr && err == nil {
				t.Error("runWebFetchGuard() error = nil, want error")
			}
			if !tt.wantErr && err != nil {
				t.Errorf("runWebFetchGuard() unexpected error: %v", err)
			}
			if tt.wantBlock {
				var ee *exitError
				if err == nil {
					t.Fatal("expected exitError for blocked URL")
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

func TestWebFetchGuardCmdViaRoot(t *testing.T) {
	t.Parallel()

	// Execute the full command path: hooks pre-tool-use webfetch-guard
	root := newHooksCmd()
	root.SetArgs([]string{"pre-tool-use", "webfetch-guard"})
	root.SetIn(strings.NewReader(`{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`))
	var stdout, stderr bytes.Buffer
	root.SetOut(&stdout)
	root.SetErr(&stderr)
	err := root.Execute()
	if err != nil {
		t.Errorf("webfetch-guard command returned error: %v", err)
	}
}

func TestLoadWebFetchChecker(t *testing.T) {
	t.Parallel()

	t.Run("loads from real project dir", func(t *testing.T) {
		t.Parallel()
		projectDir := detectProjectDir()
		checker := loadWebFetchChecker(projectDir)
		// The real enforcement-policy.json has blocked patterns.
		if len(checker.BlockedPatterns) == 0 {
			t.Error("loadWebFetchChecker() returned no blocked patterns from real config")
		}
		if len(checker.TrustedDomains) == 0 {
			t.Error("loadWebFetchChecker() returned no trusted domains from real config")
		}
	})

	t.Run("returns empty checker for missing dir", func(t *testing.T) {
		t.Parallel()
		checker := loadWebFetchChecker("/nonexistent/path")
		if len(checker.BlockedPatterns) != 0 {
			t.Error("loadWebFetchChecker() returned blocked patterns for missing dir")
		}
		if len(checker.TrustedDomains) != 0 {
			t.Error("loadWebFetchChecker() returned trusted domains for missing dir")
		}
	})

	t.Run("returns empty checker for invalid JSON", func(t *testing.T) {
		t.Parallel()
		tmpDir := t.TempDir()
		configDir := filepath.Join(tmpDir, ".codeflow", "config", "enforcement")
		if err := os.MkdirAll(configDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte("not json"), 0o644); err != nil {
			t.Fatal(err)
		}
		checker := loadWebFetchChecker(tmpDir)
		if len(checker.BlockedPatterns) != 0 {
			t.Error("loadWebFetchChecker() returned blocked patterns for invalid JSON")
		}
	})

	t.Run("skips blocked patterns when disabled", func(t *testing.T) {
		t.Parallel()
		tmpDir := t.TempDir()
		configDir := filepath.Join(tmpDir, ".codeflow", "config", "enforcement")
		if err := os.MkdirAll(configDir, 0o755); err != nil {
			t.Fatal(err)
		}
		policy := `{"network":{"always_block_domains":{"enabled":false,"patterns":["localhost"],"private_ip_ranges":["10.*"]}}}`
		if err := os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(policy), 0o644); err != nil {
			t.Fatal(err)
		}
		checker := loadWebFetchChecker(tmpDir)
		if len(checker.BlockedPatterns) != 0 {
			t.Error("loadWebFetchChecker() loaded patterns when enabled=false")
		}
	})
}

func TestLoadTrustedDomains(t *testing.T) {
	t.Parallel()

	t.Run("parses list file", func(t *testing.T) {
		t.Parallel()
		tmpDir := t.TempDir()
		content := "# comment\ngithub.com\n\napi.example.com\n# another comment\n"
		path := filepath.Join(tmpDir, "test.list")
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
		domains := loadTrustedDomains(path)
		if len(domains) != 2 {
			t.Fatalf("loadTrustedDomains() returned %d domains, want 2", len(domains))
		}
		if domains[0] != "github.com" {
			t.Errorf("domains[0] = %q, want %q", domains[0], "github.com")
		}
		if domains[1] != "api.example.com" {
			t.Errorf("domains[1] = %q, want %q", domains[1], "api.example.com")
		}
	})

	t.Run("returns nil for missing file", func(t *testing.T) {
		t.Parallel()
		domains := loadTrustedDomains("/nonexistent/path/to/file.list")
		if domains != nil {
			t.Errorf("loadTrustedDomains() = %v, want nil for missing file", domains)
		}
	})

	t.Run("returns nil for empty file", func(t *testing.T) {
		t.Parallel()
		tmpDir := t.TempDir()
		path := filepath.Join(tmpDir, "empty.list")
		if err := os.WriteFile(path, []byte(""), 0o644); err != nil {
			t.Fatal(err)
		}
		domains := loadTrustedDomains(path)
		if domains != nil {
			t.Errorf("loadTrustedDomains() = %v, want nil for empty file", domains)
		}
	})

	t.Run("skips comment-only file", func(t *testing.T) {
		t.Parallel()
		tmpDir := t.TempDir()
		content := "# only comments\n# nothing else\n"
		path := filepath.Join(tmpDir, "comments.list")
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
		domains := loadTrustedDomains(path)
		if domains != nil {
			t.Errorf("loadTrustedDomains() = %v, want nil for comment-only file", domains)
		}
	})
}

// --- Team Guard tests ---

func TestNewTeamGuardCmd(t *testing.T) {
	t.Parallel()

	cmd := newTeamGuardCmd()
	if cmd.Use != "team-guard" {
		t.Errorf("newTeamGuardCmd().Use = %q, want %q", cmd.Use, "team-guard")
	}
}

func TestPreToolUseCmdHasTeamGuard(t *testing.T) {
	t.Parallel()

	cmd := newPreToolUseCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["team-guard"] {
		t.Error("newPreToolUseCmd() missing subcommand 'team-guard'")
	}
}

func TestRunTeamGuard(t *testing.T) {
	t.Parallel()

	t.Run("allows when no session ID", func(t *testing.T) {
		t.Parallel()

		// CODEFLOW_SESSION_ID not set → allow through.
		stdin := strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`)
		var stdout, stderr bytes.Buffer
		err := runTeamGuard(stdin, &stdout, &stderr)
		if err != nil {
			t.Errorf("runTeamGuard() unexpected error: %v", err)
		}
	})

	t.Run("allows non-TeamDelete tool", func(t *testing.T) {
		t.Parallel()

		stdin := strings.NewReader(`{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`)
		var stdout, stderr bytes.Buffer
		err := runTeamGuard(stdin, &stdout, &stderr)
		if err != nil {
			t.Errorf("runTeamGuard() unexpected error: %v", err)
		}
	})

	t.Run("allows empty stdin", func(t *testing.T) {
		t.Parallel()

		stdin := strings.NewReader("")
		var stdout, stderr bytes.Buffer
		err := runTeamGuard(stdin, &stdout, &stderr)
		if err != nil {
			t.Errorf("runTeamGuard() unexpected error: %v", err)
		}
	})

	t.Run("allows invalid JSON (graceful degradation)", func(t *testing.T) {
		t.Parallel()

		stdin := strings.NewReader("not json")
		var stdout, stderr bytes.Buffer
		err := runTeamGuard(stdin, &stdout, &stderr)
		if err != nil {
			t.Errorf("runTeamGuard() unexpected error: %v", err)
		}
	})
}

func TestRunTeamGuardWithPathFlow(t *testing.T) {
	// NOTE: no t.Parallel -- modifies environment variables via t.Setenv.

	projectDir := detectProjectDir()
	sessionID := "ses-test-team-guard-" + t.Name()

	// Create pathflow-active flag.
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

	t.Setenv("CODEFLOW_SESSION_ID", sessionID)

	t.Run("TeamDelete blocked without pf-6", func(t *testing.T) {
		stdin := strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`)
		var stdout, stderr bytes.Buffer
		err := runTeamGuard(stdin, &stdout, &stderr)
		if err == nil {
			t.Fatal("expected error for TeamDelete without pf-6")
		}
		code := exitCode(err)
		if code != ExitHookBlock {
			t.Errorf("exit code = %d, want %d", code, ExitHookBlock)
		}
		if !strings.Contains(stderr.String(), "BLOCKED") {
			t.Errorf("stderr = %q, want substring 'BLOCKED'", stderr.String())
		}
	})

	t.Run("TeamDelete allowed with pf-6", func(t *testing.T) {
		// Create pf-6 sentinel.
		if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-6"), []byte("1"), 0o644); err != nil {
			t.Fatal(err)
		}

		stdin := strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`)
		var stdout, stderr bytes.Buffer
		err := runTeamGuard(stdin, &stdout, &stderr)
		if err != nil {
			t.Errorf("runTeamGuard() unexpected error: %v", err)
		}
	})

	t.Run("non-TeamDelete allowed even with PathFlow", func(t *testing.T) {
		stdin := strings.NewReader(`{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`)
		var stdout, stderr bytes.Buffer
		err := runTeamGuard(stdin, &stdout, &stderr)
		if err != nil {
			t.Errorf("runTeamGuard() unexpected error: %v", err)
		}
	})
}

func TestTeamGuardCmdViaRoot(t *testing.T) {
	t.Parallel()

	// Execute the full command path: hooks pre-tool-use team-guard
	// No CODEFLOW_SESSION_ID → should allow.
	root := newHooksCmd()
	root.SetArgs([]string{"pre-tool-use", "team-guard"})
	root.SetIn(strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`))
	var stdout, stderr bytes.Buffer
	root.SetOut(&stdout)
	root.SetErr(&stderr)
	err := root.Execute()
	if err != nil {
		t.Errorf("team-guard command returned error: %v", err)
	}
}

// --- GH PR Guard tests ---

func TestNewGHPRGuardCmd(t *testing.T) {
	t.Parallel()

	cmd := newGHPRGuardCmd()
	if cmd.Use != "gh-pr-guard" {
		t.Errorf("newGHPRGuardCmd().Use = %q, want %q", cmd.Use, "gh-pr-guard")
	}
}

func TestPreToolUseCmdHasGHPRGuard(t *testing.T) {
	t.Parallel()

	cmd := newPreToolUseCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["gh-pr-guard"] {
		t.Error("newPreToolUseCmd() missing subcommand 'gh-pr-guard'")
	}
}

func TestRunGHPRGuard(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		stdin     string
		wantErr   bool
		wantBlock bool
	}{
		{
			name:  "non-Bash tool allowed",
			stdin: `{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`,
		},
		{
			name:  "empty stdin allowed",
			stdin: "",
		},
		{
			name:  "invalid JSON allowed",
			stdin: "not json",
		},
		{
			name:  "non-gh command allowed",
			stdin: `{"tool_name":"Bash","tool_input":{"command":"git status"}}`,
		},
		{
			name:  "gh pr list allowed (not merge)",
			stdin: `{"tool_name":"Bash","tool_input":{"command":"gh pr list"}}`,
		},
		{
			name:  "gh pr merge without PR number allowed (no resolution possible)",
			stdin: `{"tool_name":"Bash","tool_input":{"command":"gh pr merge"}}`,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			stdin := strings.NewReader(tt.stdin)
			var stdout, stderr bytes.Buffer
			err := runGHPRGuard(stdin, &stdout, &stderr)

			if tt.wantErr && err == nil {
				t.Error("runGHPRGuard() error = nil, want error")
			}
			if !tt.wantErr && err != nil {
				t.Errorf("runGHPRGuard() unexpected error: %v", err)
			}
			if tt.wantBlock {
				var ee *exitError
				if err == nil {
					t.Fatal("expected exitError for blocked merge")
				}
				if ok := isExitError(err, &ee); !ok {
					t.Errorf("error type = %T, want *exitError", err)
				} else if ee.code != ExitHookBlock {
					t.Errorf("exit code = %d, want %d", ee.code, ExitHookBlock)
				}
			}
		})
	}
}

func TestGHPRGuardCmdViaRoot(t *testing.T) {
	t.Parallel()

	// Execute the full command path: hooks pre-tool-use gh-pr-guard
	root := newHooksCmd()
	root.SetArgs([]string{"pre-tool-use", "gh-pr-guard"})
	root.SetIn(strings.NewReader(`{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`))
	var stdout, stderr bytes.Buffer
	root.SetOut(&stdout)
	root.SetErr(&stderr)
	err := root.Execute()
	if err != nil {
		t.Errorf("gh-pr-guard command returned error: %v", err)
	}
}

// --- All subcommands registered ---

func TestPreToolUseCmdHasAllSubcommands(t *testing.T) {
	t.Parallel()

	cmd := newPreToolUseCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	expected := []string{"security", "gate-check", "gh-pr-guard", "webfetch-guard", "team-guard"}
	for _, name := range expected {
		if !names[name] {
			t.Errorf("newPreToolUseCmd() missing subcommand %q", name)
		}
	}
}

// --- Post-tool-use command group ---

func TestNewPostToolUseCmd(t *testing.T) {
	t.Parallel()

	cmd := newPostToolUseCmd()
	if cmd.Use != "post-tool-use" {
		t.Errorf("newPostToolUseCmd().Use = %q, want %q", cmd.Use, "post-tool-use")
	}
	if !cmd.HasSubCommands() {
		t.Error("newPostToolUseCmd() has no subcommands")
	}

	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	expected := []string{"sentinel-write", "checkpoint-register", "settings-validate"}
	for _, name := range expected {
		if !names[name] {
			t.Errorf("newPostToolUseCmd() missing subcommand %q", name)
		}
	}
}

func TestPostToolUseCmdHelp(t *testing.T) {
	t.Parallel()

	cmd := newPostToolUseCmd()
	cmd.SetArgs([]string{})
	var out bytes.Buffer
	cmd.SetOut(&out)
	err := cmd.Execute()
	if err != nil {
		t.Errorf("post-tool-use help returned error: %v", err)
	}
	if !strings.Contains(out.String(), "sentinel and checkpoint management") {
		t.Errorf("post-tool-use help output = %q, want to contain 'sentinel and checkpoint management'", out.String())
	}
}

// --- Task-completed command group ---

func TestNewTaskCompletedCmd(t *testing.T) {
	t.Parallel()

	cmd := newTaskCompletedCmd()
	if cmd.Use != "task-completed" {
		t.Errorf("newTaskCompletedCmd().Use = %q, want %q", cmd.Use, "task-completed")
	}
	if !cmd.HasSubCommands() {
		t.Error("newTaskCompletedCmd() has no subcommands")
	}

	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["checkpoint-complete"] {
		t.Error("newTaskCompletedCmd() missing subcommand 'checkpoint-complete'")
	}
}

func TestTaskCompletedCmdHelp(t *testing.T) {
	t.Parallel()

	cmd := newTaskCompletedCmd()
	cmd.SetArgs([]string{})
	var out bytes.Buffer
	cmd.SetOut(&out)
	err := cmd.Execute()
	if err != nil {
		t.Errorf("task-completed help returned error: %v", err)
	}
	if !strings.Contains(out.String(), "task is marked complete") {
		t.Errorf("task-completed help output = %q, want to contain 'task is marked complete'", out.String())
	}
}

// --- Hooks cmd has post-tool-use and task-completed ---

func TestHooksCmdHasAllGroups(t *testing.T) {
	t.Parallel()

	cmd := newHooksCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	expected := []string{"pre-tool-use", "post-tool-use", "task-completed", "session-start"}
	for _, name := range expected {
		if !names[name] {
			t.Errorf("newHooksCmd() missing subcommand group %q", name)
		}
	}
}

// --- Sentinel write CLI handler ---

func TestRunSentinelWrite(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		stdin     string
		wantErr   bool
		wantBlock bool
	}{
		{
			name:  "non-SendMessage tool allowed",
			stdin: `{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`,
		},
		{
			name:  "empty stdin allowed",
			stdin: "",
		},
		{
			name:  "invalid JSON allowed",
			stdin: "not json",
		},
		{
			name:  "SendMessage without stage pattern allowed",
			stdin: `{"tool_name":"SendMessage","tool_input":{"content":"Hello world"}}`,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			stdin := strings.NewReader(tt.stdin)
			var stdout, stderr bytes.Buffer
			err := runSentinelWrite(stdin, &stdout, &stderr)

			if tt.wantErr && err == nil {
				t.Error("runSentinelWrite() error = nil, want error")
			}
			if !tt.wantErr && err != nil {
				t.Errorf("runSentinelWrite() unexpected error: %v", err)
			}
		})
	}
}

func TestRunSentinelWriteCreatesFile(t *testing.T) {
	t.Parallel()

	// Call the business logic directly (not through runSentinelWrite which
	// uses env vars) to verify integration between CLI and package.
	sentinelDir := t.TempDir()
	stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}`)

	verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentinelDir)
	if !verdict.Allow {
		t.Errorf("expected allow, got block: %s", verdict.Reason)
	}

	// Verify sentinel file was created.
	path := filepath.Join(sentinelDir, "pathflow-ws-dev")
	if _, err := os.Stat(path); os.IsNotExist(err) {
		t.Error("sentinel file pathflow-ws-dev not created")
	}
}

func TestRunSentinelWriteWithSession(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir which modify process-wide state.

	// Set up temp project dir with sentinel directory.
	projectDir := t.TempDir()
	sessionID := "ses-test-sentinel"
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Override detectProjectDir by changing cwd.
	origDir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	// Create a git repo in the temp dir so detectProjectDir returns it.
	if err := os.MkdirAll(filepath.Join(projectDir, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.Chdir(projectDir); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(origDir) })

	t.Setenv("CODEFLOW_SESSION_ID", sessionID)

	t.Run("creates sentinel on STAGE-COMPLETE", func(t *testing.T) {
		stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}`)
		var stdout, stderr bytes.Buffer
		err := runSentinelWrite(stdin, &stdout, &stderr)
		if err != nil {
			t.Errorf("runSentinelWrite() unexpected error: %v", err)
		}

		path := filepath.Join(sentinelDir, "pathflow-ws-dev")
		if _, err := os.Stat(path); os.IsNotExist(err) {
			t.Error("sentinel file pathflow-ws-dev not created")
		}
	})

	t.Run("blocks ws-rev without primary stage", func(t *testing.T) {
		// Remove ws-dev sentinel to ensure rev is blocked.
		_ = os.Remove(filepath.Join(sentinelDir, "pathflow-ws-dev"))

		stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV"}}`)
		var stdout, stderr bytes.Buffer
		err := runSentinelWrite(stdin, &stdout, &stderr)
		if err == nil {
			t.Fatal("runSentinelWrite() expected error for blocked ws-rev")
		}
		var ee *exitError
		if ok := isExitError(err, &ee); !ok {
			t.Errorf("error type = %T, want *exitError", err)
		} else if ee.code != ExitHookBlock {
			t.Errorf("exit code = %d, want %d", ee.code, ExitHookBlock)
		}
		if !strings.Contains(stderr.String(), "BLOCKED") {
			t.Errorf("stderr = %q, want substring 'BLOCKED'", stderr.String())
		}
	})
}

func TestSentinelWriteCmdViaRoot(t *testing.T) {
	t.Parallel()

	root := newHooksCmd()
	root.SetArgs([]string{"post-tool-use", "sentinel-write"})
	root.SetIn(strings.NewReader(`{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`))
	var stdout, stderr bytes.Buffer
	root.SetOut(&stdout)
	root.SetErr(&stderr)
	err := root.Execute()
	if err != nil {
		t.Errorf("sentinel-write command returned error: %v", err)
	}
}

// --- Checkpoint register CLI handler ---

func TestRunHookCheckpointRegister(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name  string
		stdin string
	}{
		{
			name:  "non-TaskCreate tool allowed",
			stdin: `{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`,
		},
		{
			name:  "empty stdin allowed",
			stdin: "",
		},
		{
			name:  "invalid JSON allowed",
			stdin: "not json",
		},
		{
			name:  "TaskCreate without PF pattern allowed",
			stdin: `{"tool_name":"TaskCreate","tool_input":{"subject":"Regular task"}}`,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			stdin := strings.NewReader(tt.stdin)
			var stdout, stderr bytes.Buffer
			err := runHookCheckpointRegister(stdin, &stdout, &stderr)

			if err != nil {
				t.Errorf("runHookCheckpointRegister() unexpected error: %v", err)
			}
		})
	}
}

func TestRunHookCheckpointRegisterWithSession(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir which modify process-wide state.

	projectDir := t.TempDir()
	sessionID := "ses-test-checkpoint-reg"
	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create git repo so detectProjectDir works.
	if err := os.MkdirAll(filepath.Join(projectDir, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	origDir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Chdir(projectDir); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(origDir) })

	t.Setenv("CODEFLOW_SESSION_ID", sessionID)

	// Create a checkpoint file with PF1 phase.
	checkpointFile := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
	checkpointJSON := `{"PF1":{"expected":["PF1-TSK-01"],"conditions":{},"registered":{},"completed":{},"skipped":{},"sentinel_created":false},"PF2":{"expected":["PF2-TSK-01"],"conditions":{},"registered":{},"completed":{},"skipped":{},"sentinel_created":false}}`
	if err := os.WriteFile(checkpointFile, []byte(checkpointJSON), 0o644); err != nil {
		t.Fatal(err)
	}

	t.Run("registers PF1 task", func(t *testing.T) {
		stdin := strings.NewReader(`{"tool_name":"TaskCreate","tool_input":{"subject":"PF1-TSK-01 Init"}}`)
		var stdout, stderr bytes.Buffer
		err := runHookCheckpointRegister(stdin, &stdout, &stderr)
		if err != nil {
			t.Errorf("runHookCheckpointRegister() unexpected error: %v", err)
		}
	})

	t.Run("blocks PF2 task without pf-1 sentinel", func(t *testing.T) {
		stdin := strings.NewReader(`{"tool_name":"TaskCreate","tool_input":{"subject":"PF2-TSK-01 Context"}}`)
		var stdout, stderr bytes.Buffer
		err := runHookCheckpointRegister(stdin, &stdout, &stderr)
		if err == nil {
			t.Fatal("runHookCheckpointRegister() expected error for cross-phase block")
		}
		var ee *exitError
		if ok := isExitError(err, &ee); !ok {
			t.Errorf("error type = %T, want *exitError", err)
		} else if ee.code != ExitHookBlock {
			t.Errorf("exit code = %d, want %d", ee.code, ExitHookBlock)
		}
		if !strings.Contains(stderr.String(), "CHECKPOINT BLOCK") {
			t.Errorf("stderr = %q, want substring 'CHECKPOINT BLOCK'", stderr.String())
		}
	})
}

func TestCheckpointRegisterCmdViaRoot(t *testing.T) {
	t.Parallel()

	root := newHooksCmd()
	root.SetArgs([]string{"post-tool-use", "checkpoint-register"})
	root.SetIn(strings.NewReader(`{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`))
	var stdout, stderr bytes.Buffer
	root.SetOut(&stdout)
	root.SetErr(&stderr)
	err := root.Execute()
	if err != nil {
		t.Errorf("checkpoint-register command returned error: %v", err)
	}
}

// --- Checkpoint complete CLI handler ---

func TestRunHookCheckpointComplete(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name  string
		stdin string
	}{
		{
			name:  "empty stdin allowed",
			stdin: "",
		},
		{
			name:  "invalid JSON allowed",
			stdin: "not json",
		},
		{
			name:  "non-PF task subject allowed",
			stdin: `{"task_subject":"Regular task completion"}`,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			stdin := strings.NewReader(tt.stdin)
			var stdout, stderr bytes.Buffer
			err := runHookCheckpointComplete(stdin, &stdout, &stderr)

			if err != nil {
				t.Errorf("runHookCheckpointComplete() unexpected error: %v", err)
			}
		})
	}
}

func TestRunHookCheckpointCompleteWithSession(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir which modify process-wide state.

	projectDir := t.TempDir()
	sessionID := "ses-test-checkpoint-cpl"
	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create git repo so detectProjectDir works.
	if err := os.MkdirAll(filepath.Join(projectDir, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	origDir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Chdir(projectDir); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(origDir) })

	t.Setenv("CODEFLOW_SESSION_ID", sessionID)

	// Create a checkpoint file with PF1 phase (single task, already registered).
	checkpointFile := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
	checkpointJSON := `{"PF1":{"expected":["PF1-TSK-01"],"conditions":{},"registered":{"PF1-TSK-01":"2026-02-28T00:00:00Z"},"completed":{},"skipped":{},"sentinel_created":false},"PF2":{"expected":["PF2-TSK-01"],"conditions":{},"registered":{},"completed":{},"skipped":{},"sentinel_created":false}}`
	if err := os.WriteFile(checkpointFile, []byte(checkpointJSON), 0o644); err != nil {
		t.Fatal(err)
	}

	t.Run("completes PF1 task and creates sentinel", func(t *testing.T) {
		stdin := strings.NewReader(`{"task_subject":"PF1-TSK-01 Init"}`)
		var stdout, stderr bytes.Buffer
		err := runHookCheckpointComplete(stdin, &stdout, &stderr)
		if err != nil {
			t.Errorf("runHookCheckpointComplete() unexpected error: %v", err)
		}

		// Verify phase sentinel was created (PF1 has 1 task, now complete).
		path := filepath.Join(sentinelDir, "pathflow-pf-1")
		if _, err := os.Stat(path); os.IsNotExist(err) {
			t.Error("phase sentinel pathflow-pf-1 not created")
		}
	})

	t.Run("blocks PF2 task without pf-1 sentinel", func(t *testing.T) {
		// Remove pf-1 sentinel to test cross-phase block.
		_ = os.Remove(filepath.Join(sentinelDir, "pathflow-pf-1"))

		stdin := strings.NewReader(`{"task_subject":"PF2-TSK-01 Context"}`)
		var stdout, stderr bytes.Buffer
		err := runHookCheckpointComplete(stdin, &stdout, &stderr)
		if err == nil {
			t.Fatal("runHookCheckpointComplete() expected error for cross-phase block")
		}
		var ee *exitError
		if ok := isExitError(err, &ee); !ok {
			t.Errorf("error type = %T, want *exitError", err)
		} else if ee.code != ExitHookBlock {
			t.Errorf("exit code = %d, want %d", ee.code, ExitHookBlock)
		}
		if !strings.Contains(stderr.String(), "CHECKPOINT BLOCK") {
			t.Errorf("stderr = %q, want substring 'CHECKPOINT BLOCK'", stderr.String())
		}
	})
}

func TestCheckpointCompleteCmdViaRoot(t *testing.T) {
	t.Parallel()

	root := newHooksCmd()
	root.SetArgs([]string{"task-completed", "checkpoint-complete"})
	root.SetIn(strings.NewReader(`{"task_subject":"Regular task"}`))
	var stdout, stderr bytes.Buffer
	root.SetOut(&stdout)
	root.SetErr(&stderr)
	err := root.Execute()
	if err != nil {
		t.Errorf("checkpoint-complete command returned error: %v", err)
	}
}

// --- Session-start command group ---

func TestNewHookSessionStartCmd(t *testing.T) {
	t.Parallel()

	cmd := newHookSessionStartCmd()
	if cmd.Use != "session-start" {
		t.Errorf("newHookSessionStartCmd().Use = %q, want %q", cmd.Use, "session-start")
	}
	if !cmd.HasSubCommands() {
		t.Error("newHookSessionStartCmd() has no subcommands, want init")
	}

	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["init"] {
		t.Error("newHookSessionStartCmd() missing subcommand 'init'")
	}
}

func TestNewHookSessionStartInitCmd(t *testing.T) {
	t.Parallel()

	cmd := newHookSessionStartInitCmd()
	if cmd.Use != "init" {
		t.Errorf("newHookSessionStartInitCmd().Use = %q, want %q", cmd.Use, "init")
	}
}

func TestRunSessionStartInit(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir to control detectProjectDir.

	projectDir := t.TempDir()

	// Create .git dir so detectProjectDir returns our temp dir.
	if err := os.MkdirAll(filepath.Join(projectDir, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create pathflow config for checkpoint init (minimal with 7 phases).
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}
	pfConfig := `{"phases":{"PF1":{"tasks":["PF1-TSK-01"]},"PF2":{"tasks":["PF2-TSK-01"]},"PF3":{"tasks":["PF3-TSK-01"]},"PF4":{"tasks":["PF4-TSK-01"]},"PF5":{"tasks":["PF5-TSK-01"]},"PF6":{"tasks":["PF6-TSK-01"]},"PF7":{"tasks":["PF7-TSK-01"]}}}`
	if err := os.WriteFile(filepath.Join(configDir, "pathflow-config.json"), []byte(pfConfig), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create runtime dir.
	if err := os.MkdirAll(filepath.Join(projectDir, ".state", "runtime"), 0o755); err != nil {
		t.Fatal(err)
	}

	origDir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Chdir(projectDir); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(origDir) })

	t.Run("fresh session init", func(t *testing.T) {
		stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
		var stdout, stderr bytes.Buffer
		err := runSessionStartInit(stdin, &stdout, &stderr)
		if err != nil {
			t.Fatalf("runSessionStartInit() unexpected error: %v", err)
		}

		// Stdout should contain env JSON.
		output := stdout.String()
		if !strings.Contains(output, "CODEFLOW_SESSION_ID") {
			t.Errorf("stdout = %q, want to contain 'CODEFLOW_SESSION_ID'", output)
		}
		if !strings.Contains(output, "CF_PROJECT_ROOT") {
			t.Errorf("stdout = %q, want to contain 'CF_PROJECT_ROOT'", output)
		}
		if !strings.Contains(output, "ses-") {
			t.Errorf("stdout = %q, want to contain session ID starting with 'ses-'", output)
		}
	})

	t.Run("handles empty stdin gracefully", func(t *testing.T) {
		stdin := strings.NewReader("")
		var stdout, stderr bytes.Buffer
		err := runSessionStartInit(stdin, &stdout, &stderr)
		if err != nil {
			t.Fatalf("runSessionStartInit() with empty stdin unexpected error: %v", err)
		}

		output := stdout.String()
		if !strings.Contains(output, "CODEFLOW_SESSION_ID") {
			t.Errorf("stdout = %q, want to contain 'CODEFLOW_SESSION_ID'", output)
		}
	})

	t.Run("handles invalid JSON gracefully", func(t *testing.T) {
		stdin := strings.NewReader("not json")
		var stdout, stderr bytes.Buffer
		err := runSessionStartInit(stdin, &stdout, &stderr)
		if err != nil {
			t.Fatalf("runSessionStartInit() with invalid JSON unexpected error: %v", err)
		}

		output := stdout.String()
		if !strings.Contains(output, "CODEFLOW_SESSION_ID") {
			t.Errorf("stdout = %q, want to contain 'CODEFLOW_SESSION_ID'", output)
		}
	})
}

func TestSessionStartInitCmdViaRoot(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir.

	projectDir := t.TempDir()

	// Create .git dir.
	if err := os.MkdirAll(filepath.Join(projectDir, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create pathflow config.
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}
	pfConfig := `{"phases":{"PF1":{"tasks":["PF1-TSK-01"]},"PF2":{"tasks":["PF2-TSK-01"]},"PF3":{"tasks":["PF3-TSK-01"]},"PF4":{"tasks":["PF4-TSK-01"]},"PF5":{"tasks":["PF5-TSK-01"]},"PF6":{"tasks":["PF6-TSK-01"]},"PF7":{"tasks":["PF7-TSK-01"]}}}`
	if err := os.WriteFile(filepath.Join(configDir, "pathflow-config.json"), []byte(pfConfig), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create runtime dir.
	if err := os.MkdirAll(filepath.Join(projectDir, ".state", "runtime"), 0o755); err != nil {
		t.Fatal(err)
	}

	origDir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Chdir(projectDir); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(origDir) })

	// Execute the full command path: hooks session-start init
	root := newHooksCmd()
	root.SetArgs([]string{"session-start", "init"})
	root.SetIn(strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`))
	var stdout, stderr bytes.Buffer
	root.SetOut(&stdout)
	root.SetErr(&stderr)
	err = root.Execute()
	if err != nil {
		t.Errorf("session-start init command returned error: %v", err)
	}

	if !strings.Contains(stdout.String(), "CODEFLOW_SESSION_ID") {
		t.Errorf("stdout = %q, want to contain 'CODEFLOW_SESSION_ID'", stdout.String())
	}
}

func TestSessionStartCmdHelp(t *testing.T) {
	t.Parallel()

	cmd := newHookSessionStartCmd()
	cmd.SetArgs([]string{})
	var out bytes.Buffer
	cmd.SetOut(&out)
	err := cmd.Execute()
	if err != nil {
		t.Errorf("session-start help returned error: %v", err)
	}
	if !strings.Contains(out.String(), "session-start") {
		t.Errorf("session-start help output = %q, want to contain 'session-start'", out.String())
	}
}

func TestNewHookSessionEndCmd(t *testing.T) {
	t.Parallel()

	cmd := newHookSessionEndCmd()
	if cmd.Use != "session-end" {
		t.Errorf("newHookSessionEndCmd().Use = %q, want %q", cmd.Use, "session-end")
	}
	if !cmd.HasSubCommands() {
		t.Error("newHookSessionEndCmd() has no subcommands, want cleanup")
	}

	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["cleanup"] {
		t.Error("newHookSessionEndCmd() missing subcommand 'cleanup'")
	}
}

func TestNewHookSessionEndCleanupCmd(t *testing.T) {
	t.Parallel()

	cmd := newHookSessionEndCleanupCmd()
	if cmd.Use != "cleanup" {
		t.Errorf("newHookSessionEndCleanupCmd().Use = %q, want %q", cmd.Use, "cleanup")
	}
}

func TestRunSessionEndCleanup(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir to control detectProjectDir.

	projectDir := t.TempDir()
	sessionID := "ses-1234567890123abcdef012345"

	// Create .git dir so detectProjectDir returns our temp dir.
	if err := os.MkdirAll(filepath.Join(projectDir, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create runtime dir with env file and current-session-id.
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='" + sessionID + "'\nexport CF_PROJECT_ROOT='testproject'\n"
	if err := os.WriteFile(filepath.Join(runtimeDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(runtimeDir, "current-session-id"), []byte(sessionID), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create ledger directory.
	if err := os.MkdirAll(filepath.Join(projectDir, ".state", "ledger"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create sentinel directories.
	pfSentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.MkdirAll(pfSentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(pfSentinelDir, "pathflow-pf-7"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create session state directory.
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(sessionStateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	origDir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Chdir(projectDir); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(origDir) })

	t.Run("cleanup completes successfully", func(t *testing.T) {
		stdin := strings.NewReader(`{"session_id":"test-uuid","transcript_path":"/tmp/tx"}`)
		var stdout, stderr bytes.Buffer
		err := runSessionEndCleanup(stdin, &stdout, &stderr)
		if err != nil {
			t.Fatalf("runSessionEndCleanup() unexpected error: %v", err)
		}

		// Stderr should contain cleanup messages.
		stderrStr := stderr.String()
		if !strings.Contains(stderrStr, "SessionEnd") {
			t.Errorf("stderr = %q, want to contain 'SessionEnd'", stderrStr)
		}
	})

	t.Run("handles empty stdin gracefully", func(t *testing.T) {
		// Re-create runtime files for this subtest.
		if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(runtimeDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
			t.Fatal(err)
		}
		// Re-create session and ledger dirs.
		if err := os.MkdirAll(sessionStateDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.MkdirAll(filepath.Join(projectDir, ".state", "ledger"), 0o755); err != nil {
			t.Fatal(err)
		}

		stdin := strings.NewReader("")
		var stdout, stderr bytes.Buffer
		err := runSessionEndCleanup(stdin, &stdout, &stderr)
		if err != nil {
			t.Fatalf("runSessionEndCleanup() with empty stdin unexpected error: %v", err)
		}
	})
}

func TestSessionEndCleanupCmdViaRoot(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir.

	projectDir := t.TempDir()
	sessionID := "ses-1234567890123abcdef012345"

	// Create .git dir.
	if err := os.MkdirAll(filepath.Join(projectDir, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create runtime dir with env file.
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='" + sessionID + "'\n"
	if err := os.WriteFile(filepath.Join(runtimeDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create ledger directory.
	if err := os.MkdirAll(filepath.Join(projectDir, ".state", "ledger"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create session state directory.
	if err := os.MkdirAll(filepath.Join(projectDir, ".state", "session", sessionID, "pathflow"), 0o755); err != nil {
		t.Fatal(err)
	}

	origDir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Chdir(projectDir); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(origDir) })

	// Execute the full command path: hooks session-end cleanup
	root := newHooksCmd()
	root.SetArgs([]string{"session-end", "cleanup"})
	root.SetIn(strings.NewReader(`{"session_id":"test-uuid"}`))
	var stdout, stderr bytes.Buffer
	root.SetOut(&stdout)
	root.SetErr(&stderr)
	err = root.Execute()
	if err != nil {
		t.Errorf("session-end cleanup command returned error: %v", err)
	}

	if !strings.Contains(stderr.String(), "SessionEnd") {
		t.Errorf("stderr = %q, want to contain 'SessionEnd'", stderr.String())
	}
}

func TestSessionEndCmdHelp(t *testing.T) {
	t.Parallel()

	cmd := newHookSessionEndCmd()
	cmd.SetArgs([]string{})
	var out bytes.Buffer
	cmd.SetOut(&out)
	err := cmd.Execute()
	if err != nil {
		t.Errorf("session-end help returned error: %v", err)
	}
	if !strings.Contains(out.String(), "session-end") {
		t.Errorf("session-end help output = %q, want to contain 'session-end'", out.String())
	}
}

func TestHooksCmd_HasSessionEnd(t *testing.T) {
	t.Parallel()

	cmd := newHooksCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["session-end"] {
		t.Error("newHooksCmd() missing subcommand 'session-end'")
	}
	if !names["session-start"] {
		t.Error("newHooksCmd() missing subcommand 'session-start'")
	}
}

// --- Protection guard command tests ---

func TestNewProtectionGuardCmd(t *testing.T) {
	t.Parallel()

	cmd := newProtectionGuardCmd()
	if cmd.Use != "protection-guard" {
		t.Errorf("newProtectionGuardCmd().Use = %q, want %q", cmd.Use, "protection-guard")
	}
	if cmd.Short == "" {
		t.Error("newProtectionGuardCmd() has empty Short description")
	}
}

func TestRunProtectionGuard_NonEditTool(t *testing.T) {
	t.Parallel()

	// Non-Edit/Write tool should pass through silently.
	cmd := newProtectionGuardCmd()
	cmd.SetIn(strings.NewReader(`{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`))
	var out, errOut bytes.Buffer
	cmd.SetOut(&out)
	cmd.SetErr(&errOut)
	cmd.SilenceUsage = true

	err := cmd.Execute()
	if err != nil {
		t.Errorf("protection-guard should allow non-Edit tool, got error: %v", err)
	}
}

func TestRunProtectionGuard_UnprotectedFile(t *testing.T) {
	t.Parallel()

	// Edit to a file not in any protection tier should pass.
	// Use an absolute temp path to avoid matching any protection pattern.
	cmd := newProtectionGuardCmd()
	cmd.SetIn(strings.NewReader(`{"tool_name":"Edit","tool_input":{"file_path":"/tmp/some-unprotected-file.go"}}`))
	var out, errOut bytes.Buffer
	cmd.SetOut(&out)
	cmd.SetErr(&errOut)
	cmd.SilenceUsage = true

	err := cmd.Execute()
	if err != nil {
		t.Errorf("protection-guard should allow unprotected file, got error: %v", err)
	}
}

// --- Settings-validate hook command tests ---

func TestNewSettingsValidateHookCmd(t *testing.T) {
	t.Parallel()

	cmd := newSettingsValidateHookCmd()
	if cmd.Use != "settings-validate" {
		t.Errorf("newSettingsValidateHookCmd().Use = %q, want %q", cmd.Use, "settings-validate")
	}
	if cmd.Short == "" {
		t.Error("newSettingsValidateHookCmd() has empty Short description")
	}
}

func TestRunSettingsValidateHook_NonTemplateEdit(t *testing.T) {
	t.Parallel()

	// Non-settings-template edit should be silently ignored (advisory hook).
	cmd := newSettingsValidateHookCmd()
	cmd.SetIn(strings.NewReader(`{"tool_name":"Edit","tool_input":{"file_path":"src/main.go"}}`))
	var out, errOut bytes.Buffer
	cmd.SetOut(&out)
	cmd.SetErr(&errOut)
	cmd.SilenceUsage = true

	err := cmd.Execute()
	if err != nil {
		t.Errorf("settings-validate should be advisory (exit 0), got error: %v", err)
	}
}

