package validate

import (
	"errors"
	"path/filepath"
	"testing"
)

func TestValidateTask(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		fixture    string // Fixture file in testdata/ (relative name).
		wantErrors int    // Minimum expected error count (-1 to skip check).
		wantWarns  int    // Minimum expected warning count (-1 to skip check).
		wantErr    bool   // Expect I/O or parse error.
		checkError func(t *testing.T, errs []ValidationError, warns []ValidationWarning)
	}{
		{
			name:       "valid task",
			fixture:    "valid-task.md",
			wantErrors: 0,
			wantWarns:  -1,
		},
		{
			name:       "missing required fields",
			fixture:    "task-missing-fields.md",
			wantErrors: 5, // format_id, epic_id, status, area_type, work_type
			wantWarns:  -1,
			checkError: func(t *testing.T, errs []ValidationError, _ []ValidationWarning) {
				t.Helper()
				for _, field := range []string{"format_id", "epic_id", "status", "area_type", "work_type"} {
					if !hasError(errs, field, "required") {
						t.Errorf("expected required field error for %q", field)
					}
				}
			},
		},
		{
			name:       "invalid status enum",
			fixture:    "task-invalid-status.md",
			wantErrors: 1,
			wantWarns:  -1,
			checkError: func(t *testing.T, errs []ValidationError, _ []ValidationWarning) {
				t.Helper()
				if !hasError(errs, "status", "invalid value") {
					t.Error("expected status enum error")
				}
			},
		},
		{
			name:       "bad format_id pattern",
			fixture:    "task-bad-format-id.md",
			wantErrors: 1,
			wantWarns:  -1,
			checkError: func(t *testing.T, errs []ValidationError, _ []ValidationWarning) {
				t.Helper()
				if !hasError(errs, "format_id", "does not match pattern") {
					t.Error("expected format_id pattern error")
				}
			},
		},
		{
			name:       "missing sections",
			fixture:    "task-missing-sections.md",
			wantErrors: 7, // Missing 7 of 8 sections (Description is present).
			wantWarns:  -1,
			checkError: func(t *testing.T, errs []ValidationError, _ []ValidationWarning) {
				t.Helper()
				if !hasError(errs, "body", "Approach") {
					t.Error("expected missing Approach section error")
				}
				if !hasError(errs, "body", "Stage Reports") {
					t.Error("expected missing Stage Reports section error")
				}
			},
		},
		{
			name:       "autorun without acceptance",
			fixture:    "task-autorun-no-acceptance.md",
			wantErrors: 1,
			wantWarns:  -1,
			checkError: func(t *testing.T, errs []ValidationError, _ []ValidationWarning) {
				t.Helper()
				if !hasError(errs, "autorun_eligible", "acceptance is empty") {
					t.Error("expected autorun_eligible/acceptance error")
				}
			},
		},
		{
			name:       "code task without tests",
			fixture:    "task-code-no-tests.md",
			wantErrors: 1,
			wantWarns:  -1,
			checkError: func(t *testing.T, errs []ValidationError, _ []ValidationWarning) {
				t.Helper()
				if !hasError(errs, "tests", "requires non-empty tests") {
					t.Error("expected code task tests error")
				}
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			path := filepath.Join("testdata", tt.fixture)
			errs, warns, err := ValidateTask(path)
			if tt.wantErr {
				if err == nil {
					t.Error("ValidateTask() error = nil, want error")
				}
				return
			}
			if err != nil {
				t.Fatalf("ValidateTask() unexpected error: %v", err)
			}

			if tt.wantErrors >= 0 && len(errs) < tt.wantErrors {
				t.Errorf("ValidateTask() got %d errors, want >= %d: %v", len(errs), tt.wantErrors, errs)
			}
			if tt.wantWarns >= 0 && len(warns) < tt.wantWarns {
				t.Errorf("ValidateTask() got %d warnings, want >= %d: %v", len(warns), tt.wantWarns, warns)
			}
			if tt.checkError != nil {
				tt.checkError(t, errs, warns)
			}
		})
	}
}

func TestValidateTask_FileNotFound(t *testing.T) {
	t.Parallel()

	_, _, err := ValidateTask("/nonexistent/path/task.md")
	if err == nil {
		t.Error("ValidateTask() error = nil, want error for missing file")
	}
}

func TestValidateTask_NoFrontmatter(t *testing.T) {
	t.Parallel()

	path := writeFixture(t, "no-frontmatter.md", "# Just a heading\nNo YAML here.\n")
	_, _, err := ValidateTask(path)
	if err == nil {
		t.Error("ValidateTask() error = nil, want error for missing frontmatter")
	}
	if !errors.Is(err, ErrInvalidFrontmatter) {
		t.Errorf("ValidateTask() error = %v, want ErrInvalidFrontmatter", err)
	}
}

func TestValidateTask_TemplateSentinel(t *testing.T) {
	t.Parallel()

	content := `---
id: "{task-id-placeholder}"
format_id: "INF-TSK-001-001"
epic_id: "PLACEHOLDER-epic"
title: "Template {title}"
status: todo
area_type: "INF"
work_type: "FEAT"
---

# Template task

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
	path := writeFixture(t, "INF-TSK-001-001.md", content)
	errs, _, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	// Should detect sentinels in id, epic_id, and title.
	if !hasError(errs, "id", "template sentinel") {
		t.Error("expected template sentinel error for id")
	}
	if !hasError(errs, "epic_id", "placeholder") {
		t.Error("expected placeholder error for epic_id")
	}
	if !hasError(errs, "title", "template sentinel") {
		t.Error("expected template sentinel error for title")
	}
}

func TestValidateTask_AutoMergeProtectedBranch(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Auto merge on main"
status: todo
area_type: "INF"
work_type: "FEAT"
auto_merge: true
raise_pr: true
target_branch: "main"
---

# Auto merge on protected branch

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
	path := writeFixture(t, "INF-TSK-001-001.md", content)
	errs, _, err := ValidateTask(path, WithProtectedBranches([]string{"main", "master"}))
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	if !hasError(errs, "auto_merge", "protected branch") {
		t.Error("expected auto_merge protected branch error")
	}
}

func TestValidateTask_AutoMergeNoTargetBranch(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Auto merge no target"
status: todo
area_type: "INF"
work_type: "FEAT"
auto_merge: true
raise_pr: true
---

# Auto merge no target

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
	path := writeFixture(t, "INF-TSK-001-001.md", content)
	errs, _, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	if !hasError(errs, "auto_merge", "target_branch is not set") {
		t.Error("expected auto_merge missing target_branch error")
	}
}

func TestValidateTask_RaisePRFalseAutoMergeTrue(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Contradictory PR settings"
status: todo
area_type: "INF"
work_type: "FEAT"
raise_pr: false
auto_merge: true
target_branch: "develop"
---

# Contradictory settings

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
	path := writeFixture(t, "INF-TSK-001-001.md", content)
	errs, _, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	if !hasError(errs, "auto_merge", "raise_pr to be true") {
		t.Error("expected raise_pr/auto_merge contradiction error")
	}
}

func TestValidateTask_InvalidBooleanField(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Invalid boolean"
status: todo
area_type: "INF"
work_type: "FEAT"
autorun_eligible: "yes"
---

# Invalid boolean

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
	path := writeFixture(t, "INF-TSK-001-001.md", content)
	errs, _, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	if !hasError(errs, "autorun_eligible", "must be true or false") {
		t.Error("expected boolean validation error for autorun_eligible")
	}
}

func TestValidateTask_FormatIDAreaTypeMismatch(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "FRT-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Area mismatch"
status: todo
area_type: "INF"
work_type: "FEAT"
---

# Area mismatch

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
	path := writeFixture(t, "FRT-TSK-001-001.md", content)
	errs, _, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	if !hasError(errs, "format_id", "does not match area_type") {
		t.Error("expected format_id/area_type mismatch error")
	}
}

func TestValidateTask_FilenameWarning(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Filename mismatch"
status: todo
area_type: "INF"
work_type: "FEAT"
---

# Filename mismatch

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
	// Write with a non-matching filename.
	path := writeFixture(t, "wrong-name.md", content)
	_, warns, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	if !hasWarning(warns, "format_id", "does not match") {
		t.Error("expected filename mismatch warning")
	}
}

func TestValidateTask_PIIWarning(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "PII task"
status: todo
area_type: "INF"
work_type: "FEAT"
file_scope: ["src/auth/login.go", "src/session/manager.go"]
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
	path := writeFixture(t, "INF-TSK-001-001.md", content)
	_, warns, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	if !hasWarning(warns, "file_scope", "PII-sensitive") {
		t.Error("expected PII warning for file_scope")
	}
}

func TestValidateTask_OptionalEnums(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Bad optional enums"
status: todo
area_type: "INF"
work_type: "FEAT"
origin: "invalid_origin"
priority: "mega"
estimate: "XXL"
stage: "unknown"
stage_status: "broken"
scope_policy: "chaos"
---

# Bad optional enums

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
	path := writeFixture(t, "INF-TSK-001-001.md", content)
	errs, _, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	for _, field := range []string{"origin", "priority", "estimate", "stage", "stage_status", "scope_policy"} {
		if !hasError(errs, field, "invalid value") {
			t.Errorf("expected invalid value error for optional enum %q", field)
		}
	}
}

func TestValidateTask_ValidOptionalEnums(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Good optional enums"
status: todo
area_type: "INF"
work_type: "FEAT"
origin: planned
priority: high
estimate: "M"
stage: dev
stage_status: pending
scope_policy: hard
---

# Good optional enums

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
	path := writeFixture(t, "INF-TSK-001-001.md", content)
	errs, _, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	if len(errs) != 0 {
		t.Errorf("ValidateTask() got %d errors, want 0: %v", len(errs), errs)
	}
}

func TestValidateTask_CodeTaskWithTests(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "INF-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Code task with tests"
status: todo
area_type: "INF"
work_type: "FEAT"
file_scope: ["scripts/setup.sh"]
tests: ["tests/test-setup.sh"]
---

# Code task with tests

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
	path := writeFixture(t, "INF-TSK-001-001.md", content)
	errs, _, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	if hasError(errs, "tests", "") {
		t.Error("unexpected tests error — code task has tests defined")
	}
}

func TestValidateTask_NonCodeWorkTypeNoTestsOK(t *testing.T) {
	t.Parallel()

	content := `---
id: "task-01ABC"
format_id: "DOC-TSK-001-001"
epic_id: "epic-01XYZ"
title: "Docs task"
status: todo
area_type: "DOC"
work_type: "DOCS"
file_scope: ["docs/guide.md"]
tests: []
---

# Docs task

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
	path := writeFixture(t, "DOC-TSK-001-001.md", content)
	errs, _, err := ValidateTask(path)
	if err != nil {
		t.Fatalf("ValidateTask() unexpected error: %v", err)
	}

	if hasError(errs, "tests", "") {
		t.Error("unexpected tests error — DOCS work type should not require tests")
	}
}
