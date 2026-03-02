package edit

import (
	"strings"
	"testing"
)

func newTestChecker() *ScopeChecker {
	return &ScopeChecker{
		ProjectDir:         "/project",
		BlockedDirs:        DefaultBlockedDirs(),
		AllowedTmpPrefixes: DefaultAllowedTmpPrefixes(),
		DangerousExts:      DefaultDangerousExtensions(),
		WarnOnDangerous:    true,
		ProtectedBranches:  DefaultProtectedBranches(),
		CurrentBranch:      "feat/my-feature",
	}
}

func TestCheck(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name        string
		stdin       string
		branch      string
		wantAllow   bool
		wantContain string // substring expected in Message
	}{
		{
			name:      "empty stdin allows",
			stdin:     "",
			wantAllow: true,
		},
		{
			name:      "invalid JSON allows",
			stdin:     "not json",
			wantAllow: true,
		},
		{
			name:      "non Edit/Write tool allows",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"ls"}}`,
			wantAllow: true,
		},
		{
			name:      "Edit with no file_path allows",
			stdin:     `{"tool_name":"Edit","tool_input":{}}`,
			wantAllow: true,
		},
		{
			name:      "Edit normal file allows",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/project/src/main.go"}}`,
			wantAllow: true,
		},
		{
			name:      "Write normal file allows",
			stdin:     `{"tool_name":"Write","tool_input":{"file_path":"/project/src/main.go"}}`,
			wantAllow: true,
		},
		{
			name:        "Edit in blocked dir blocks",
			stdin:       `{"tool_name":"Edit","tool_input":{"file_path":"/project/.git/config"}}`,
			wantAllow:   false,
			wantContain: ".git",
		},
		{
			name:        "Write in node_modules blocks",
			stdin:       `{"tool_name":"Write","tool_input":{"file_path":"/project/node_modules/pkg/index.js"}}`,
			wantAllow:   false,
			wantContain: "node_modules",
		},
		{
			name:      "Edit in allowed tmp allows",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/codeflow/managed/file.txt"}}`,
			wantAllow: true,
		},
		{
			name:        "Edit outside project blocks",
			stdin:       `{"tool_name":"Edit","tool_input":{"file_path":"/other/dir/file.go"}}`,
			wantAllow:   false,
			wantContain: "within the project directory",
		},
		{
			name:        "Edit on protected branch blocks",
			stdin:       `{"tool_name":"Edit","tool_input":{"file_path":"/project/src/main.go"}}`,
			branch:      "main",
			wantAllow:   false,
			wantContain: "protected branch",
		},
		{
			name:        "Edit on master branch blocks",
			stdin:       `{"tool_name":"Edit","tool_input":{"file_path":"/project/src/main.go"}}`,
			branch:      "master",
			wantAllow:   false,
			wantContain: "protected branch",
		},
		{
			name:      "Edit on protected branch but tmp allows",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/file.txt"}}`,
			branch:    "main",
			wantAllow: true,
		},
		{
			name:        "Write binary file warns",
			stdin:       `{"tool_name":"Write","tool_input":{"file_path":"/project/build/app.exe"}}`,
			wantAllow:   true,
			wantContain: "binary file",
		},
		{
			name:        "Write credential file warns",
			stdin:       `{"tool_name":"Write","tool_input":{"file_path":"/project/certs/server.pem"}}`,
			wantAllow:   true,
			wantContain: "credential file",
		},
		{
			name:        "Write archive file warns",
			stdin:       `{"tool_name":"Write","tool_input":{"file_path":"/project/dist/app.zip"}}`,
			wantAllow:   true,
			wantContain: "archive file",
		},
		{
			name:        "Edit in __pycache__ blocks",
			stdin:       `{"tool_name":"Edit","tool_input":{"file_path":"/project/src/__pycache__/module.pyc"}}`,
			wantAllow:   false,
			wantContain: "__pycache__",
		},
		{
			name:      "relative path within project allows",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"src/main.go"}}`,
			wantAllow: true,
		},
		{
			name:        "path traversal via prefix overlap blocks",
			stdin:       `{"tool_name":"Edit","tool_input":{"file_path":"/project-other/file.go"}}`,
			wantAllow:   false,
			wantContain: "within the project directory",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			checker := newTestChecker()
			if tt.branch != "" {
				checker.CurrentBranch = tt.branch
			}

			verdict, err := checker.Check(strings.NewReader(tt.stdin))
			if err != nil {
				t.Fatalf("Check() error = %v", err)
			}

			if verdict.Allow != tt.wantAllow {
				t.Errorf("Check() Allow = %v, want %v; message: %s",
					verdict.Allow, tt.wantAllow, verdict.Message)
			}
			if tt.wantContain != "" && !strings.Contains(verdict.Message, tt.wantContain) {
				t.Errorf("Check() Message should contain %q; got: %s",
					tt.wantContain, verdict.Message)
			}
		})
	}
}

func TestIsAllowedTmp(t *testing.T) {
	t.Parallel()

	checker := newTestChecker()

	tests := []struct {
		name string
		path string
		want bool
	}{
		{"tmp/claude path", "/tmp/claude/project/file.txt", true},
		{"tmp path", "/tmp/other/file.txt", true},
		{"project path", "/project/src/main.go", false},
		{"home path", "/home/user/file.txt", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := checker.isAllowedTmp(tt.path); got != tt.want {
				t.Errorf("isAllowedTmp(%q) = %v, want %v", tt.path, got, tt.want)
			}
		})
	}
}

func TestIsProtectedBranch(t *testing.T) {
	t.Parallel()

	checker := newTestChecker()
	checker.ProtectedBranches = append(checker.ProtectedBranches, "release/*", "production")

	tests := []struct {
		name   string
		branch string
		want   bool
	}{
		{"main is protected", "main", true},
		{"master is protected", "master", true},
		{"release/v1.0 is protected", "release/v1.0", true},
		{"production is protected", "production", true},
		{"feat branch is not", "feat/my-feature", false},
		{"fix branch is not", "fix/bug-123", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := checker.isProtectedBranch(tt.branch); got != tt.want {
				t.Errorf("isProtectedBranch(%q) = %v, want %v", tt.branch, got, tt.want)
			}
		})
	}
}

func TestMatchBlockedDir(t *testing.T) {
	t.Parallel()

	checker := newTestChecker()

	tests := []struct {
		name    string
		path    string
		wantDir string
	}{
		{"starts with .git/", ".git/config", ".git"},
		{"contains /node_modules/", "src/node_modules/pkg/index.js", "node_modules"},
		{"ends with /__pycache__", "src/__pycache__", "__pycache__"},
		{"normal path", "src/main.go", ""},
		{"venv dir", ".venv/lib/python3.11/site.py", ".venv"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := checker.matchBlockedDir(tt.path)
			if got != tt.wantDir {
				t.Errorf("matchBlockedDir(%q) = %q, want %q", tt.path, got, tt.wantDir)
			}
		})
	}
}

func TestCheckDangerousExtension(t *testing.T) {
	t.Parallel()

	checker := newTestChecker()

	tests := []struct {
		name     string
		filePath string
		wantWarn bool
		wantType string
	}{
		{"go file no warn", "main.go", false, ""},
		{"exe is binary", "app.exe", true, "binary"},
		{"dll is binary", "lib.dll", true, "binary"},
		{"pem is credential", "cert.pem", true, "credential"},
		{"key is credential", "server.key", true, "credential"},
		{"zip is archive", "data.zip", true, "archive"},
		{"tar is archive", "backup.tar", true, "archive"},
		{"no extension", "Makefile", false, ""},
		{"uppercase exe", "APP.EXE", true, "binary"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			msg := checker.checkDangerousExtension("Edit", tt.filePath)
			if tt.wantWarn && msg == "" {
				t.Errorf("checkDangerousExtension(%q) = empty, want warning", tt.filePath)
			}
			if !tt.wantWarn && msg != "" {
				t.Errorf("checkDangerousExtension(%q) = %q, want empty", tt.filePath, msg)
			}
			if tt.wantType != "" && !strings.Contains(msg, tt.wantType) {
				t.Errorf("checkDangerousExtension(%q) should contain %q; got: %s",
					tt.filePath, tt.wantType, msg)
			}
		})
	}
}

func TestCheckWarnOnDangerousDisabled(t *testing.T) {
	t.Parallel()

	checker := newTestChecker()
	checker.WarnOnDangerous = false

	verdict, err := checker.Check(strings.NewReader(
		`{"tool_name":"Write","tool_input":{"file_path":"/project/app.exe"}}`))
	if err != nil {
		t.Fatalf("Check() error = %v", err)
	}

	if !verdict.Allow {
		t.Error("Check() should allow when WarnOnDangerous=false")
	}
	if verdict.Message != "" {
		t.Errorf("Check() Message should be empty when WarnOnDangerous=false; got: %s",
			verdict.Message)
	}
}

func TestBlockMessage(t *testing.T) {
	t.Parallel()

	v := block("Edit", "/project/.git/config", "Cannot write to '.git' directory")
	if v.Allow {
		t.Error("block() should not allow")
	}

	expectedPhrases := []string{
		"BLOCKED",
		"Edit",
		"/project/.git/config",
		".git",
		"CodeFlow",
	}
	for _, phrase := range expectedPhrases {
		if !strings.Contains(v.Message, phrase) {
			t.Errorf("block() Message missing %q; got: %s", phrase, v.Message)
		}
	}
}

func TestBlockMessageProtectedBranch(t *testing.T) {
	t.Parallel()

	v := block("Edit", "/project/src/main.go", "Cannot write files directly on protected branch 'main'. Create a feature branch first.")
	if v.Allow {
		t.Error("block() should not allow")
	}

	if !strings.Contains(v.Message, "cf-git-operations") {
		t.Errorf("Protected branch block should include cf-git-operations guidance; got: %s", v.Message)
	}
}

func TestCheckGracefulDegradation(t *testing.T) {
	t.Parallel()

	// Empty checker with no config should not panic.
	checker := &ScopeChecker{
		ProjectDir: "/project",
	}

	verdict, err := checker.Check(strings.NewReader(
		`{"tool_name":"Edit","tool_input":{"file_path":"/project/src/main.go"}}`))
	if err != nil {
		t.Fatalf("Check() error = %v", err)
	}

	if !verdict.Allow {
		t.Error("Check() with empty config should allow normal paths")
	}
}

func TestContainsExt(t *testing.T) {
	t.Parallel()

	exts := []string{"exe", "dll", "so"}

	if !containsExt(exts, "exe") {
		t.Error("containsExt should find exe")
	}
	if containsExt(exts, "go") {
		t.Error("containsExt should not find go")
	}
	if containsExt(nil, "exe") {
		t.Error("containsExt on nil should return false")
	}
}
