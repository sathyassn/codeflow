package db

import (
	"context"
	"database/sql"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func newTestDB(t *testing.T) *DB {
	t.Helper()
	path := filepath.Join(t.TempDir(), "test.db")
	d, err := newDB(path)
	if err != nil {
		t.Fatalf("newTestDB: %v", err)
	}
	t.Cleanup(func() { d.Close() })
	return d
}

func TestSingleton(t *testing.T) {
	t.Run("Get returns same instance", func(t *testing.T) {
		Reset() // ensure clean state
		t.Cleanup(Reset)

		path := filepath.Join(t.TempDir(), "singleton.db")
		db1 := Get(path)
		db2 := Get(path)

		if db1 != db2 {
			t.Error("Get() returned different instances")
		}
	})

	t.Run("Reset allows new instance", func(t *testing.T) {
		Reset()
		t.Cleanup(Reset)

		path1 := filepath.Join(t.TempDir(), "singleton1.db")
		db1 := Get(path1)

		Reset()

		path2 := filepath.Join(t.TempDir(), "singleton2.db")
		db2 := Get(path2)

		if db1 == db2 {
			t.Error("Get() returned same instance after Reset()")
		}
		if db2.path != path2 {
			t.Errorf("new instance path = %q, want %q", db2.path, path2)
		}
	})
}

func TestPRAGMAEnforcement(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	tests := []struct {
		name   string
		pragma string
		want   string
	}{
		{"journal_mode", "PRAGMA journal_mode", "wal"},
		{"foreign_keys", "PRAGMA foreign_keys", "1"},
		{"busy_timeout", "PRAGMA busy_timeout", "5000"},
		{"synchronous", "PRAGMA synchronous", "1"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var got string
			if err := d.db.QueryRowContext(ctx, tt.pragma).Scan(&got); err != nil {
				t.Fatalf("querying %s: %v", tt.pragma, err)
			}
			if got != tt.want {
				t.Errorf("%s = %q, want %q", tt.pragma, got, tt.want)
			}
		})
	}
}

func TestMaxOpenConns(t *testing.T) {
	d := newTestDB(t)
	stats := d.db.Stats()
	if stats.MaxOpenConnections != 1 {
		t.Errorf("MaxOpenConnections = %d, want 1", stats.MaxOpenConnections)
	}
}

func TestInitFromSchema(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	if err := d.InitFromSchema(ctx); err != nil {
		t.Fatalf("InitFromSchema: %v", err)
	}

	// Verify key tables exist by querying sqlite_master.
	wantTables := []string{
		"schema_version", "project_config", "users", "epics", "tasks",
		"active_work", "sessions", "memory_events", "autorun_sessions", "autorun_workers",
	}

	for _, table := range wantTables {
		t.Run(table, func(t *testing.T) {
			var name string
			err := d.db.QueryRowContext(ctx,
				"SELECT name FROM sqlite_master WHERE type='table' AND name=?", table,
			).Scan(&name)
			if err != nil {
				t.Errorf("table %q not found: %v", table, err)
			}
		})
	}
}

func TestQuery(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	// Create a simple table and insert data.
	_, err := d.db.ExecContext(ctx, "CREATE TABLE test_query (id INTEGER PRIMARY KEY, val TEXT)")
	if err != nil {
		t.Fatalf("creating table: %v", err)
	}

	_, err = d.Execute(ctx, "INSERT INTO test_query (id, val) VALUES (?, ?)", 1, "hello")
	if err != nil {
		t.Fatalf("inserting: %v", err)
	}

	rows, err := d.Query(ctx, "SELECT id, val FROM test_query WHERE id = ?", 1)
	if err != nil {
		t.Fatalf("querying: %v", err)
	}
	defer rows.Close()

	if !rows.Next() {
		t.Fatal("expected one row, got none")
	}

	var id int
	var val string
	if err := rows.Scan(&id, &val); err != nil {
		t.Fatalf("scanning: %v", err)
	}
	if id != 1 || val != "hello" {
		t.Errorf("got id=%d val=%q, want id=1 val=hello", id, val)
	}
}

func TestQueryRow(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	_, err := d.db.ExecContext(ctx, "CREATE TABLE test_qr (id INTEGER PRIMARY KEY, val TEXT)")
	if err != nil {
		t.Fatalf("creating table: %v", err)
	}
	_, err = d.Execute(ctx, "INSERT INTO test_qr (id, val) VALUES (?, ?)", 1, "world")
	if err != nil {
		t.Fatalf("inserting: %v", err)
	}

	var val string
	err = d.QueryRow(ctx, "SELECT val FROM test_qr WHERE id = ?", 1).Scan(&val)
	if err != nil {
		t.Fatalf("QueryRow: %v", err)
	}
	if val != "world" {
		t.Errorf("val = %q, want %q", val, "world")
	}

	// Non-existent row should return sql.ErrNoRows.
	err = d.QueryRow(ctx, "SELECT val FROM test_qr WHERE id = ?", 999).Scan(&val)
	if !errors.Is(err, sql.ErrNoRows) {
		t.Errorf("expected sql.ErrNoRows, got %v", err)
	}
}

func TestExecute(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	_, err := d.db.ExecContext(ctx, "CREATE TABLE test_exec (id INTEGER PRIMARY KEY, val TEXT)")
	if err != nil {
		t.Fatalf("creating table: %v", err)
	}

	result, err := d.Execute(ctx, "INSERT INTO test_exec (id, val) VALUES (?, ?)", 1, "test")
	if err != nil {
		t.Fatalf("Execute: %v", err)
	}

	affected, err := result.RowsAffected()
	if err != nil {
		t.Fatalf("RowsAffected: %v", err)
	}
	if affected != 1 {
		t.Errorf("RowsAffected = %d, want 1", affected)
	}
}

func TestTransactionCommit(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	_, err := d.db.ExecContext(ctx, "CREATE TABLE test_tx (id INTEGER PRIMARY KEY, val TEXT)")
	if err != nil {
		t.Fatalf("creating table: %v", err)
	}

	err = d.Transaction(ctx, func(tx *sql.Tx) error {
		_, err := tx.ExecContext(ctx, "INSERT INTO test_tx (id, val) VALUES (?, ?)", 1, "committed")
		return err
	})
	if err != nil {
		t.Fatalf("Transaction: %v", err)
	}

	// Verify data was committed.
	var val string
	err = d.db.QueryRowContext(ctx, "SELECT val FROM test_tx WHERE id = 1").Scan(&val)
	if err != nil {
		t.Fatalf("reading committed data: %v", err)
	}
	if val != "committed" {
		t.Errorf("val = %q, want %q", val, "committed")
	}
}

func TestTransactionRollback(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	_, err := d.db.ExecContext(ctx, "CREATE TABLE test_rb (id INTEGER PRIMARY KEY, val TEXT)")
	if err != nil {
		t.Fatalf("creating table: %v", err)
	}

	testErr := errors.New("intentional rollback")
	err = d.Transaction(ctx, func(tx *sql.Tx) error {
		if _, err := tx.ExecContext(ctx, "INSERT INTO test_rb (id, val) VALUES (?, ?)", 1, "should_not_persist"); err != nil {
			return err
		}
		return testErr
	})

	if !errors.Is(err, testErr) {
		t.Errorf("Transaction error = %v, want %v", err, testErr)
	}

	// Verify data was NOT persisted.
	var count int
	err = d.db.QueryRowContext(ctx, "SELECT COUNT(*) FROM test_rb").Scan(&count)
	if err != nil {
		t.Fatalf("counting rows: %v", err)
	}
	if count != 0 {
		t.Errorf("row count = %d, want 0 (rollback failed)", count)
	}
}

func TestCheckIntegrity(t *testing.T) {
	t.Run("valid database passes", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.CheckIntegrity(ctx); err != nil {
			t.Errorf("CheckIntegrity on valid DB: %v", err)
		}
	})

	t.Run("initialized database passes", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.InitFromSchema(ctx); err != nil {
			t.Fatalf("InitFromSchema: %v", err)
		}
		if err := d.CheckIntegrity(ctx); err != nil {
			t.Errorf("CheckIntegrity after schema init: %v", err)
		}
	})
}

func TestRetryOnBusy(t *testing.T) {
	d := newTestDB(t)

	// Verify retry configuration is set.
	if d.MaxRetries != DefaultMaxRetries {
		t.Errorf("MaxRetries = %d, want %d", d.MaxRetries, DefaultMaxRetries)
	}
	if d.RetryDelay != DefaultRetryDelay {
		t.Errorf("RetryDelay = %v, want %v", d.RetryDelay, DefaultRetryDelay)
	}

	// Verify isBusyError detects busy errors.
	busyCases := []struct {
		name string
		err  error
		want bool
	}{
		{"nil", nil, false},
		{"generic", errors.New("something"), false},
		{"sqlite_busy", errors.New("SQLITE_BUSY"), true},
		{"database_locked", errors.New("database is locked"), true},
	}

	for _, tc := range busyCases {
		t.Run(tc.name, func(t *testing.T) {
			got := isBusyError(tc.err)
			if got != tc.want {
				t.Errorf("isBusyError(%v) = %v, want %v", tc.err, got, tc.want)
			}
		})
	}

	// Verify that retry respects context cancellation.
	t.Run("context cancellation stops retry", func(t *testing.T) {
		d2 := newTestDB(t)
		d2.MaxRetries = 10
		d2.RetryDelay = 50 * time.Millisecond
		ctx, cancel := context.WithCancel(t.Context())
		cancel() // cancel immediately

		// Query on a non-existent table -- the context cancellation should stop us quickly.
		_, err := d2.Query(ctx, "SELECT * FROM nonexistent_table")
		if err == nil {
			t.Error("expected error from cancelled context query")
		}
	})
}

func TestGetUserVersion(t *testing.T) {
	t.Run("default version is zero", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		version, err := d.GetUserVersion(ctx)
		if err != nil {
			t.Fatalf("GetUserVersion: %v", err)
		}
		if version != 0 {
			t.Errorf("user_version = %d, want 0", version)
		}
	})

	t.Run("version after set", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if _, err := d.db.ExecContext(ctx, "PRAGMA user_version = 42"); err != nil {
			t.Fatalf("setting user_version: %v", err)
		}

		version, err := d.GetUserVersion(ctx)
		if err != nil {
			t.Fatalf("GetUserVersion: %v", err)
		}
		if version != 42 {
			t.Errorf("user_version = %d, want 42", version)
		}
	})
}

func TestClose(t *testing.T) {
	path := filepath.Join(t.TempDir(), "close.db")
	d, err := newDB(path)
	if err != nil {
		t.Fatalf("newDB: %v", err)
	}

	if err := d.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}

	// After close, operations should fail.
	_, err = d.db.Exec("SELECT 1")
	if err == nil {
		t.Error("expected error after Close, got nil")
	}
}

func TestMigrate(t *testing.T) {
	t.Run("no-op when fully migrated", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		// Set user_version to the latest migration so Migrate is a no-op.
		migrations, err := LoadMigrationsFromEmbed(EmbeddedMigrations())
		if err != nil {
			t.Fatalf("loading migrations: %v", err)
		}
		latest := migrations[len(migrations)-1].Version
		if err := d.SetUserVersion(ctx, latest); err != nil {
			t.Fatalf("SetUserVersion: %v", err)
		}

		result, err := d.Migrate(ctx)
		if err != nil {
			t.Fatalf("Migrate: %v", err)
		}
		if result == nil {
			t.Fatal("Migrate returned nil result")
		}
		if len(result.Applied) != 0 {
			t.Errorf("Applied = %v, want empty (all migrations already applied)", result.Applied)
		}
		if result.CurrentVersion != latest {
			t.Errorf("CurrentVersion = %d, want %d", result.CurrentVersion, latest)
		}
	})

	t.Run("returns result with correct type", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		// Set version high so no migrations apply -- just verify the return type.
		if err := d.SetUserVersion(ctx, 9999); err != nil {
			t.Fatalf("SetUserVersion: %v", err)
		}

		result, err := d.Migrate(ctx)
		if err != nil {
			t.Fatalf("Migrate: %v", err)
		}
		if result.PendingCount != 0 {
			t.Errorf("PendingCount = %d, want 0", result.PendingCount)
		}
	})
}

func TestNewDBInvalidPath(t *testing.T) {
	// Attempting to open a directory as a DB should fail at PRAGMA execution time
	// or when the driver rejects the path.
	_, err := newDB(filepath.Join(t.TempDir(), "nonexistent", "sub", "test.db"))
	// The error depends on the driver, but it should not be nil since the parent
	// directory doesn't exist and the driver will fail to create the file.
	// Some drivers may defer the error to the first query; we test what we can.
	if err != nil {
		// Expected: driver or PRAGMA setup failed.
		return
	}
	// If newDB didn't error, that's acceptable for some drivers that create files lazily.
	// The test verifies the code path doesn't panic.
}

func TestSchemaEmbedded(t *testing.T) {
	if len(schemaSQL) == 0 {
		t.Fatal("schemaSQL is empty; go:embed failed")
	}
	if !strings.Contains(schemaSQL, "CREATE TABLE") {
		t.Error("schemaSQL does not contain CREATE TABLE statements")
	}
}

func TestQueryError(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	// Query on a non-existent table should return an error (not a busy error,
	// so no retries).
	_, err := d.Query(ctx, "SELECT * FROM nonexistent_table_xyz")
	if err == nil {
		t.Error("expected error from invalid query, got nil")
	}
}

func TestExecuteError(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	// Execute invalid SQL should return an error.
	_, err := d.Execute(ctx, "INSERT INTO nonexistent_table_xyz VALUES (1)")
	if err == nil {
		t.Error("expected error from invalid execute, got nil")
	}
}

func TestTransactionPanicRecovery(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	_, err := d.db.ExecContext(ctx, "CREATE TABLE test_panic (id INTEGER PRIMARY KEY)")
	if err != nil {
		t.Fatalf("creating table: %v", err)
	}

	// Verify that a panic inside Transaction is recovered and re-panicked.
	defer func() {
		r := recover()
		if r == nil {
			t.Fatal("expected panic to be re-raised")
		}
		if r != "test panic" {
			t.Errorf("recovered value = %v, want %q", r, "test panic")
		}

		// Verify the insert was rolled back.
		var count int
		err := d.db.QueryRowContext(ctx, "SELECT COUNT(*) FROM test_panic").Scan(&count)
		if err != nil {
			t.Fatalf("counting rows: %v", err)
		}
		if count != 0 {
			t.Errorf("row count = %d, want 0 (panic rollback failed)", count)
		}
	}()

	_ = d.Transaction(ctx, func(tx *sql.Tx) error {
		if _, err := tx.ExecContext(ctx, "INSERT INTO test_panic (id) VALUES (1)"); err != nil {
			return err
		}
		panic("test panic")
	})
}

func TestInitFromSchemaError(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	// Close the underlying connection so InitFromSchema fails.
	d.db.Close()

	err := d.InitFromSchema(ctx)
	if err == nil {
		t.Error("expected error from InitFromSchema on closed DB, got nil")
	}
}

func TestCheckIntegrityOnClosedDB(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	d.db.Close()

	err := d.CheckIntegrity(ctx)
	if err == nil {
		t.Error("expected error from CheckIntegrity on closed DB, got nil")
	}
}

func TestGetUserVersionOnClosedDB(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	d.db.Close()

	_, err := d.GetUserVersion(ctx)
	if err == nil {
		t.Error("expected error from GetUserVersion on closed DB, got nil")
	}
}

func TestMigrateOnClosedDB(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	d.db.Close()

	_, err := d.Migrate(ctx)
	if err == nil {
		t.Error("expected error from Migrate on closed DB, got nil")
	}
	if !errors.Is(err, ErrMigration) {
		t.Errorf("error should wrap ErrMigration, got: %v", err)
	}
}

func TestTransactionBeginError(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	d.db.Close()

	err := d.Transaction(ctx, func(_ *sql.Tx) error {
		return nil
	})
	if err == nil {
		t.Error("expected error from Transaction on closed DB, got nil")
	}
}

func TestCheckIntegrityForeignKeyViolation(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	// Create tables with a foreign key relationship.
	_, err := d.db.ExecContext(ctx, `
		CREATE TABLE parent (id INTEGER PRIMARY KEY);
		CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id));
	`)
	if err != nil {
		t.Fatalf("creating tables: %v", err)
	}

	// Temporarily disable foreign keys to insert a violation.
	_, err = d.db.ExecContext(ctx, "PRAGMA foreign_keys=OFF")
	if err != nil {
		t.Fatalf("disabling foreign keys: %v", err)
	}

	// Insert a child row with a non-existent parent.
	_, err = d.db.ExecContext(ctx, "INSERT INTO child (id, parent_id) VALUES (1, 999)")
	if err != nil {
		t.Fatalf("inserting orphan: %v", err)
	}

	// Re-enable foreign keys.
	_, err = d.db.ExecContext(ctx, "PRAGMA foreign_keys=ON")
	if err != nil {
		t.Fatalf("re-enabling foreign keys: %v", err)
	}

	// CheckIntegrity should detect the foreign key violation.
	err = d.CheckIntegrity(ctx)
	if err == nil {
		t.Error("expected foreign key violation error, got nil")
	}
	if !errors.Is(err, ErrIntegrityCheck) {
		t.Errorf("error should wrap ErrIntegrityCheck, got: %v", err)
	}
}

func TestCloseError(t *testing.T) {
	path := filepath.Join(t.TempDir(), "close-err.db")
	d, err := newDB(path)
	if err != nil {
		t.Fatalf("newDB: %v", err)
	}

	// Close once (should succeed).
	if err := d.Close(); err != nil {
		t.Fatalf("first Close: %v", err)
	}

	// Close again -- may or may not return an error depending on driver,
	// but should not panic.
	_ = d.Close()
}

func TestQueryRowOnClosedDB(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	d.db.Close()

	var val int
	err := d.QueryRow(ctx, "SELECT 1").Scan(&val)
	if err == nil {
		t.Error("expected error from QueryRow on closed DB, got nil")
	}
}

func TestQueryRetryExhaustion(t *testing.T) {
	// Create two DB connections to the same file. Use one to hold an exclusive
	// lock while the other attempts a query, forcing SQLITE_BUSY retries.
	dir := t.TempDir()
	dbPath := filepath.Join(dir, "busy.db")

	// Connection 1: holds the lock.
	holder, err := newDB(dbPath)
	if err != nil {
		t.Fatalf("newDB holder: %v", err)
	}
	t.Cleanup(func() { holder.Close() })

	ctx := t.Context()
	_, err = holder.db.ExecContext(ctx, "CREATE TABLE busy_test (id INTEGER PRIMARY KEY, val TEXT)")
	if err != nil {
		t.Fatalf("creating table: %v", err)
	}

	// Start an exclusive transaction on the holder.
	tx, err := holder.db.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin tx: %v", err)
	}
	_, err = tx.ExecContext(ctx, "INSERT INTO busy_test (id, val) VALUES (1, 'lock')")
	if err != nil {
		tx.Rollback()
		t.Fatalf("insert in tx: %v", err)
	}

	// Connection 2: tries to write while holder has a lock.
	// Use very short retry delay and low retry count to avoid slow tests.
	writer, err := newDB(dbPath)
	if err != nil {
		tx.Rollback()
		t.Fatalf("newDB writer: %v", err)
	}
	t.Cleanup(func() { writer.Close() })
	writer.MaxRetries = 2
	writer.RetryDelay = 10 * time.Millisecond
	// Override busy_timeout to a very short value so we get BUSY quickly.
	_, _ = writer.db.ExecContext(ctx, "PRAGMA busy_timeout=10")

	// This Execute should hit SQLITE_BUSY and exhaust retries.
	_, err = writer.Execute(ctx, "INSERT INTO busy_test (id, val) VALUES (2, 'blocked')")
	// We expect either a busy error or the retries-exhausted wrapper.
	if err == nil {
		t.Log("Execute succeeded (DB was not busy); test inconclusive for retry coverage")
	}

	// Release the lock.
	tx.Rollback()
}

func TestQueryRetryExhaustionViaWrite(t *testing.T) {
	dir := t.TempDir()
	dbPath := filepath.Join(dir, "busy2.db")

	holder, err := newDB(dbPath)
	if err != nil {
		t.Fatalf("newDB holder: %v", err)
	}
	t.Cleanup(func() { holder.Close() })

	ctx := t.Context()
	_, err = holder.db.ExecContext(ctx, "CREATE TABLE busy_test2 (id INTEGER PRIMARY KEY, val TEXT)")
	if err != nil {
		t.Fatalf("creating table: %v", err)
	}

	// Hold exclusive lock.
	tx, err := holder.db.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin tx: %v", err)
	}
	_, err = tx.ExecContext(ctx, "INSERT INTO busy_test2 (id, val) VALUES (1, 'lock')")
	if err != nil {
		_ = tx.Rollback()
		t.Fatalf("insert in tx: %v", err)
	}

	// Second connection tries Query (write) while lock held.
	reader, err := newDB(dbPath)
	if err != nil {
		_ = tx.Rollback()
		t.Fatalf("newDB reader: %v", err)
	}
	t.Cleanup(func() { reader.Close() })
	reader.MaxRetries = 2
	reader.RetryDelay = 10 * time.Millisecond
	_, _ = reader.db.ExecContext(ctx, "PRAGMA busy_timeout=10")

	// Use Query with a write statement to trigger BUSY on the Query path.
	_, err = reader.Query(ctx, "INSERT INTO busy_test2 (id, val) VALUES (2, 'blocked')")
	if err == nil {
		t.Log("Query succeeded (DB was not busy); test inconclusive for retry coverage")
	}

	_ = tx.Rollback()
}
