package db

import (
	"os"
	"path/filepath"
	"testing"
)

func TestSetUserVersion(t *testing.T) {
	t.Run("sets and reads user_version", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.SetUserVersion(ctx, 42); err != nil {
			t.Fatalf("SetUserVersion: %v", err)
		}

		got, err := d.GetUserVersion(ctx)
		if err != nil {
			t.Fatalf("GetUserVersion: %v", err)
		}

		if got != 42 {
			t.Errorf("user_version = %d, want 42", got)
		}
	})

	t.Run("sets version to zero", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.SetUserVersion(ctx, 5); err != nil {
			t.Fatalf("SetUserVersion to 5: %v", err)
		}
		if err := d.SetUserVersion(ctx, 0); err != nil {
			t.Fatalf("SetUserVersion to 0: %v", err)
		}

		got, err := d.GetUserVersion(ctx)
		if err != nil {
			t.Fatalf("GetUserVersion: %v", err)
		}
		if got != 0 {
			t.Errorf("user_version = %d, want 0", got)
		}
	})

	t.Run("overwrites previous version", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		for _, v := range []int{1, 5, 10, 3} {
			if err := d.SetUserVersion(ctx, v); err != nil {
				t.Fatalf("SetUserVersion(%d): %v", v, err)
			}
		}

		got, err := d.GetUserVersion(ctx)
		if err != nil {
			t.Fatalf("GetUserVersion: %v", err)
		}
		if got != 3 {
			t.Errorf("user_version = %d, want 3 (last set)", got)
		}
	})
}

func TestLoadMigrationsFromDir(t *testing.T) {
	t.Run("loads and sorts migration files", func(t *testing.T) {
		dir := t.TempDir()

		// Create migration files out of order.
		writeFile(t, filepath.Join(dir, "003_third.sql"), "CREATE TABLE c (id INTEGER)")
		writeFile(t, filepath.Join(dir, "001_first.sql"), "CREATE TABLE a (id INTEGER)")
		writeFile(t, filepath.Join(dir, "002_second.sql"), "CREATE TABLE b (id INTEGER)")

		migrations, err := LoadMigrationsFromDir(dir)
		if err != nil {
			t.Fatalf("LoadMigrationsFromDir: %v", err)
		}

		if len(migrations) != 3 {
			t.Fatalf("got %d migrations, want 3", len(migrations))
		}

		// Verify sorting.
		for i, want := range []int{1, 2, 3} {
			if migrations[i].Version != want {
				t.Errorf("migration[%d].Version = %d, want %d", i, migrations[i].Version, want)
			}
		}

		// Verify name parsing.
		if migrations[0].Name != "first" {
			t.Errorf("migration[0].Name = %q, want %q", migrations[0].Name, "first")
		}

		// Verify SQL content.
		if migrations[0].SQL != "CREATE TABLE a (id INTEGER)" {
			t.Errorf("migration[0].SQL = %q", migrations[0].SQL)
		}
	})

	t.Run("returns nil for nonexistent directory", func(t *testing.T) {
		migrations, err := LoadMigrationsFromDir("/nonexistent/path")
		if err != nil {
			t.Fatalf("expected nil error, got: %v", err)
		}
		if migrations != nil {
			t.Errorf("expected nil migrations, got %d", len(migrations))
		}
	})

	t.Run("skips non-sql files", func(t *testing.T) {
		dir := t.TempDir()
		writeFile(t, filepath.Join(dir, "001_real.sql"), "SELECT 1")
		writeFile(t, filepath.Join(dir, "README.md"), "not a migration")
		writeFile(t, filepath.Join(dir, "notes.txt"), "also not a migration")

		migrations, err := LoadMigrationsFromDir(dir)
		if err != nil {
			t.Fatalf("LoadMigrationsFromDir: %v", err)
		}

		if len(migrations) != 1 {
			t.Errorf("got %d migrations, want 1 (only .sql files)", len(migrations))
		}
	})

	t.Run("rejects invalid filename without underscore", func(t *testing.T) {
		dir := t.TempDir()
		writeFile(t, filepath.Join(dir, "nomatch.sql"), "SELECT 1")

		_, err := LoadMigrationsFromDir(dir)
		if err == nil {
			t.Error("expected error for filename without underscore")
		}
	})

	t.Run("rejects non-numeric version prefix", func(t *testing.T) {
		dir := t.TempDir()
		writeFile(t, filepath.Join(dir, "abc_migration.sql"), "SELECT 1")

		_, err := LoadMigrationsFromDir(dir)
		if err == nil {
			t.Error("expected error for non-numeric version")
		}
	})

	t.Run("skips directories", func(t *testing.T) {
		dir := t.TempDir()
		writeFile(t, filepath.Join(dir, "001_real.sql"), "SELECT 1")
		if err := os.Mkdir(filepath.Join(dir, "002_subdir.sql"), 0o755); err != nil {
			t.Fatalf("creating subdir: %v", err)
		}

		migrations, err := LoadMigrationsFromDir(dir)
		if err != nil {
			t.Fatalf("LoadMigrationsFromDir: %v", err)
		}
		if len(migrations) != 1 {
			t.Errorf("got %d migrations, want 1", len(migrations))
		}
	})
}

func TestApplyMigrations(t *testing.T) {
	t.Run("applies all pending migrations", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		migrations := []Migration{
			{Version: 1, Name: "create_a", SQL: "CREATE TABLE a (id INTEGER PRIMARY KEY)"},
			{Version: 2, Name: "create_b", SQL: "CREATE TABLE b (id INTEGER PRIMARY KEY)"},
		}

		result, err := d.ApplyMigrations(ctx, migrations)
		if err != nil {
			t.Fatalf("ApplyMigrations: %v", err)
		}

		if len(result.Applied) != 2 {
			t.Errorf("applied %d, want 2", len(result.Applied))
		}
		if result.CurrentVersion != 2 {
			t.Errorf("version = %d, want 2", result.CurrentVersion)
		}

		// Verify tables exist.
		for _, table := range []string{"a", "b"} {
			_, err := d.CountRows(ctx, table)
			if err != nil {
				t.Errorf("table %s should exist: %v", table, err)
			}
		}

		// Verify user_version was updated.
		v, _ := d.GetUserVersion(ctx)
		if v != 2 {
			t.Errorf("user_version = %d, want 2", v)
		}
	})

	t.Run("skips already-applied migrations", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.SetUserVersion(ctx, 2); err != nil {
			t.Fatalf("SetUserVersion: %v", err)
		}

		migrations := []Migration{
			{Version: 1, Name: "old", SQL: "CREATE TABLE x (id INTEGER)"},
			{Version: 2, Name: "current", SQL: "CREATE TABLE y (id INTEGER)"},
			{Version: 3, Name: "new", SQL: "CREATE TABLE z (id INTEGER)"},
		}

		result, err := d.ApplyMigrations(ctx, migrations)
		if err != nil {
			t.Fatalf("ApplyMigrations: %v", err)
		}

		if len(result.Applied) != 1 {
			t.Errorf("applied %d, want 1 (only v3)", len(result.Applied))
		}
		if result.Applied[0] != 3 {
			t.Errorf("applied version = %d, want 3", result.Applied[0])
		}
	})

	t.Run("no-op when all applied", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.SetUserVersion(ctx, 5); err != nil {
			t.Fatalf("SetUserVersion: %v", err)
		}

		migrations := []Migration{
			{Version: 1, Name: "old1", SQL: "SELECT 1"},
			{Version: 3, Name: "old3", SQL: "SELECT 1"},
		}

		result, err := d.ApplyMigrations(ctx, migrations)
		if err != nil {
			t.Fatalf("ApplyMigrations: %v", err)
		}

		if len(result.Applied) != 0 {
			t.Errorf("applied %d, want 0", len(result.Applied))
		}
		if result.PendingCount != 0 {
			t.Errorf("pending = %d, want 0", result.PendingCount)
		}
	})

	t.Run("returns error for invalid SQL migration", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		migrations := []Migration{
			{Version: 1, Name: "bad", SQL: "THIS IS NOT VALID SQL"},
		}

		_, err := d.ApplyMigrations(ctx, migrations)
		if err == nil {
			t.Error("expected error for invalid SQL")
		}
	})

	t.Run("empty migrations slice is no-op", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		result, err := d.ApplyMigrations(ctx, nil)
		if err != nil {
			t.Fatalf("ApplyMigrations: %v", err)
		}

		if len(result.Applied) != 0 {
			t.Errorf("applied %d, want 0", len(result.Applied))
		}
	})
}

func TestPendingMigrations(t *testing.T) {
	t.Run("returns only pending", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.SetUserVersion(ctx, 2); err != nil {
			t.Fatalf("SetUserVersion: %v", err)
		}

		migrations := []Migration{
			{Version: 1, Name: "v1"},
			{Version: 2, Name: "v2"},
			{Version: 3, Name: "v3"},
			{Version: 4, Name: "v4"},
		}

		pending, err := d.PendingMigrations(ctx, migrations)
		if err != nil {
			t.Fatalf("PendingMigrations: %v", err)
		}

		if len(pending) != 2 {
			t.Fatalf("got %d pending, want 2", len(pending))
		}
		if pending[0].Version != 3 || pending[1].Version != 4 {
			t.Errorf("pending versions = [%d, %d], want [3, 4]", pending[0].Version, pending[1].Version)
		}
	})

	t.Run("returns empty for no pending", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.SetUserVersion(ctx, 10); err != nil {
			t.Fatalf("SetUserVersion: %v", err)
		}

		pending, err := d.PendingMigrations(ctx, []Migration{{Version: 1}, {Version: 5}})
		if err != nil {
			t.Fatalf("PendingMigrations: %v", err)
		}

		if len(pending) != 0 {
			t.Errorf("got %d pending, want 0", len(pending))
		}
	})

	t.Run("returns all when version is 0", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		migrations := []Migration{{Version: 1}, {Version: 2}, {Version: 3}}
		pending, err := d.PendingMigrations(ctx, migrations)
		if err != nil {
			t.Fatalf("PendingMigrations: %v", err)
		}

		if len(pending) != 3 {
			t.Errorf("got %d pending, want 3", len(pending))
		}
	})
}

// writeFile is a test helper that creates a file with the given content.
func writeFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("writeFile(%s): %v", path, err)
	}
}
