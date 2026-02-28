package validate

import (
	"fmt"
	"os"
	"regexp"
)

// Epic validation constants.
var (
	epicRequiredFields = []string{"id", "format_id", "title", "status", "area_type", "work_type"}

	epicStatusValues = []string{"draft", "planning", "in_progress", "blocked", "complete", "archived"}

	epicFormatIDPattern = regexp.MustCompile(`^[A-Z]{2,4}-EPC-[0-9]{3}$`)

	epicRequiredSections = []string{
		"## Summary", "## Scope", "## Acceptance Criteria", "## Tasks",
		"## Dependencies", "## Technical Notes", "## Related",
	}

	epicTemplateSentinelFields = []string{"id", "title"}
)

// ValidateEpic validates an epic markdown file at the given path.
// It returns validation errors, warnings, and any I/O or parsing error.
func ValidateEpic(path string, opts ...Option) ([]ValidationError, []ValidationWarning, error) {
	_ = buildOptions(opts)

	content, err := os.ReadFile(path)
	if err != nil {
		return nil, nil, fmt.Errorf("validate: reading file: %w", err)
	}

	data, body, err := ParseFrontmatter(content)
	if err != nil {
		return nil, nil, err
	}

	var errs []ValidationError
	var warns []ValidationWarning

	// Required fields.
	errs = append(errs, validateRequiredFields(data, epicRequiredFields)...)

	// Template sentinels.
	errs = append(errs, checkTemplateSentinels(data, epicTemplateSentinelFields)...)

	// Format ID pattern.
	if fid := getStringField(data, "format_id"); fid != "" {
		if !epicFormatIDPattern.MatchString(fid) {
			errs = append(errs, ValidationError{
				Field:   "format_id",
				Message: fmt.Sprintf("does not match pattern %s", epicFormatIDPattern.String()),
			})
		}
	}

	// Enum validations.
	errs = append(errs, validateEnum(data, "status", epicStatusValues)...)
	errs = append(errs, validateEnum(data, "work_type", workTypeValues)...)
	errs = append(errs, validateEnum(data, "area_type", areaTypeValues)...)
	errs = append(errs, validateOptionalEnum(data, "priority", priorityValues)...)

	// Cross-field: format_id prefix must match area_type.
	errs = append(errs, validateFormatIDPrefix(data, "format_id", "-EPC-")...)

	// Section checks.
	errs = append(errs, validateSections(body, epicRequiredSections)...)

	// Warnings: filename vs format_id.
	warns = append(warns, checkFilenameMatch(path, data)...)

	// Warnings: PII patterns in file_scope.
	warns = append(warns, checkPIIPatterns(data)...)

	return errs, warns, nil
}
