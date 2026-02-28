package validate

import (
	"errors"
	"path/filepath"
	"testing"
)

func TestValidateEpic(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		fixture    string
		wantErrors int
		wantWarns  int
		wantErr    bool
		checkError func(t *testing.T, errs []ValidationError, warns []ValidationWarning)
	}{
		{
			name:       "valid epic",
			fixture:    "valid-epic.md",
			wantErrors: 0,
			wantWarns:  -1,
		},
		{
			name:       "invalid status",
			fixture:    "epic-invalid-status.md",
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
			name:       "missing sections",
			fixture:    "epic-missing-sections.md",
			wantErrors: 6, // Missing 6 of 7 sections (Summary is present).
			wantWarns:  -1,
			checkError: func(t *testing.T, errs []ValidationError, _ []ValidationWarning) {
				t.Helper()
				if !hasError(errs, "body", "Scope") {
					t.Error("expected missing Scope section error")
				}
				if !hasError(errs, "body", "Technical Notes") {
					t.Error("expected missing Technical Notes section error")
				}
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			path := filepath.Join("testdata", tt.fixture)
			errs, warns, err := ValidateEpic(path)
			if tt.wantErr {
				if err == nil {
					t.Error("ValidateEpic() error = nil, want error")
				}
				return
			}
			if err != nil {
				t.Fatalf("ValidateEpic() unexpected error: %v", err)
			}

			if tt.wantErrors >= 0 && len(errs) < tt.wantErrors {
				t.Errorf("ValidateEpic() got %d errors, want >= %d: %v", len(errs), tt.wantErrors, errs)
			}
			if tt.wantWarns >= 0 && len(warns) < tt.wantWarns {
				t.Errorf("ValidateEpic() got %d warnings, want >= %d: %v", len(warns), tt.wantWarns, warns)
			}
			if tt.checkError != nil {
				tt.checkError(t, errs, warns)
			}
		})
	}
}

func TestValidateEpic_FileNotFound(t *testing.T) {
	t.Parallel()

	_, _, err := ValidateEpic("/nonexistent/path/epic.md")
	if err == nil {
		t.Error("ValidateEpic() error = nil, want error for missing file")
	}
}

func TestValidateEpic_NoFrontmatter(t *testing.T) {
	t.Parallel()

	path := writeFixture(t, "no-frontmatter.md", "# Just a heading\n")
	_, _, err := ValidateEpic(path)
	if err == nil {
		t.Error("ValidateEpic() error = nil, want error for missing frontmatter")
	}
	if !errors.Is(err, ErrInvalidFrontmatter) {
		t.Errorf("ValidateEpic() error = %v, want ErrInvalidFrontmatter", err)
	}
}

func TestValidateEpic_MissingFields(t *testing.T) {
	t.Parallel()

	content := `---
id: "epic-01ABC"
title: "Missing fields"
---

# Missing fields

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
`
	path := writeFixture(t, "missing.md", content)
	errs, _, err := ValidateEpic(path)
	if err != nil {
		t.Fatalf("ValidateEpic() unexpected error: %v", err)
	}

	// Should be missing: format_id, status, area_type, work_type.
	for _, field := range []string{"format_id", "status", "area_type", "work_type"} {
		if !hasError(errs, field, "required") {
			t.Errorf("expected required field error for %q", field)
		}
	}
}

func TestValidateEpic_FormatIDPattern(t *testing.T) {
	t.Parallel()

	content := `---
id: "epic-01ABC"
format_id: "INF-BADFORMAT"
title: "Bad format"
status: draft
area_type: "INF"
work_type: "FEAT"
---

# Bad format

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
`
	path := writeFixture(t, "bad.md", content)
	errs, _, err := ValidateEpic(path)
	if err != nil {
		t.Fatalf("ValidateEpic() unexpected error: %v", err)
	}

	if !hasError(errs, "format_id", "does not match pattern") {
		t.Error("expected format_id pattern error")
	}
}

func TestValidateEpic_FormatIDAreaTypeMismatch(t *testing.T) {
	t.Parallel()

	content := `---
id: "epic-01ABC"
format_id: "FRT-EPC-001"
title: "Area mismatch"
status: draft
area_type: "INF"
work_type: "FEAT"
---

# Area mismatch

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
`
	path := writeFixture(t, "FRT-EPC-001.md", content)
	errs, _, err := ValidateEpic(path)
	if err != nil {
		t.Fatalf("ValidateEpic() unexpected error: %v", err)
	}

	if !hasError(errs, "format_id", "does not match area_type") {
		t.Error("expected format_id/area_type mismatch error")
	}
}

func TestValidateEpic_TemplateSentinel(t *testing.T) {
	t.Parallel()

	content := `---
id: "PLACEHOLDER"
format_id: "INF-EPC-001"
title: "Epic {title}"
status: draft
area_type: "INF"
work_type: "FEAT"
---

# Epic template

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
`
	path := writeFixture(t, "INF-EPC-001.md", content)
	errs, _, err := ValidateEpic(path)
	if err != nil {
		t.Fatalf("ValidateEpic() unexpected error: %v", err)
	}

	if !hasError(errs, "id", "placeholder") {
		t.Error("expected placeholder error for id")
	}
	if !hasError(errs, "title", "template sentinel") {
		t.Error("expected template sentinel error for title")
	}
}

func TestValidateEpic_PIIWarning(t *testing.T) {
	t.Parallel()

	content := `---
id: "epic-01ABC"
format_id: "INF-EPC-001"
title: "PII epic"
status: draft
area_type: "INF"
work_type: "FEAT"
file_scope: ["src/auth/handler.go"]
---

# PII epic

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
`
	path := writeFixture(t, "INF-EPC-001.md", content)
	_, warns, err := ValidateEpic(path)
	if err != nil {
		t.Fatalf("ValidateEpic() unexpected error: %v", err)
	}

	if !hasWarning(warns, "file_scope", "PII-sensitive") {
		t.Error("expected PII warning for file_scope")
	}
}

func TestValidateEpic_InvalidPriority(t *testing.T) {
	t.Parallel()

	content := `---
id: "epic-01ABC"
format_id: "INF-EPC-001"
title: "Bad priority"
status: draft
area_type: "INF"
work_type: "FEAT"
priority: "mega"
---

# Bad priority

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
`
	path := writeFixture(t, "INF-EPC-001.md", content)
	errs, _, err := ValidateEpic(path)
	if err != nil {
		t.Fatalf("ValidateEpic() unexpected error: %v", err)
	}

	if !hasError(errs, "priority", "invalid value") {
		t.Error("expected priority enum error")
	}
}
