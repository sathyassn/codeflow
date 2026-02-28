package main

import (
	"fmt"
	"testing"
)

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
