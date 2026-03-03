package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// writeValidateFixture creates a temp file with content and returns its path.
func writeValidateFixture(t *testing.T, name, content string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), name)
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("writeValidateFixture(%s): %v", name, err)
	}
	return path
}

func TestRunValidateTask(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		content    string
		filename   string
		wantErr    bool
		wantStdout string // Substring expected in stdout.
		wantStderr string // Substring expected in stderr.
	}{
		{
			name: "valid task",
			content: `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Valid"
status: todo
area_type: "INF"
work_type: "FEAT"
---

# Valid task

## Description
x
## Approach
x
## Files
x
## Acceptance Criteria
x
## Dependencies
x
## Verification
x
## Stage Reports
x
## Notes
x
`,
			filename:   "INF-TSK-001-001.md",
			wantStdout: "[INFO] Validation PASSED",
		},
		{
			name: "invalid status",
			content: `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Bad status"
status: invalid
area_type: "INF"
work_type: "FEAT"
---

# Bad status

## Description
x
## Approach
x
## Files
x
## Acceptance Criteria
x
## Dependencies
x
## Verification
x
## Stage Reports
x
## Notes
x
`,
			filename:   "INF-TSK-001-001.md",
			wantErr:    true,
			wantStderr: "Validation FAILED",
		},
		{
			name:       "file not found",
			content:    "",
			filename:   "",
			wantErr:    true,
			wantStderr: "[ERROR]",
		},
		{
			name:       "no frontmatter",
			content:    "# Just markdown\nNo YAML here.\n",
			filename:   "nofm.md",
			wantErr:    true,
			wantStderr: "[ERROR]",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			var path string
			if tt.filename == "" {
				path = filepath.Join(t.TempDir(), "nonexistent", "missing.md")
			} else {
				path = writeValidateFixture(t, tt.filename, tt.content)
			}

			var stdout, stderr bytes.Buffer
			err := runValidateTask(&stdout, &stderr, path)

			if tt.wantErr && err == nil {
				t.Error("runValidateTask() error = nil, want error")
			}
			if !tt.wantErr && err != nil {
				t.Errorf("runValidateTask() unexpected error: %v", err)
			}
			if tt.wantStdout != "" && !strings.Contains(stdout.String(), tt.wantStdout) {
				t.Errorf("stdout = %q, want substring %q", stdout.String(), tt.wantStdout)
			}
			if tt.wantStderr != "" && !strings.Contains(stderr.String(), tt.wantStderr) {
				t.Errorf("stderr = %q, want substring %q", stderr.String(), tt.wantStderr)
			}
		})
	}
}

func TestRunValidateTask_WithWarnings(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "PII warning task"
status: todo
area_type: "INF"
work_type: "FEAT"
file_scope: ["src/auth/login.go"]
---

# PII task

## Description
x
## Approach
x
## Files
x
## Acceptance Criteria
x
## Dependencies
x
## Verification
x
## Stage Reports
x
## Notes
x
`
	// Use non-matching filename to also trigger filename warning.
	path := writeValidateFixture(t, "wrong-name.md", content)

	var stdout, stderr bytes.Buffer
	err := runValidateTask(&stdout, &stderr, path)
	if err != nil {
		t.Fatalf("runValidateTask() unexpected error: %v", err)
	}

	if !strings.Contains(stdout.String(), "Validation PASSED with") {
		t.Errorf("stdout = %q, want 'Validation PASSED with'", stdout.String())
	}
	if !strings.Contains(stderr.String(), "[WARN]") {
		t.Errorf("stderr = %q, want [WARN] prefix", stderr.String())
	}
}

func TestRunValidateEpic(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		content    string
		filename   string
		wantErr    bool
		wantStdout string
		wantStderr string
	}{
		{
			name: "valid epic",
			content: `---
id: "epic-01ABC"
format_id: "INF-EPC-001"
title: "Valid"
status: draft
area_type: "INF"
work_type: "FEAT"
---

# Valid epic

## Summary
x
## Scope
x
## Acceptance Criteria
x
## Tasks
x
## Dependencies
x
## Technical Notes
x
## Related
x
`,
			filename:   "INF-EPC-001.md",
			wantStdout: "[INFO] Validation PASSED",
		},
		{
			name: "invalid epic",
			content: `---
id: "epic-01ABC"
format_id: "INF-EPC-001"
title: "Bad"
status: bogus
area_type: "INF"
work_type: "FEAT"
---

# Bad epic

## Summary
x
## Scope
x
## Acceptance Criteria
x
## Tasks
x
## Dependencies
x
## Technical Notes
x
## Related
x
`,
			filename:   "INF-EPC-001.md",
			wantErr:    true,
			wantStderr: "Validation FAILED",
		},
		{
			name:       "file not found",
			content:    "",
			filename:   "",
			wantErr:    true,
			wantStderr: "[ERROR]",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			var path string
			if tt.filename == "" {
				path = filepath.Join(t.TempDir(), "nonexistent", "missing.md")
			} else {
				path = writeValidateFixture(t, tt.filename, tt.content)
			}

			var stdout, stderr bytes.Buffer
			err := runValidateEpic(&stdout, &stderr, path)

			if tt.wantErr && err == nil {
				t.Error("runValidateEpic() error = nil, want error")
			}
			if !tt.wantErr && err != nil {
				t.Errorf("runValidateEpic() unexpected error: %v", err)
			}
			if tt.wantStdout != "" && !strings.Contains(stdout.String(), tt.wantStdout) {
				t.Errorf("stdout = %q, want substring %q", stdout.String(), tt.wantStdout)
			}
			if tt.wantStderr != "" && !strings.Contains(stderr.String(), tt.wantStderr) {
				t.Errorf("stderr = %q, want substring %q", stderr.String(), tt.wantStderr)
			}
		})
	}
}

func TestNewValidateCmd(t *testing.T) {
	t.Parallel()

	cmd := newValidateCmd()
	if cmd.Use != "validate" {
		t.Errorf("newValidateCmd().Use = %q, want %q", cmd.Use, "validate")
	}
	if !cmd.HasSubCommands() {
		t.Error("newValidateCmd() has no subcommands, want task and epic")
	}

	// Verify subcommand names.
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	for _, want := range []string{"task", "epic"} {
		if !names[want] {
			t.Errorf("newValidateCmd() missing subcommand %q", want)
		}
	}
}
