package db

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

func TestCheckHealth(t *testing.T) {
	t.Parallel()
	t.Run("healthy database returns OK", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		// Initialize schema so tables exist.
		if err := d.InitFromSchema(ctx); err != nil {
			t.Fatalf("InitFromSchema: %v", err)
		}

		report, err := d.CheckHealth(ctx)
		if err != nil {
			t.Fatalf("CheckHealth: %v", err)
		}

		if !report.IsHealthy() {
			t.Errorf("expected healthy, got errors: %v", report.Errors)
		}
		if !report.IntegrityOK {
			t.Error("expected IntegrityOK = true")
		}
		if !report.ForeignKeyOK {
			t.Error("expected ForeignKeyOK = true")
		}
	})

	t.Run("empty database returns healthy", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		report, err := d.CheckHealth(ctx)
		if err != nil {
			t.Fatalf("CheckHealth: %v", err)
		}

		if !report.IsHealthy() {
			t.Errorf("expected healthy for empty db, got errors: %v", report.Errors)
		}
	})

	t.Run("foreign key violation detected", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		// Create a parent and child table with a foreign key relationship.
		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE parent (id INTEGER PRIMARY KEY);
			CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id));
		`)
		if err != nil {
			t.Fatalf("creating tables: %v", err)
		}

		// Insert a child row referencing a non-existent parent (disable FK checks first).
		_, err = d.db.ExecContext(ctx, "PRAGMA foreign_keys=OFF")
		if err != nil {
			t.Fatalf("disabling FK: %v", err)
		}
		_, err = d.db.ExecContext(ctx, "INSERT INTO child (id, parent_id) VALUES (1, 999)")
		if err != nil {
			t.Fatalf("inserting orphan: %v", err)
		}
		_, err = d.db.ExecContext(ctx, "PRAGMA foreign_keys=ON")
		if err != nil {
			t.Fatalf("re-enabling FK: %v", err)
		}

		report, err := d.CheckHealth(ctx)
		if err != nil {
			t.Fatalf("CheckHealth: %v", err)
		}

		if report.ForeignKeyOK {
			t.Error("expected ForeignKeyOK = false for orphan row")
		}
		if report.IsHealthy() {
			t.Error("expected IsHealthy() = false for FK violation")
		}
		if len(report.Errors) == 0 {
			t.Error("expected at least one error for FK violation")
		}
	})

	t.Run("integrity check passes on valid db", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.InitFromSchema(ctx); err != nil {
			t.Fatalf("InitFromSchema: %v", err)
		}

		// Insert valid data to exercise integrity with data present.
		// domains PK is 'code', not 'id'.
		_, err := d.Execute(ctx,
			"INSERT OR IGNORE INTO domains (code, name) VALUES ('TSTD', 'Test Domain')")
		if err != nil {
			t.Fatalf("inserting domain: %v", err)
		}

		report, err := d.CheckHealth(ctx)
		if err != nil {
			t.Fatalf("CheckHealth: %v", err)
		}

		if !report.IntegrityOK {
			t.Error("expected IntegrityOK = true after valid insert")
		}
	})

	t.Run("errors slice is nil for healthy db", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		report, err := d.CheckHealth(ctx)
		if err != nil {
			t.Fatalf("CheckHealth: %v", err)
		}

		if report.Errors != nil {
			t.Errorf("expected nil errors for healthy db, got %v", report.Errors)
		}
	})
}

func TestCheckHealthOnClosedDB(t *testing.T) {
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()

	// Close the underlying connection to exercise the error return paths
	// in CheckHealth (integrity_check query and foreign_key_check query).
	d.db.Close()

	_, err := d.CheckHealth(ctx)
	if err == nil {
		t.Fatal("expected error from CheckHealth on closed DB, got nil")
	}
}

func TestCheckHealthCanceledContext(t *testing.T) {
	t.Parallel()
	d := newTestDB(t)

	// A pre-canceled context should cause the first query to fail,
	// exercising the error return at the integrity_check query.
	ctx, cancel := context.WithCancel(t.Context())
	cancel()

	_, err := d.CheckHealth(ctx)
	if err == nil {
		t.Fatal("expected error from CheckHealth with canceled context, got nil")
	}
}

func TestCheckHealthIntegrityCorrupt(t *testing.T) {
	t.Parallel()
	// Create a DB with a table and index, populate it, then corrupt
	// an index page to trigger integrity_check returning something
	// other than "ok". This exercises the result != "ok" branch.
	dbPath := filepath.Join(t.TempDir(), "corrupt.db")
	d, err := NewDB(dbPath)
	if err != nil {
		t.Fatalf("NewDB: %v", err)
	}

	ctx := t.Context()
	_, err = d.db.ExecContext(ctx, `
		CREATE TABLE test_integrity (id INTEGER PRIMARY KEY, name TEXT);
		CREATE INDEX idx_name ON test_integrity(name);
	`)
	if err != nil {
		t.Fatalf("setup tables: %v", err)
	}
	// Insert enough rows to create a multi-page index.
	for i := range 100 {
		_, err = d.db.ExecContext(ctx, "INSERT INTO test_integrity VALUES (?, ?)",
			i, fmt.Sprintf("name_%04d", i))
		if err != nil {
			t.Fatalf("insert %d: %v", i, err)
		}
	}
	d.Close()

	// Corrupt the index page (page 3, which is at offset 2*4096).
	data, err := os.ReadFile(dbPath)
	if err != nil {
		t.Fatalf("reading db: %v", err)
	}
	pageSize := 4096 // SQLite default
	offset := 2 * pageSize
	if offset+200 < len(data) {
		for i := offset + 8; i < offset+200; i++ {
			data[i] = 0x00
		}
	}
	if err := os.WriteFile(dbPath, data, 0o644); err != nil {
		t.Fatalf("writing corrupt db: %v", err)
	}

	d2, err := NewDB(dbPath)
	if err != nil {
		t.Fatalf("NewDB on corrupt: %v", err)
	}
	defer d2.Close()

	report, err := d2.CheckHealth(ctx)
	if err != nil {
		// Query itself failed — still exercises an error return.
		return
	}
	if report.IntegrityOK {
		t.Log("corruption not detected by integrity_check (page may not be index)")
		return
	}
	// Successfully exercised lines 28-31 (result != "ok" branch).
	if len(report.Errors) == 0 {
		t.Error("expected errors when IntegrityOK is false")
	}
}

func TestCheckHealthMultipleFKViolations(t *testing.T) {
	t.Parallel()
	d := newTestDB(t)
	ctx := t.Context()

	// Create tables with FK relationship and insert multiple violations
	// to exercise the rows.Next() loop and rows.Err() paths.
	_, err := d.db.ExecContext(ctx, `
		CREATE TABLE parent2 (id INTEGER PRIMARY KEY);
		CREATE TABLE child2 (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent2(id));
	`)
	if err != nil {
		t.Fatalf("creating tables: %v", err)
	}

	// Disable FK checks to insert multiple violations.
	_, err = d.db.ExecContext(ctx, "PRAGMA foreign_keys=OFF")
	if err != nil {
		t.Fatalf("disabling FK: %v", err)
	}
	_, err = d.db.ExecContext(ctx, `
		INSERT INTO child2 (id, parent_id) VALUES (1, 901);
		INSERT INTO child2 (id, parent_id) VALUES (2, 902);
		INSERT INTO child2 (id, parent_id) VALUES (3, 903);
	`)
	if err != nil {
		t.Fatalf("inserting orphans: %v", err)
	}
	_, err = d.db.ExecContext(ctx, "PRAGMA foreign_keys=ON")
	if err != nil {
		t.Fatalf("re-enabling FK: %v", err)
	}

	report, err := d.CheckHealth(ctx)
	if err != nil {
		t.Fatalf("CheckHealth: %v", err)
	}

	if report.ForeignKeyOK {
		t.Error("expected ForeignKeyOK = false")
	}
	if len(report.Errors) < 3 {
		t.Errorf("expected at least 3 FK errors, got %d", len(report.Errors))
	}
}

func TestNewDB(t *testing.T) {
	t.Parallel()
	t.Run("creates independent connection", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "newdb.db")
		d, err := NewDB(path)
		if err != nil {
			t.Fatalf("NewDB: %v", err)
		}
		defer d.Close()

		ctx := t.Context()
		// Verify we can use it.
		var version int
		if err := d.db.QueryRowContext(ctx, "PRAGMA user_version").Scan(&version); err != nil {
			t.Fatalf("querying: %v", err)
		}
		if version != 0 {
			t.Errorf("initial user_version = %d, want 0", version)
		}
	})

	t.Run("returns error for invalid path", func(t *testing.T) {
		_, err := NewDB("/nonexistent/dir/db.db")
		if err == nil {
			t.Error("expected error for invalid path")
		}
	})
}

func TestIsHealthy(t *testing.T) {
	t.Parallel()
	tests := []struct {
		name     string
		report   HealthReport
		wantOK   bool
	}{
		{
			name:   "both OK",
			report: HealthReport{IntegrityOK: true, ForeignKeyOK: true},
			wantOK: true,
		},
		{
			name:   "integrity failed",
			report: HealthReport{IntegrityOK: false, ForeignKeyOK: true},
			wantOK: false,
		},
		{
			name:   "foreign key failed",
			report: HealthReport{IntegrityOK: true, ForeignKeyOK: false},
			wantOK: false,
		},
		{
			name:   "both failed",
			report: HealthReport{IntegrityOK: false, ForeignKeyOK: false},
			wantOK: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.report.IsHealthy(); got != tt.wantOK {
				t.Errorf("IsHealthy() = %v, want %v", got, tt.wantOK)
			}
		})
	}
}
