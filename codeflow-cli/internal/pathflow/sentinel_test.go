package pathflow

import (
	"os"
	"path/filepath"
	"sort"
	"testing"
)

func TestCreate(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name        string
		sentinelDir string
		sentinelNm  string
		wantErr     bool
	}{
		{
			name:        "creates sentinel file",
			sentinelDir: filepath.Join(t.TempDir(), "sentinels"),
			sentinelNm:  "pf-1",
		},
		{
			name:        "idempotent on existing sentinel",
			sentinelDir: filepath.Join(t.TempDir(), "sentinels"),
			sentinelNm:  "pf-2",
		},
		{
			name:        "empty directory returns error",
			sentinelDir: "",
			sentinelNm:  "pf-1",
			wantErr:     true,
		},
		{
			name:        "empty name returns error",
			sentinelDir: t.TempDir(),
			sentinelNm:  "",
			wantErr:     true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			err := Create(tt.sentinelDir, tt.sentinelNm)
			if tt.wantErr {
				if err == nil {
					t.Fatal("Create() expected error, got nil")
				}
				return
			}
			if err != nil {
				t.Fatalf("Create() unexpected error: %v", err)
			}

			// Verify file exists.
			path := filepath.Join(tt.sentinelDir, sentinelPrefix+tt.sentinelNm)
			if _, err := os.Stat(path); err != nil {
				t.Fatalf("sentinel file not found at %s: %v", path, err)
			}

			// Verify idempotent: calling again should not error.
			if err := Create(tt.sentinelDir, tt.sentinelNm); err != nil {
				t.Fatalf("Create() idempotent call failed: %v", err)
			}
		})
	}
}

func TestExists(t *testing.T) {
	t.Parallel()

	t.Run("returns true for existing sentinel", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		if err := Create(dir, "pf-1"); err != nil {
			t.Fatalf("setup: %v", err)
		}

		if !Exists(dir, "pf-1") {
			t.Error("Exists() = false, want true")
		}
	})

	t.Run("returns false for missing sentinel", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		if Exists(dir, "pf-99") {
			t.Error("Exists() = true, want false")
		}
	})

	t.Run("returns false for missing directory", func(t *testing.T) {
		t.Parallel()

		if Exists("/nonexistent/path", "pf-1") {
			t.Error("Exists() = true for missing dir, want false")
		}
	})

	t.Run("returns false for empty arguments", func(t *testing.T) {
		t.Parallel()

		if Exists("", "pf-1") {
			t.Error("Exists(\"\", \"pf-1\") = true, want false")
		}
		if Exists(t.TempDir(), "") {
			t.Error("Exists(dir, \"\") = true, want false")
		}
	})
}

func TestList(t *testing.T) {
	t.Parallel()

	t.Run("lists sentinel names without prefix", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		for _, name := range []string{"pf-1", "pf-2", "ws-dev"} {
			if err := Create(dir, name); err != nil {
				t.Fatalf("setup: %v", err)
			}
		}

		got, err := List(dir)
		if err != nil {
			t.Fatalf("List() error: %v", err)
		}

		sort.Strings(got)
		want := []string{"pf-1", "pf-2", "ws-dev"}
		sort.Strings(want)

		if len(got) != len(want) {
			t.Fatalf("List() = %v, want %v", got, want)
		}
		for i := range want {
			if got[i] != want[i] {
				t.Errorf("List()[%d] = %q, want %q", i, got[i], want[i])
			}
		}
	})

	t.Run("returns empty slice for missing directory", func(t *testing.T) {
		t.Parallel()

		got, err := List(filepath.Join(t.TempDir(), "nonexistent"))
		if err != nil {
			t.Fatalf("List() error: %v", err)
		}
		if len(got) != 0 {
			t.Errorf("List() = %v, want empty", got)
		}
	})

	t.Run("ignores non-sentinel files", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		// Create a sentinel and a non-sentinel file.
		if err := Create(dir, "pf-1"); err != nil {
			t.Fatalf("setup: %v", err)
		}
		if err := os.WriteFile(filepath.Join(dir, "other-file.txt"), []byte("data"), 0o644); err != nil {
			t.Fatalf("setup: %v", err)
		}

		got, err := List(dir)
		if err != nil {
			t.Fatalf("List() error: %v", err)
		}
		if len(got) != 1 || got[0] != "pf-1" {
			t.Errorf("List() = %v, want [pf-1]", got)
		}
	})

	t.Run("ignores subdirectories", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		if err := os.Mkdir(filepath.Join(dir, sentinelPrefix+"subdir"), 0o755); err != nil {
			t.Fatalf("setup: %v", err)
		}
		if err := Create(dir, "pf-1"); err != nil {
			t.Fatalf("setup: %v", err)
		}

		got, err := List(dir)
		if err != nil {
			t.Fatalf("List() error: %v", err)
		}
		if len(got) != 1 || got[0] != "pf-1" {
			t.Errorf("List() = %v, want [pf-1]", got)
		}
	})

	t.Run("returns error for empty directory argument", func(t *testing.T) {
		t.Parallel()

		_, err := List("")
		if err == nil {
			t.Error("List(\"\") expected error, got nil")
		}
	})
}
