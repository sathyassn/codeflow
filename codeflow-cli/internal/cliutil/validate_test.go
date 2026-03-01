package cliutil

import (
	"errors"
	"testing"
)

func TestValidateULID_Valid(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		id   string
	}{
		{name: "all zeros", id: "00000000000000000000000000"},
		{name: "all nines", id: "99999999999999999999999999"},
		{name: "mixed alphanumeric", id: "01jm8g5a9k3e7n4b2c6d0f9h0a"},
		{name: "realistic ULID", id: "01jm0a1b2c3d4e5f6g7h8j9k0m"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if err := ValidateULID(tt.id); err != nil {
				t.Errorf("ValidateULID(%q) unexpected error: %v", tt.id, err)
			}
		})
	}
}

func TestValidateULID_Invalid(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		id   string
	}{
		{name: "too short", id: "01jm8g5a9k3e7n4b2c6d0f1"},
		{name: "too long", id: "01jm8g5a9k3e7n4b2c6d0f1hx"},
		{name: "empty string", id: ""},
		{name: "contains uppercase", id: "01JM8G5A9K3E7N4B2C6D0F1H"},
		{name: "contains excluded i", id: "01jm8g5a9k3e7n4b2c6d0fi1"},
		{name: "contains excluded l", id: "01jm8g5a9k3e7n4b2c6d0fl1"},
		{name: "contains excluded o", id: "01jm8g5a9k3e7n4b2c6d0fo1"},
		{name: "contains excluded u", id: "01jm8g5a9k3e7n4b2c6d0fu1"},
		{name: "contains space", id: "01jm8g5a9k 3e7n4b2c6d0f1"},
		{name: "contains hyphen", id: "01jm8g5a9k-3e7n4b2c6d0f1"},
		{name: "prefixed ULID", id: "ses-01jm8g5a9k3e7n4b2c6d0f"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			err := ValidateULID(tt.id)
			if err == nil {
				t.Errorf("ValidateULID(%q) expected error, got nil", tt.id)
			}
			if !errors.Is(err, ErrInvalidULID) {
				t.Errorf("ValidateULID(%q) error = %v, want ErrInvalidULID", tt.id, err)
			}
		})
	}
}

func TestValidateFilePath_Valid(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		path string
	}{
		{name: "simple relative", path: "file.txt"},
		{name: "nested relative", path: "dir/subdir/file.go"},
		{name: "absolute path", path: "/usr/local/bin/codeflow"},
		{name: "dot prefix", path: ".codeflow/config/file.json"},
		{name: "single dot segment", path: "./relative/path"},
		{name: "hidden file", path: ".gitignore"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if err := ValidateFilePath(tt.path); err != nil {
				t.Errorf("ValidateFilePath(%q) unexpected error: %v", tt.path, err)
			}
		})
	}
}

func TestValidateFilePath_Invalid(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		path string
	}{
		{name: "empty path", path: ""},
		{name: "null byte", path: "file\x00.txt"},
		{name: "traversal at start", path: "../etc/passwd"},
		{name: "traversal in middle", path: "dir/../../../etc/passwd"},
		{name: "traversal at end", path: "dir/.."},
		{name: "double traversal", path: "../../secret"},
		{name: "null byte in middle", path: "dir/sub\x00dir/file"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			err := ValidateFilePath(tt.path)
			if err == nil {
				t.Errorf("ValidateFilePath(%q) expected error, got nil", tt.path)
			}
			if !errors.Is(err, ErrInvalidFilePath) {
				t.Errorf("ValidateFilePath(%q) error = %v, want ErrInvalidFilePath", tt.path, err)
			}
		})
	}
}

func TestValidateFilePath_EdgeCases(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		path    string
		wantErr bool
	}{
		{name: "dots in filename", path: "file..name.txt", wantErr: false},
		{name: "triple dots", path: "dir/.../file", wantErr: false},
		{name: "dot dot in name segment", path: "dir/..hidden/file", wantErr: false},
		{name: "standalone double dot", path: "..", wantErr: true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			err := ValidateFilePath(tt.path)
			if (err != nil) != tt.wantErr {
				t.Errorf("ValidateFilePath(%q) error = %v, wantErr = %v", tt.path, err, tt.wantErr)
			}
		})
	}
}
