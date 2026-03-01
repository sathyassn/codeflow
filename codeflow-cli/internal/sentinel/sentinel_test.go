package sentinel

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
)

func TestResolveScopeDir(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		scope     Scope
		sessionID string
		wantSuffix string
		wantErr   error
	}{
		{
			name:       "pathflow scope",
			scope:      ScopePathFlow,
			sessionID:  "ses-abc123",
			wantSuffix: filepath.Join(".state", "sentinels", "pathflow", "ses-abc123"),
		},
		{
			name:      "invalid scope",
			scope:     Scope("invalid"),
			sessionID: "ses-abc123",
			wantErr:   ErrInvalidScope,
		},
		{
			name:      "empty session ID",
			scope:     ScopePathFlow,
			sessionID: "",
			wantErr:   ErrEmptySessionID,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			mgr := &Manager{BaseDir: "/project"}
			got, err := mgr.ResolveScopeDir(tc.scope, tc.sessionID)
			if tc.wantErr != nil {
				if err == nil {
					t.Fatalf("expected error %v, got nil", tc.wantErr)
				}
				if !errors.Is(err, tc.wantErr) {
					t.Fatalf("expected error containing %v, got %v", tc.wantErr, err)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if !strings.HasSuffix(got, tc.wantSuffix) {
				t.Errorf("got %q, want suffix %q", got, tc.wantSuffix)
			}
		})
	}
}

func TestCreate(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		sentinel  string
		sessionID string
		wantErr   error
	}{
		{
			name:      "create sentinel",
			sentinel:  "pf-3",
			sessionID: "ses-abc123",
		},
		{
			name:      "create with dot",
			sentinel:  "ws-dev.done",
			sessionID: "ses-abc123",
		},
		{
			name:      "empty name",
			sentinel:  "",
			sessionID: "ses-abc123",
			wantErr:   ErrEmptyName,
		},
		{
			name:      "invalid name with slash",
			sentinel:  "../escape",
			sessionID: "ses-abc123",
			wantErr:   ErrInvalidName,
		},
		{
			name:      "empty session",
			sentinel:  "pf-3",
			sessionID: "",
			wantErr:   ErrEmptySessionID,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			baseDir := t.TempDir()
			mgr := &Manager{BaseDir: baseDir}

			err := mgr.Create(ScopePathFlow, tc.sessionID, tc.sentinel)
			if tc.wantErr != nil {
				if err == nil {
					t.Fatalf("expected error %v, got nil", tc.wantErr)
				}
				if !errors.Is(err, tc.wantErr) {
					t.Fatalf("expected error containing %v, got %v", tc.wantErr, err)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}

			// Verify file exists.
			scopeDir, _ := mgr.ResolveScopeDir(ScopePathFlow, tc.sessionID)
			path := filepath.Join(scopeDir, sentinelPrefix+tc.sentinel)
			info, statErr := os.Stat(path)
			if statErr != nil {
				t.Fatalf("sentinel file not created: %v", statErr)
			}
			if info.Size() != 0 {
				t.Errorf("sentinel file should be empty, got %d bytes", info.Size())
			}
		})
	}
}

func TestCreateIdempotent(t *testing.T) {
	t.Parallel()

	baseDir := t.TempDir()
	mgr := &Manager{BaseDir: baseDir}

	// Create twice — second call should not error.
	if err := mgr.Create(ScopePathFlow, "ses-abc123", "pf-3"); err != nil {
		t.Fatalf("first create: %v", err)
	}
	if err := mgr.Create(ScopePathFlow, "ses-abc123", "pf-3"); err != nil {
		t.Fatalf("second create (idempotent): %v", err)
	}
}

func TestCreateMkdirAll(t *testing.T) {
	t.Parallel()

	baseDir := t.TempDir()
	mgr := &Manager{BaseDir: baseDir}

	// Directory does not exist yet — Create should auto-create it.
	if err := mgr.Create(ScopePathFlow, "ses-newdir", "pf-1"); err != nil {
		t.Fatalf("create with missing parent: %v", err)
	}

	scopeDir, _ := mgr.ResolveScopeDir(ScopePathFlow, "ses-newdir")
	if _, err := os.Stat(scopeDir); err != nil {
		t.Fatalf("scope directory not created: %v", err)
	}
}

func TestCheck(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		sentinel  string
		sessionID string
		setup     bool // true = create the sentinel first
		want      bool
		wantErr   error
	}{
		{
			name:      "existing sentinel",
			sentinel:  "pf-3",
			sessionID: "ses-abc123",
			setup:     true,
			want:      true,
		},
		{
			name:      "non-existent sentinel",
			sentinel:  "pf-99",
			sessionID: "ses-abc123",
			setup:     false,
			want:      false,
		},
		{
			name:      "empty name",
			sentinel:  "",
			sessionID: "ses-abc123",
			wantErr:   ErrEmptyName,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			baseDir := t.TempDir()
			mgr := &Manager{BaseDir: baseDir}

			if tc.setup {
				if err := mgr.Create(ScopePathFlow, tc.sessionID, tc.sentinel); err != nil {
					t.Fatalf("setup create: %v", err)
				}
			}

			got, err := mgr.Check(ScopePathFlow, tc.sessionID, tc.sentinel)
			if tc.wantErr != nil {
				if err == nil {
					t.Fatalf("expected error %v, got nil", tc.wantErr)
				}
				if !errors.Is(err, tc.wantErr) {
					t.Fatalf("expected error containing %v, got %v", tc.wantErr, err)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if got != tc.want {
				t.Errorf("Check() = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestCheckNonExistentDirectory(t *testing.T) {
	t.Parallel()

	baseDir := t.TempDir()
	mgr := &Manager{BaseDir: baseDir}

	// Directory doesn't exist — Check should return false, no error.
	got, err := mgr.Check(ScopePathFlow, "ses-nodir", "pf-1")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got {
		t.Error("Check() = true for non-existent directory, want false")
	}
}

func TestList(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		sessionID string
		sentinels []string
		want      []string
		wantErr   error
	}{
		{
			name:      "multiple sentinels sorted",
			sessionID: "ses-abc123",
			sentinels: []string{"ws-rev", "pf-3", "ws-dev", "pf-1"},
			want:      []string{"pf-1", "pf-3", "ws-dev", "ws-rev"},
		},
		{
			name:      "empty directory",
			sessionID: "ses-empty",
			sentinels: nil,
			want:      nil,
		},
		{
			name:      "non-existent directory",
			sessionID: "ses-nodir",
			sentinels: nil,
			want:      nil,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			baseDir := t.TempDir()
			mgr := &Manager{BaseDir: baseDir}

			// Create sentinels for "empty directory" case — create the dir only.
			if tc.name == "empty directory" {
				scopeDir, _ := mgr.ResolveScopeDir(ScopePathFlow, tc.sessionID)
				if err := os.MkdirAll(scopeDir, 0o755); err != nil {
					t.Fatal(err)
				}
			}

			for _, s := range tc.sentinels {
				if err := mgr.Create(ScopePathFlow, tc.sessionID, s); err != nil {
					t.Fatalf("setup create %q: %v", s, err)
				}
			}

			got, err := mgr.List(ScopePathFlow, tc.sessionID)
			if tc.wantErr != nil {
				if err == nil {
					t.Fatalf("expected error %v, got nil", tc.wantErr)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}

			if !slices.Equal(got, tc.want) {
				t.Errorf("List() = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestListIgnoresDirectories(t *testing.T) {
	t.Parallel()

	baseDir := t.TempDir()
	mgr := &Manager{BaseDir: baseDir}
	sessionID := "ses-abc123"

	if err := mgr.Create(ScopePathFlow, sessionID, "pf-3"); err != nil {
		t.Fatal(err)
	}

	// Create a subdirectory that looks like a sentinel.
	scopeDir, _ := mgr.ResolveScopeDir(ScopePathFlow, sessionID)
	if err := os.MkdirAll(filepath.Join(scopeDir, sentinelPrefix+"subdir"), 0o755); err != nil {
		t.Fatal(err)
	}

	got, err := mgr.List(ScopePathFlow, sessionID)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 || got[0] != "pf-3" {
		t.Errorf("List() = %v, want [pf-3] (should ignore directories)", got)
	}
}

func TestListIgnoresNonPrefixedFiles(t *testing.T) {
	t.Parallel()

	baseDir := t.TempDir()
	mgr := &Manager{BaseDir: baseDir}
	sessionID := "ses-abc123"

	if err := mgr.Create(ScopePathFlow, sessionID, "pf-3"); err != nil {
		t.Fatal(err)
	}

	// Create a non-prefixed file.
	scopeDir, _ := mgr.ResolveScopeDir(ScopePathFlow, sessionID)
	if err := os.WriteFile(filepath.Join(scopeDir, "other-file.txt"), nil, 0o644); err != nil {
		t.Fatal(err)
	}

	got, err := mgr.List(ScopePathFlow, sessionID)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 || got[0] != "pf-3" {
		t.Errorf("List() = %v, want [pf-3] (should ignore non-prefixed files)", got)
	}
}

func TestDelete(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		sentinel  string
		sessionID string
		setup     bool // true = create the sentinel first
		wantErr   error
	}{
		{
			name:      "delete existing",
			sentinel:  "pf-3",
			sessionID: "ses-abc123",
			setup:     true,
		},
		{
			name:      "delete non-existent (idempotent)",
			sentinel:  "pf-99",
			sessionID: "ses-abc123",
			setup:     false,
		},
		{
			name:      "empty name",
			sentinel:  "",
			sessionID: "ses-abc123",
			wantErr:   ErrEmptyName,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			baseDir := t.TempDir()
			mgr := &Manager{BaseDir: baseDir}

			if tc.setup {
				if err := mgr.Create(ScopePathFlow, tc.sessionID, tc.sentinel); err != nil {
					t.Fatalf("setup create: %v", err)
				}
			} else if tc.sentinel != "" {
				// Ensure directory exists for idempotent delete.
				scopeDir, _ := mgr.ResolveScopeDir(ScopePathFlow, tc.sessionID)
				if err := os.MkdirAll(scopeDir, 0o755); err != nil {
					t.Fatal(err)
				}
			}

			err := mgr.Delete(ScopePathFlow, tc.sessionID, tc.sentinel)
			if tc.wantErr != nil {
				if err == nil {
					t.Fatalf("expected error %v, got nil", tc.wantErr)
				}
				if !errors.Is(err, tc.wantErr) {
					t.Fatalf("expected error containing %v, got %v", tc.wantErr, err)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}

			// If we set up a sentinel, verify it's gone.
			if tc.setup {
				exists, checkErr := mgr.Check(ScopePathFlow, tc.sessionID, tc.sentinel)
				if checkErr != nil {
					t.Fatalf("check after delete: %v", checkErr)
				}
				if exists {
					t.Error("sentinel still exists after delete")
				}
			}
		})
	}
}

func TestConcurrentCreate(t *testing.T) {
	t.Parallel()

	baseDir := t.TempDir()
	mgr := &Manager{BaseDir: baseDir}
	sessionID := "ses-concurrent"

	const goroutines = 10
	var wg sync.WaitGroup
	errs := make([]error, goroutines)

	for i := range goroutines {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			errs[idx] = mgr.Create(ScopePathFlow, sessionID, "pf-3")
		}(i)
	}
	wg.Wait()

	for i, err := range errs {
		if err != nil {
			t.Errorf("goroutine %d: %v", i, err)
		}
	}

	// Sentinel should exist.
	exists, err := mgr.Check(ScopePathFlow, sessionID, "pf-3")
	if err != nil {
		t.Fatalf("check after concurrent create: %v", err)
	}
	if !exists {
		t.Error("sentinel does not exist after concurrent creates")
	}
}

func TestValidateName(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		input   string
		wantErr error
	}{
		{"valid simple", "pf-3", nil},
		{"valid with dots", "ws-dev.done", nil},
		{"valid with underscore", "my_sentinel", nil},
		{"valid alphanumeric", "abc123", nil},
		{"empty", "", ErrEmptyName},
		{"path separator", "foo/bar", ErrInvalidName},
		{"backslash", "foo\\bar", ErrInvalidName},
		{"starts with dot", ".hidden", ErrInvalidName},
		{"starts with hyphen", "-invalid", ErrInvalidName},
		{"space", "foo bar", ErrInvalidName},
		{"parent traversal", "../etc", ErrInvalidName},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			err := validateName(tc.input)
			if tc.wantErr != nil {
				if err == nil {
					t.Fatalf("expected error %v, got nil", tc.wantErr)
				}
				if !errors.Is(err, tc.wantErr) {
					t.Fatalf("expected error containing %v, got %v", tc.wantErr, err)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

func TestCRUDRoundTrip(t *testing.T) {
	t.Parallel()

	baseDir := t.TempDir()
	mgr := &Manager{BaseDir: baseDir}
	sessionID := "ses-roundtrip"

	// Create.
	if err := mgr.Create(ScopePathFlow, sessionID, "pf-3"); err != nil {
		t.Fatalf("create: %v", err)
	}

	// Check — exists.
	exists, err := mgr.Check(ScopePathFlow, sessionID, "pf-3")
	if err != nil {
		t.Fatalf("check: %v", err)
	}
	if !exists {
		t.Fatal("check: sentinel should exist")
	}

	// List — should include pf-3.
	names, err := mgr.List(ScopePathFlow, sessionID)
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	if len(names) != 1 || names[0] != "pf-3" {
		t.Fatalf("list = %v, want [pf-3]", names)
	}

	// Delete.
	if err := mgr.Delete(ScopePathFlow, sessionID, "pf-3"); err != nil {
		t.Fatalf("delete: %v", err)
	}

	// Check — gone.
	exists, err = mgr.Check(ScopePathFlow, sessionID, "pf-3")
	if err != nil {
		t.Fatalf("check after delete: %v", err)
	}
	if exists {
		t.Fatal("check: sentinel should not exist after delete")
	}

	// List — empty.
	names, err = mgr.List(ScopePathFlow, sessionID)
	if err != nil {
		t.Fatalf("list after delete: %v", err)
	}
	if len(names) != 0 {
		t.Fatalf("list after delete = %v, want empty", names)
	}
}

func TestInvalidScope(t *testing.T) {
	t.Parallel()

	baseDir := t.TempDir()
	mgr := &Manager{BaseDir: baseDir}

	if err := mgr.Create(Scope("skills"), "ses-abc", "test"); err == nil {
		t.Fatal("Create with invalid scope should error")
	}

	if _, err := mgr.Check(Scope("skills"), "ses-abc", "test"); err == nil {
		t.Fatal("Check with invalid scope should error")
	}

	if _, err := mgr.List(Scope("skills"), "ses-abc"); err == nil {
		t.Fatal("List with invalid scope should error")
	}

	if err := mgr.Delete(Scope("skills"), "ses-abc", "test"); err == nil {
		t.Fatal("Delete with invalid scope should error")
	}
}

