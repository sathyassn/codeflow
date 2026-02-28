// Package validate provides YAML frontmatter validation for task and epic markdown files.
package validate

import (
	"errors"
	"fmt"
	"path/filepath"
	"strings"

	"gopkg.in/yaml.v3"
)

// ErrInvalidFrontmatter indicates the file does not contain valid YAML frontmatter.
var ErrInvalidFrontmatter = errors.New("validate: invalid frontmatter")

// ValidationError represents a validation error found in a markdown file.
type ValidationError struct {
	Field   string
	Message string
}

// Error implements the error interface.
func (e ValidationError) Error() string {
	if e.Field != "" {
		return fmt.Sprintf("%s: %s", e.Field, e.Message)
	}
	return e.Message
}

// ValidationWarning represents a non-blocking validation warning.
type ValidationWarning struct {
	Field   string
	Message string
}

// String returns a human-readable representation of the warning.
func (w ValidationWarning) String() string {
	if w.Field != "" {
		return fmt.Sprintf("%s: %s", w.Field, w.Message)
	}
	return w.Message
}

// Option configures validation behavior.
type Option func(*options)

type options struct {
	protectedBranches []string
}

var defaultProtectedBranches = []string{"main", "master", "release/*", "production"}

// WithProtectedBranches overrides the default protected branch list
// used for auto_merge validation.
func WithProtectedBranches(branches []string) Option {
	return func(o *options) {
		o.protectedBranches = branches
	}
}

func buildOptions(opts []Option) *options {
	o := &options{
		protectedBranches: defaultProtectedBranches,
	}
	for _, opt := range opts {
		opt(o)
	}
	return o
}

// ParseFrontmatter extracts YAML frontmatter from markdown content.
// It returns the parsed YAML as a map, the remaining body content, and any error.
// The frontmatter must be enclosed between two "---" delimiter lines at the
// start of the content.
func ParseFrontmatter(content []byte) (map[string]any, []byte, error) {
	s := string(content)

	// Skip BOM if present.
	s = strings.TrimPrefix(s, "\xEF\xBB\xBF")

	// Must start with "---" on the first line.
	if !strings.HasPrefix(s, "---") {
		return nil, nil, fmt.Errorf("%w: missing opening delimiter", ErrInvalidFrontmatter)
	}

	// Find end of opening delimiter line.
	idx := strings.Index(s, "\n")
	if idx < 0 {
		return nil, nil, fmt.Errorf("%w: no content after opening delimiter", ErrInvalidFrontmatter)
	}

	rest := s[idx+1:]

	// Find closing "---" on its own line.
	closing := findClosingDelim(rest)
	if closing < 0 {
		return nil, nil, fmt.Errorf("%w: missing closing delimiter", ErrInvalidFrontmatter)
	}

	yamlStr := rest[:closing]

	// Body starts after closing delimiter line.
	afterClosing := rest[closing:]
	bodyIdx := strings.Index(afterClosing, "\n")
	var body string
	if bodyIdx >= 0 {
		body = afterClosing[bodyIdx+1:]
	}

	var data map[string]any
	if err := yaml.Unmarshal([]byte(yamlStr), &data); err != nil {
		return nil, nil, fmt.Errorf("%w: %v", ErrInvalidFrontmatter, err)
	}
	if data == nil {
		return nil, nil, fmt.Errorf("%w: empty YAML content", ErrInvalidFrontmatter)
	}

	return data, []byte(body), nil
}

// findClosingDelim finds the byte offset of the closing "---" delimiter line
// within content. Returns -1 if not found.
func findClosingDelim(s string) int {
	pos := 0
	for pos < len(s) {
		lineEnd := strings.Index(s[pos:], "\n")
		var line string
		if lineEnd < 0 {
			line = s[pos:]
		} else {
			line = s[pos : pos+lineEnd]
		}

		trimmed := strings.TrimRight(line, " \t\r")
		if trimmed == "---" {
			return pos
		}

		if lineEnd < 0 {
			break
		}
		pos += lineEnd + 1
	}
	return -1
}

// Shared validation helpers used by both task and epic validators.

// Shared enum value lists.
var (
	workTypeValues = []string{"FEAT", "FIX", "HTFX", "RFCT", "DOCS", "TEST", "CHOR", "CICD", "SPKE", "PLAN"}
	areaTypeValues = []string{"FRT", "BKD", "INF", "SHR", "DOC", "PLN"}
	priorityValues = []string{"low", "normal", "high", "critical"}
)

// PII-sensitive path patterns.
var piiPatterns = []string{
	"auth", "login", "user", "session", "password",
	"credential", "token", "account", "profile", "identity",
}

// getStringField extracts a string field from the frontmatter map.
// Returns empty string if the field is nil, null, ~, or not present.
func getStringField(data map[string]any, key string) string {
	v, ok := data[key]
	if !ok || v == nil {
		return ""
	}
	switch val := v.(type) {
	case string:
		if val == "null" || val == "~" {
			return ""
		}
		return val
	case bool:
		return fmt.Sprintf("%t", val)
	case int:
		return fmt.Sprintf("%d", val)
	case float64:
		return fmt.Sprintf("%g", val)
	default:
		return fmt.Sprintf("%v", val)
	}
}

// isFieldEmpty returns true if the field value represents an empty/null/unset value.
func isFieldEmpty(data map[string]any, key string) bool {
	v, ok := data[key]
	if !ok || v == nil {
		return true
	}
	switch val := v.(type) {
	case string:
		return val == "" || val == "null" || val == "~"
	case []any:
		return len(val) == 0
	default:
		return false
	}
}

// getBoolField returns the boolean value of a field, and whether it was set
// to a valid boolean.
func getBoolField(data map[string]any, key string) (bool, bool) {
	v, ok := data[key]
	if !ok || v == nil {
		return false, false
	}
	switch val := v.(type) {
	case bool:
		return val, true
	case string:
		switch val {
		case "true":
			return true, true
		case "false":
			return false, true
		}
	}
	return false, false
}

// validateRequiredFields checks that all required fields are present and non-empty.
func validateRequiredFields(data map[string]any, required []string) []ValidationError {
	var errs []ValidationError
	for _, field := range required {
		if isFieldEmpty(data, field) {
			errs = append(errs, ValidationError{
				Field:   field,
				Message: "required field is missing or empty",
			})
		}
	}
	return errs
}

// validateEnum checks that a required field's value is in the allowed set.
func validateEnum(data map[string]any, field string, allowed []string) []ValidationError {
	val := getStringField(data, field)
	if val == "" {
		return nil // Required field check already handles this.
	}
	for _, a := range allowed {
		if val == a {
			return nil
		}
	}
	return []ValidationError{{
		Field:   field,
		Message: fmt.Sprintf("invalid value %q, must be one of: %s", val, strings.Join(allowed, ", ")),
	}}
}

// validateOptionalEnum checks an optional field's value against allowed values.
// If the field is absent or empty, no error is returned.
func validateOptionalEnum(data map[string]any, field string, allowed []string) []ValidationError {
	if isFieldEmpty(data, field) {
		return nil
	}
	return validateEnum(data, field, allowed)
}

// validateBooleanFields checks that boolean fields contain valid boolean values.
func validateBooleanFields(data map[string]any, fields []string) []ValidationError {
	var errs []ValidationError
	for _, field := range fields {
		v, ok := data[field]
		if !ok || v == nil {
			continue
		}
		switch val := v.(type) {
		case bool:
			// Valid.
		case string:
			if val != "true" && val != "false" {
				errs = append(errs, ValidationError{
					Field:   field,
					Message: fmt.Sprintf("must be true or false, got %q", val),
				})
			}
		default:
			errs = append(errs, ValidationError{
				Field:   field,
				Message: fmt.Sprintf("must be true or false, got %v", val),
			})
		}
	}
	return errs
}

// checkTemplateSentinels checks fields for unfilled template markers.
func checkTemplateSentinels(data map[string]any, fields []string) []ValidationError {
	var errs []ValidationError
	for _, field := range fields {
		val := getStringField(data, field)
		if val == "" {
			continue
		}
		if strings.Contains(val, "{") || strings.Contains(val, "}") {
			errs = append(errs, ValidationError{
				Field:   field,
				Message: "contains unfilled template sentinel",
			})
		}
		if strings.Contains(strings.ToUpper(val), "PLACEHOLDER") {
			errs = append(errs, ValidationError{
				Field:   field,
				Message: "contains placeholder value",
			})
		}
	}
	return errs
}

// validateFormatIDPrefix checks that the format_id prefix matches area_type.
func validateFormatIDPrefix(data map[string]any, field, separator string) []ValidationError {
	fid := getStringField(data, field)
	areaType := getStringField(data, "area_type")
	if fid == "" || areaType == "" {
		return nil
	}

	idx := strings.Index(fid, separator)
	if idx < 0 {
		return nil // Pattern validation will catch malformed format_id.
	}
	prefix := fid[:idx]
	if prefix != areaType {
		return []ValidationError{{
			Field:   field,
			Message: fmt.Sprintf("prefix %q does not match area_type %q", prefix, areaType),
		}}
	}
	return nil
}

// validateSections checks that all required section headings exist in the body.
func validateSections(body []byte, required []string) []ValidationError {
	bodyStr := string(body)
	var errs []ValidationError
	for _, section := range required {
		if !strings.Contains(bodyStr, "\n"+section) && !strings.HasPrefix(bodyStr, section) {
			errs = append(errs, ValidationError{
				Field:   "body",
				Message: fmt.Sprintf("missing required section %q", section),
			})
		}
	}
	return errs
}

// checkFilenameMatch warns if the filename doesn't match format_id.
func checkFilenameMatch(path string, data map[string]any) []ValidationWarning {
	fid := getStringField(data, "format_id")
	if fid == "" {
		return nil
	}
	base := strings.TrimSuffix(filepath.Base(path), ".md")
	if base != fid {
		return []ValidationWarning{{
			Field:   "format_id",
			Message: fmt.Sprintf("filename %q does not match format_id %q", filepath.Base(path), fid),
		}}
	}
	return nil
}

// checkPIIPatterns warns if file_scope contains PII-sensitive path patterns.
func checkPIIPatterns(data map[string]any) []ValidationWarning {
	v, ok := data["file_scope"]
	if !ok || v == nil {
		return nil
	}

	var scopeStr string
	switch val := v.(type) {
	case []any:
		if len(val) == 0 {
			return nil
		}
		parts := make([]string, 0, len(val))
		for _, item := range val {
			parts = append(parts, strings.ToLower(fmt.Sprint(item)))
		}
		scopeStr = strings.Join(parts, " ")
	case string:
		scopeStr = strings.ToLower(val)
	default:
		scopeStr = strings.ToLower(fmt.Sprint(val))
	}

	if scopeStr == "" || scopeStr == "[]" || scopeStr == "null" || scopeStr == "~" {
		return nil
	}

	for _, pattern := range piiPatterns {
		if strings.Contains(scopeStr, pattern) {
			return []ValidationWarning{{
				Field:   "file_scope",
				Message: fmt.Sprintf("contains PII-sensitive pattern %q — verify PII handling compliance", pattern),
			}}
		}
	}
	return nil
}
