package db

import (
	"path/filepath"
	"testing"
)

func TestCheckHealth(t *testing.T) {
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

func TestNewDB(t *testing.T) {
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
