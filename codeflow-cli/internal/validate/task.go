package validate

import (
	"fmt"
	"os"
	"regexp"
	"strings"
)

// Task validation constants.
var (
	taskRequiredFields = []string{"id", "format_id", "epic_id", "title", "status", "area_type", "work_type"}

	taskStatusValues  = []string{"todo", "blocked", "in_progress", "complete", "cancelled"}
	originValues      = []string{"planned", "informal", "auto"}
	scopePolicyValues = []string{"soft", "hard", "permissive"}
	estimateValues    = []string{"XS", "S", "M", "L", "XL"}
	stageValues       = []string{"dev", "review", "qa", "done"}
	stageStatusValues = []string{"pending", "in_progress", "complete", "failed"}

	taskBooleanFields = []string{"autorun_eligible", "raise_pr", "auto_merge"}

	codeWorkTypes = map[string]bool{
		"FEAT": true, "FIX": true, "RFCT": true, "HTFX": true,
		"CHOR": true, "CICD": true, "TEST": true,
	}

	taskFormatIDPattern = regexp.MustCompile(`^[A-Z]{2,4}-TSK-[0-9]{3}-[0-9]{3}$`)

	taskRequiredSections = []string{
		"## Description", "## Approach", "## Files", "## Acceptance Criteria",
		"## Dependencies", "## Verification", "## Stage Reports", "## Notes",
	}

	taskTemplateSentinelFields = []string{"id", "title", "epic_id"}
)

// ValidateTask validates a task markdown file at the given path.
// It returns validation errors, warnings, and any I/O or parsing error.
func ValidateTask(path string, opts ...Option) ([]ValidationError, []ValidationWarning, error) {
	o := buildOptions(opts)

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
	errs = append(errs, validateRequiredFields(data, taskRequiredFields)...)

	// Template sentinels.
	errs = append(errs, checkTemplateSentinels(data, taskTemplateSentinelFields)...)

	// Format ID pattern.
	if fid := getStringField(data, "format_id"); fid != "" {
		if !taskFormatIDPattern.MatchString(fid) {
			errs = append(errs, ValidationError{
				Field:   "format_id",
				Message: fmt.Sprintf("does not match pattern %s", taskFormatIDPattern.String()),
			})
		}
	}

	// Enum validations.
	errs = append(errs, validateEnum(data, "status", taskStatusValues)...)
	errs = append(errs, validateEnum(data, "work_type", workTypeValues)...)
	errs = append(errs, validateEnum(data, "area_type", areaTypeValues)...)
	errs = append(errs, validateOptionalEnum(data, "origin", originValues)...)
	errs = append(errs, validateOptionalEnum(data, "scope_policy", scopePolicyValues)...)
	errs = append(errs, validateOptionalEnum(data, "priority", priorityValues)...)
	errs = append(errs, validateOptionalEnum(data, "estimate", estimateValues)...)
	errs = append(errs, validateOptionalEnum(data, "stage", stageValues)...)
	errs = append(errs, validateOptionalEnum(data, "stage_status", stageStatusValues)...)

	// Boolean fields.
	errs = append(errs, validateBooleanFields(data, taskBooleanFields)...)

	// Cross-field: format_id prefix must match area_type.
	errs = append(errs, validateFormatIDPrefix(data, "format_id", "-TSK-")...)

	// Cross-field: auto_merge validations.
	errs = append(errs, validateAutoMerge(data, o.protectedBranches)...)

	// Cross-field: raise_pr false + auto_merge true.
	errs = append(errs, validateRaisePRAutoMerge(data)...)

	// Cross-field: autorun_eligible requires acceptance.
	errs = append(errs, validateAutorunAcceptance(data)...)

	// Cross-field: code work type with .sh/.py in file_scope requires tests.
	errs = append(errs, validateCodeTaskTests(data)...)

	// Section checks.
	errs = append(errs, validateSections(body, taskRequiredSections)...)

	// Warnings: filename vs format_id.
	warns = append(warns, checkFilenameMatch(path, data)...)

	// Warnings: PII patterns in file_scope.
	warns = append(warns, checkPIIPatterns(data)...)

	return errs, warns, nil
}

// validateAutoMerge checks auto_merge cross-field constraints.
func validateAutoMerge(data map[string]any, protectedBranches []string) []ValidationError {
	autoMerge, set := getBoolField(data, "auto_merge")
	if !set || !autoMerge {
		return nil
	}

	var errs []ValidationError

	targetBranch := getStringField(data, "target_branch")
	if targetBranch == "" {
		errs = append(errs, ValidationError{
			Field:   "auto_merge",
			Message: "auto_merge is true but target_branch is not set",
		})
		return errs
	}

	for _, pb := range protectedBranches {
		if targetBranch == pb {
			errs = append(errs, ValidationError{
				Field:   "auto_merge",
				Message: fmt.Sprintf("auto_merge is true but target_branch %q is a protected branch", targetBranch),
			})
			break
		}
	}

	return errs
}

// validateRaisePRAutoMerge checks raise_pr/auto_merge contradiction.
func validateRaisePRAutoMerge(data map[string]any) []ValidationError {
	raisePR, raisePRSet := getBoolField(data, "raise_pr")
	autoMerge, autoMergeSet := getBoolField(data, "auto_merge")

	if raisePRSet && !raisePR && autoMergeSet && autoMerge {
		return []ValidationError{{
			Field:   "auto_merge",
			Message: "auto_merge requires raise_pr to be true",
		}}
	}
	return nil
}

// validateAutorunAcceptance checks that autorun_eligible tasks have acceptance.
func validateAutorunAcceptance(data map[string]any) []ValidationError {
	autorun, set := getBoolField(data, "autorun_eligible")
	if !set || !autorun {
		return nil
	}

	if isFieldEmpty(data, "acceptance") {
		return []ValidationError{{
			Field:   "autorun_eligible",
			Message: "autorun_eligible is true but acceptance is empty",
		}}
	}
	return nil
}

// validateCodeTaskTests checks that code work types with .sh/.py files have tests.
func validateCodeTaskTests(data map[string]any) []ValidationError {
	wt := getStringField(data, "work_type")
	if !codeWorkTypes[wt] {
		return nil
	}

	v, ok := data["file_scope"]
	if !ok || v == nil {
		return nil
	}

	hasCodeFile := false
	switch val := v.(type) {
	case []any:
		for _, item := range val {
			s := fmt.Sprint(item)
			if strings.Contains(s, ".sh") || strings.Contains(s, ".py") {
				hasCodeFile = true
				break
			}
		}
	case string:
		if val != "" && val != "[]" && val != "null" && val != "~" {
			if strings.Contains(val, ".sh") || strings.Contains(val, ".py") {
				hasCodeFile = true
			}
		}
	}

	if !hasCodeFile {
		return nil
	}

	if isFieldEmpty(data, "tests") {
		return []ValidationError{{
			Field:   "tests",
			Message: "code work type with .sh/.py files in file_scope requires non-empty tests field",
		}}
	}
	return nil
}
