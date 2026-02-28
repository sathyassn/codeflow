package validate

import (
	"errors"
	"testing"
)

func TestParseFrontmatter(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		content   string
		wantErr   bool
		wantField string // Expected field in parsed data (empty to skip check).
		wantValue string // Expected value for wantField.
	}{
		{
			name:      "valid frontmatter",
			content:   "---\ntitle: Hello\nstatus: draft\n---\n\n# Body\n",
			wantField: "title",
			wantValue: "Hello",
		},
		{
			name:    "missing opening delimiter",
			content: "title: Hello\n---\n",
			wantErr: true,
		},
		{
			name:    "missing closing delimiter",
			content: "---\ntitle: Hello\n",
			wantErr: true,
		},
		{
			name:    "only opening delimiter no newline",
			content: "---",
			wantErr: true,
		},
		{
			name:    "empty YAML content",
			content: "---\n---\n",
			wantErr: true,
		},
		{
			name:    "invalid YAML syntax",
			content: "---\n: : invalid\n\t\tbad\n---\n",
			wantErr: true,
		},
		{
			name:      "with BOM prefix",
			content:   "\xEF\xBB\xBF---\ntitle: WithBOM\n---\n# Body\n",
			wantField: "title",
			wantValue: "WithBOM",
		},
		{
			name:      "trailing whitespace on delimiter",
			content:   "---\ntitle: Test\n---   \n# Body\n",
			wantField: "title",
			wantValue: "Test",
		},
		{
			name:      "body content preserved",
			content:   "---\nkey: val\n---\nBody line 1\nBody line 2\n",
			wantField: "key",
			wantValue: "val",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			data, _, err := ParseFrontmatter([]byte(tt.content))
			if tt.wantErr {
				if err == nil {
					t.Errorf("ParseFrontmatter() error = nil, want error")
				}
				if !errors.Is(err, ErrInvalidFrontmatter) {
					t.Errorf("ParseFrontmatter() error = %v, want ErrInvalidFrontmatter", err)
				}
				return
			}
			if err != nil {
				t.Fatalf("ParseFrontmatter() unexpected error: %v", err)
			}
			if tt.wantField != "" {
				got, ok := data[tt.wantField]
				if !ok {
					t.Errorf("ParseFrontmatter() missing field %q", tt.wantField)
				} else if gotStr, isStr := got.(string); isStr && gotStr != tt.wantValue {
					t.Errorf("ParseFrontmatter()[%q] = %q, want %q", tt.wantField, gotStr, tt.wantValue)
				}
			}
		})
	}
}

func TestParseFrontmatter_BodyContent(t *testing.T) {
	t.Parallel()

	content := "---\nkey: val\n---\nLine 1\nLine 2\n"
	_, body, err := ParseFrontmatter([]byte(content))
	if err != nil {
		t.Fatalf("ParseFrontmatter() unexpected error: %v", err)
	}
	if string(body) != "Line 1\nLine 2\n" {
		t.Errorf("ParseFrontmatter() body = %q, want %q", string(body), "Line 1\nLine 2\n")
	}
}

func TestWithProtectedBranches(t *testing.T) {
	t.Parallel()

	custom := []string{"staging", "release"}
	o := buildOptions([]Option{WithProtectedBranches(custom)})

	if len(o.protectedBranches) != 2 {
		t.Errorf("WithProtectedBranches() got %d branches, want 2", len(o.protectedBranches))
	}
	if o.protectedBranches[0] != "staging" {
		t.Errorf("WithProtectedBranches()[0] = %q, want %q", o.protectedBranches[0], "staging")
	}
}

func TestValidationError_Error(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		err  ValidationError
		want string
	}{
		{
			name: "with field",
			err:  ValidationError{Field: "status", Message: "invalid value"},
			want: "status: invalid value",
		},
		{
			name: "without field",
			err:  ValidationError{Message: "general error"},
			want: "general error",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := tt.err.Error(); got != tt.want {
				t.Errorf("ValidationError.Error() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestValidationWarning_String(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		warn ValidationWarning
		want string
	}{
		{
			name: "with field",
			warn: ValidationWarning{Field: "file_scope", Message: "contains PII"},
			want: "file_scope: contains PII",
		},
		{
			name: "without field",
			warn: ValidationWarning{Message: "general warning"},
			want: "general warning",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := tt.warn.String(); got != tt.want {
				t.Errorf("ValidationWarning.String() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestGetStringField(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		data map[string]any
		key  string
		want string
	}{
		{"string value", map[string]any{"k": "v"}, "k", "v"},
		{"missing key", map[string]any{}, "k", ""},
		{"nil value", map[string]any{"k": nil}, "k", ""},
		{"null string", map[string]any{"k": "null"}, "k", ""},
		{"tilde string", map[string]any{"k": "~"}, "k", ""},
		{"bool true", map[string]any{"k": true}, "k", "true"},
		{"bool false", map[string]any{"k": false}, "k", "false"},
		{"int value", map[string]any{"k": 42}, "k", "42"},
		{"float value", map[string]any{"k": 3.14}, "k", "3.14"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := getStringField(tt.data, tt.key); got != tt.want {
				t.Errorf("getStringField() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestIsFieldEmpty(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		data map[string]any
		key  string
		want bool
	}{
		{"missing key", map[string]any{}, "k", true},
		{"nil value", map[string]any{"k": nil}, "k", true},
		{"empty string", map[string]any{"k": ""}, "k", true},
		{"null string", map[string]any{"k": "null"}, "k", true},
		{"tilde string", map[string]any{"k": "~"}, "k", true},
		{"empty slice", map[string]any{"k": []any{}}, "k", true},
		{"non-empty string", map[string]any{"k": "hello"}, "k", false},
		{"non-empty slice", map[string]any{"k": []any{"a"}}, "k", false},
		{"bool false", map[string]any{"k": false}, "k", false},
		{"int zero", map[string]any{"k": 0}, "k", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := isFieldEmpty(tt.data, tt.key); got != tt.want {
				t.Errorf("isFieldEmpty() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestGetBoolField(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		data    map[string]any
		key     string
		wantVal bool
		wantOK  bool
	}{
		{"bool true", map[string]any{"k": true}, "k", true, true},
		{"bool false", map[string]any{"k": false}, "k", false, true},
		{"string true", map[string]any{"k": "true"}, "k", true, true},
		{"string false", map[string]any{"k": "false"}, "k", false, true},
		{"string yes", map[string]any{"k": "yes"}, "k", false, false},
		{"missing key", map[string]any{}, "k", false, false},
		{"nil value", map[string]any{"k": nil}, "k", false, false},
		{"int value", map[string]any{"k": 1}, "k", false, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			gotVal, gotOK := getBoolField(tt.data, tt.key)
			if gotVal != tt.wantVal || gotOK != tt.wantOK {
				t.Errorf("getBoolField() = (%v, %v), want (%v, %v)", gotVal, gotOK, tt.wantVal, tt.wantOK)
			}
		})
	}
}
