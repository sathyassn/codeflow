package idgen

import (
	"regexp"
	"testing"
)

// ulidPattern matches a 26-character lowercase Crockford base32 ULID.
var ulidPattern = regexp.MustCompile(`^[0-9a-z]{26}$`)

func TestNewULID_Format(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
	}{
		{name: "generates valid ULID"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			id := NewULID()
			if len(id) != 26 {
				t.Errorf("NewULID() length = %d, want 26; got %q", len(id), id)
			}
			if !ulidPattern.MatchString(id) {
				t.Errorf("NewULID() = %q, does not match Crockford base32 pattern", id)
			}
		})
	}
}

func TestNewULID_Uniqueness(t *testing.T) {
	t.Parallel()

	const count = 100
	seen := make(map[string]bool, count)
	for range count {
		id := NewULID()
		if seen[id] {
			t.Fatalf("NewULID() produced duplicate: %q", id)
		}
		seen[id] = true
	}
}

func TestNewPrefixedULID(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		prefix     string
		wantPrefix string
		wantLen    int
	}{
		{
			name:       "task prefix",
			prefix:     "task",
			wantPrefix: "task-",
			wantLen:    31, // "task-" (5) + 26 = 31
		},
		{
			name:       "epic prefix",
			prefix:     "epic",
			wantPrefix: "epic-",
			wantLen:    31,
		},
		{
			name:       "ses prefix",
			prefix:     "ses",
			wantPrefix: "ses-",
			wantLen:    30, // "ses-" (4) + 26 = 30
		},
		{
			name:       "empty prefix returns bare ULID",
			prefix:     "",
			wantPrefix: "",
			wantLen:    26,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := NewPrefixedULID(tt.prefix)
			if len(got) != tt.wantLen {
				t.Errorf("NewPrefixedULID(%q) length = %d, want %d; got %q",
					tt.prefix, len(got), tt.wantLen, got)
			}
			if tt.wantPrefix != "" {
				if got[:len(tt.wantPrefix)] != tt.wantPrefix {
					t.Errorf("NewPrefixedULID(%q) = %q, does not start with %q",
						tt.prefix, got, tt.wantPrefix)
				}
				// The ULID portion (after prefix) should match the pattern.
				ulidPart := got[len(tt.wantPrefix):]
				if !ulidPattern.MatchString(ulidPart) {
					t.Errorf("ULID portion %q does not match Crockford base32 pattern", ulidPart)
				}
			} else {
				// Bare ULID.
				if !ulidPattern.MatchString(got) {
					t.Errorf("NewPrefixedULID(%q) = %q, does not match ULID pattern", tt.prefix, got)
				}
			}
		})
	}
}
