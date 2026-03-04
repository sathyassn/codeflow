package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"strings"
	"testing"
)

// mockBuildVCS replaces the package-level readBuildVCS for a single test
// and restores the original when the test finishes.
func mockBuildVCS(t *testing.T, info *vcsInfo) {
	t.Helper()
	orig := readBuildVCS
	readBuildVCS = func() *vcsInfo { return info }
	t.Cleanup(func() { readBuildVCS = orig })
}

// Primary version/uninstall tests live in main_test.go. This file adds
// table-driven coverage for detectClaudeCodeVersionWith edge cases and
// satisfies the Go test file convention (version.go -> version_test.go).

func TestDetectClaudeCodeVersionWith_TableDriven(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name   string
		runner claudeRunner
		want   string
	}{
		{
			name:   "version with trailing newline",
			runner: func() ([]byte, error) { return []byte("1.0.16\n"), nil },
			want:   "1.0.16",
		},
		{
			name:   "version with surrounding whitespace",
			runner: func() ([]byte, error) { return []byte("  2.3.4  \n"), nil },
			want:   "2.3.4",
		},
		{
			name:   "multi-word version string",
			runner: func() ([]byte, error) { return []byte("1.0.18 (Claude Code)\n"), nil },
			want:   "1.0.18 (Claude Code)",
		},
		{
			name:   "command not found",
			runner: func() ([]byte, error) { return nil, fmt.Errorf("exec: not found") },
			want:   "unknown",
		},
		{
			name:   "completely empty output",
			runner: func() ([]byte, error) { return []byte(""), nil },
			want:   "unknown",
		},
		{
			name:   "whitespace only",
			runner: func() ([]byte, error) { return []byte("  \n  "), nil },
			want:   "unknown",
		},
		{
			name:   "nil output bytes",
			runner: func() ([]byte, error) { return nil, nil },
			want:   "unknown",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := detectClaudeCodeVersionWith(tt.runner)
			if got != tt.want {
				t.Errorf("detectClaudeCodeVersionWith() = %q, want %q", got, tt.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// VCS build info tests
// ---------------------------------------------------------------------------

func TestVersionCmd_WithVCSInfo(t *testing.T) {
	// NOTE: no t.Parallel — tests share package-level readBuildVCS mock
	mockBuildVCS(t, &vcsInfo{Revision: "abc1234", BuildTime: "2026-03-04T08:30:00Z"})

	cmd := newRootCmd()
	buf := &bytes.Buffer{}
	cmd.SetOut(buf)
	cmd.SetArgs([]string{"version"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("version command returned error: %v", err)
	}

	got := buf.String()
	expected := fmt.Sprintf("codeflow %s (abc1234 2026-03-04T08:30:00Z)\n", version)
	if got != expected {
		t.Errorf("version output: got %q, want %q", got, expected)
	}
}

func TestVersionCmd_WithoutVCSInfo(t *testing.T) {
	// NOTE: no t.Parallel — tests share package-level readBuildVCS mock
	mockBuildVCS(t, nil)

	cmd := newRootCmd()
	buf := &bytes.Buffer{}
	cmd.SetOut(buf)
	cmd.SetArgs([]string{"version"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("version command returned error: %v", err)
	}

	got := buf.String()
	expected := fmt.Sprintf("codeflow %s\n", version)
	if got != expected {
		t.Errorf("version output: got %q, want %q", got, expected)
	}
}

func TestVersionCmd_JSON_WithVCSInfo(t *testing.T) {
	// NOTE: no t.Parallel — tests share package-level readBuildVCS mock
	mockBuildVCS(t, &vcsInfo{Revision: "def5678", BuildTime: "2026-03-04T10:00:00Z"})

	cmd := newRootCmd()
	buf := &bytes.Buffer{}
	cmd.SetOut(buf)
	cmd.SetArgs([]string{"version", "--json"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("version --json returned error: %v", err)
	}

	var info versionInfo
	if err := json.Unmarshal(buf.Bytes(), &info); err != nil {
		t.Fatalf("parse JSON output: %v", err)
	}

	if info.VCSRevision != "def5678" {
		t.Errorf("vcs_revision = %q, want %q", info.VCSRevision, "def5678")
	}
	if info.VCSTime != "2026-03-04T10:00:00Z" {
		t.Errorf("vcs_time = %q, want %q", info.VCSTime, "2026-03-04T10:00:00Z")
	}
}

func TestVersionCmd_JSON_WithoutVCSInfo(t *testing.T) {
	// NOTE: no t.Parallel — tests share package-level readBuildVCS mock
	mockBuildVCS(t, nil)

	cmd := newRootCmd()
	buf := &bytes.Buffer{}
	cmd.SetOut(buf)
	cmd.SetArgs([]string{"version", "--json"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("version --json returned error: %v", err)
	}

	// VCS fields should be omitted from JSON when nil.
	got := buf.String()
	if strings.Contains(got, "vcs_revision") {
		t.Error("vcs_revision should be omitted when no VCS info")
	}
	if strings.Contains(got, "vcs_time") {
		t.Error("vcs_time should be omitted when no VCS info")
	}
}
