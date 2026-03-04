package db

import (
	"context"
	"database/sql"
	_ "embed"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"

	_ "github.com/ncruces/go-sqlite3/driver"
	_ "github.com/ncruces/go-sqlite3/embed"
)

// Sentinel errors for database operations.
var (
	// ErrNotInitialized indicates the database has not been initialized.
	ErrNotInitialized = errors.New("db: not initialized")

	// ErrIntegrityCheck indicates a database integrity check failure.
	ErrIntegrityCheck = errors.New("db: integrity check failed")

	// ErrMigration indicates a migration failure.
	ErrMigration = errors.New("db: migration failed")
)

// Default configuration values.
const (
	DefaultMaxRetries = 3
	DefaultRetryDelay = 100 * time.Millisecond
)

//go:embed schema.sql
var schemaSQL string

// DB wraps a sql.DB connection with retry logic and SQLite-specific configuration.
type DB struct {
	db         *sql.DB
	path       string
	MaxRetries int
	RetryDelay time.Duration
}

var (
	instance *DB
	once     sync.Once
	mu       sync.Mutex
)

// Get returns the singleton DB instance. It creates the connection on the first
// call using the default path. Subsequent calls return the same instance.
// Panics if the initial connection fails; callers who need error handling should
// use newDB directly.
func Get(path string) *DB {
	mu.Lock()
	defer mu.Unlock()

	once.Do(func() {
		db, err := newDB(path)
		if err != nil {
			panic(fmt.Sprintf("db: failed to initialize singleton: %v", err))
		}
		instance = db
	})
	return instance
}

// Reset closes the current singleton connection and clears the sync.Once so
// that the next call to Get creates a fresh connection. Intended for testing.
func Reset() {
	mu.Lock()
	defer mu.Unlock()

	if instance != nil {
		instance.db.Close()
		instance = nil
	}
	once = sync.Once{}
}

// newDB opens a SQLite database at the given path and configures it with
// required PRAGMAs for WAL mode, foreign keys, busy timeout, and synchronous mode.
func newDB(path string) (*DB, error) {
	sqlDB, err := sql.Open("sqlite3", path)
	if err != nil {
		return nil, fmt.Errorf("db: opening database %s: %w", path, err)
	}

	// SQLite single-writer safety: limit to one connection.
	sqlDB.SetMaxOpenConns(1)

	// Enforce PRAGMAs. These must be set on each connection.
	pragmas := []string{
		"PRAGMA journal_mode=WAL",
		"PRAGMA foreign_keys=ON",
		"PRAGMA busy_timeout=5000",
		"PRAGMA synchronous=NORMAL",
	}

	for _, pragma := range pragmas {
		if _, err := sqlDB.Exec(pragma); err != nil {
			sqlDB.Close()
			return nil, fmt.Errorf("db: setting %s: %w", pragma, err)
		}
	}

	return &DB{
		db:         sqlDB,
		path:       path,
		MaxRetries: DefaultMaxRetries,
		RetryDelay: DefaultRetryDelay,
	}, nil
}

// Query executes a SELECT query with retry logic for SQLITE_BUSY errors.
func (d *DB) Query(ctx context.Context, query string, args ...any) (*sql.Rows, error) {
	var rows *sql.Rows
	var err error

	for attempt := range d.MaxRetries {
		rows, err = d.db.QueryContext(ctx, query, args...)
		if err == nil || !isBusyError(err) {
			return rows, err
		}
		if attempt < d.MaxRetries-1 {
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case <-time.After(d.RetryDelay):
			}
		}
	}
	return rows, fmt.Errorf("db: query failed after %d retries: %w", d.MaxRetries, err)
}

// QueryRow executes a query that returns at most one row.
func (d *DB) QueryRow(ctx context.Context, query string, args ...any) *sql.Row {
	return d.db.QueryRowContext(ctx, query, args...)
}

// Execute runs an INSERT, UPDATE, or DELETE statement with retry logic for
// SQLITE_BUSY errors.
func (d *DB) Execute(ctx context.Context, query string, args ...any) (sql.Result, error) {
	var result sql.Result
	var err error

	for attempt := range d.MaxRetries {
		result, err = d.db.ExecContext(ctx, query, args...)
		if err == nil || !isBusyError(err) {
			return result, err
		}
		if attempt < d.MaxRetries-1 {
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case <-time.After(d.RetryDelay):
			}
		}
	}
	return result, fmt.Errorf("db: execute failed after %d retries: %w", d.MaxRetries, err)
}

// Transaction executes fn within a database transaction. If fn returns an error
// or panics, the transaction is rolled back. Otherwise it is committed.
func (d *DB) Transaction(ctx context.Context, fn func(tx *sql.Tx) error) error {
	tx, err := d.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("db: beginning transaction: %w", err)
	}

	defer func() {
		if p := recover(); p != nil {
			_ = tx.Rollback()
			panic(p)
		}
	}()

	if err := fn(tx); err != nil {
		rbErr := tx.Rollback()
		return errors.Join(err, rbErr)
	}

	if err := tx.Commit(); err != nil {
		return fmt.Errorf("db: committing transaction: %w", err)
	}
	return nil
}

// CheckIntegrity runs PRAGMA integrity_check and PRAGMA foreign_key_check to
// verify the database is not corrupted.
func (d *DB) CheckIntegrity(ctx context.Context) error {
	var result string
	if err := d.db.QueryRowContext(ctx, "PRAGMA integrity_check").Scan(&result); err != nil {
		return fmt.Errorf("db: running integrity check: %w", err)
	}
	if result != "ok" {
		return fmt.Errorf("%w: integrity_check returned %q", ErrIntegrityCheck, result)
	}

	rows, err := d.db.QueryContext(ctx, "PRAGMA foreign_key_check")
	if err != nil {
		return fmt.Errorf("db: running foreign key check: %w", err)
	}
	defer rows.Close()

	if rows.Next() {
		return fmt.Errorf("%w: foreign key violations found", ErrIntegrityCheck)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("db: reading foreign key check results: %w", err)
	}

	return nil
}

// InitFromSchema creates all tables by executing the embedded schema.sql file
// and ensures all migrations are applied.
//
// Strategy:
//   - Fresh DB (no tables exist): run schema.sql which includes all migrated
//     changes, then set user_version to the latest migration.
//   - Existing DB (tables already exist): run Migrate() FIRST to evolve the
//     schema (e.g. add format_id columns), THEN run schema.sql for any new
//     tables/indexes that CREATE IF NOT EXISTS handles idempotently.
//
// The discriminator is whether tables exist, not user_version, because an
// existing DB may have user_version=0 if it was created before migration
// tracking was introduced.
func (d *DB) InitFromSchema(ctx context.Context) error {
	existing, err := d.hasExistingTables(ctx)
	if err != nil {
		return fmt.Errorf("db: checking for existing tables: %w", err)
	}

	if existing {
		// Sync user_version to actual schema state. This handles databases
		// created before migration tracking (user_version=0 but migrations
		// partially applied). Without this, Migrate() would re-apply
		// already-applied migrations and fail on duplicate columns.
		if err := d.syncUserVersionToSchema(ctx); err != nil {
			return fmt.Errorf("db: syncing schema version: %w", err)
		}
		// Temporarily disable FK enforcement during migrations. Migrations
		// rebuild tables (DROP + CREATE + INSERT ... SELECT) and the copied
		// data may contain orphan FK references from historical use. SQLite
		// docs recommend disabling FKs during schema alterations.
		// Note: PRAGMA foreign_keys cannot run inside a transaction, and
		// migration SQL files contain their own BEGIN/COMMIT blocks.
		if _, err := d.db.ExecContext(ctx, "PRAGMA foreign_keys = OFF"); err != nil {
			return fmt.Errorf("db: disabling foreign keys for migration: %w", err)
		}
		// Apply pending migrations FIRST so that schema.sql index/constraint
		// statements find the columns they reference (e.g. format_id must
		// exist before CREATE INDEX ... ON epics(format_id)).
		_, migrateErr := d.Migrate(ctx)
		// Re-enable FK enforcement regardless of migration outcome.
		if _, err := d.db.ExecContext(ctx, "PRAGMA foreign_keys = ON"); err != nil {
			return fmt.Errorf("db: re-enabling foreign keys after migration: %w", err)
		}
		if migrateErr != nil {
			return fmt.Errorf("db: applying migrations before schema sync: %w", migrateErr)
		}
	}

	if _, err := d.db.ExecContext(ctx, schemaSQL); err != nil {
		return fmt.Errorf("db: executing schema: %w", err)
	}

	if !existing {
		// Fresh DB: schema.sql already includes all migrated changes, so set
		// user_version to the latest migration to prevent re-application.
		migrations, loadErr := LoadMigrationsFromEmbed(EmbeddedMigrations())
		if loadErr != nil {
			return fmt.Errorf("db: loading embedded migrations for version sync: %w", loadErr)
		}
		if len(migrations) > 0 {
			latest := migrations[len(migrations)-1].Version
			if err := d.SetUserVersion(ctx, latest); err != nil {
				return fmt.Errorf("db: setting initial version: %w", err)
			}
		}
	}

	return nil
}

// hasExistingTables checks if the database already has application tables.
func (d *DB) hasExistingTables(ctx context.Context) (bool, error) {
	var count int
	err := d.db.QueryRowContext(ctx,
		"SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sessions'",
	).Scan(&count)
	if err != nil {
		return false, err
	}
	return count > 0, nil
}

// syncUserVersionToSchema detects the actual schema state and updates
// user_version if it is behind. This handles databases created before migration
// tracking was introduced (user_version=0 but migrations partially applied).
//
// Detection uses column markers that identify which migrations have been applied:
//
//	Migration 1: tasks.format_id exists
//	Migration 2-4: tasks table rebuilt with CHECK constraints (detected via migration 1)
//	Migration 5: epics.format_id exists AND active_work.current_stage exists
//	Migration 6: tasks.stage CHECK includes 'done' (detected via migration 5)
func (d *DB) syncUserVersionToSchema(ctx context.Context) error {
	ver, err := d.GetUserVersion(ctx)
	if err != nil {
		return err
	}

	// Only sync if version appears behind the actual schema state.
	// Check highest migration marker: migration 5 adds epics.format_id.
	if ver < 5 {
		hasEpicsFmtID, err := d.hasColumn(ctx, "epics", "format_id")
		if err != nil {
			return err
		}
		if hasEpicsFmtID {
			// Migration 5 (and therefore 1-4) already applied.
			if err := d.SetUserVersion(ctx, 5); err != nil {
				return err
			}
			ver = 5
		}
	}

	if ver < 4 {
		hasTasksFmtID, err := d.hasColumn(ctx, "tasks", "format_id")
		if err != nil {
			return err
		}
		if hasTasksFmtID {
			// Migrations 1-4 already applied (tasks rebuilt with format_id).
			if err := d.SetUserVersion(ctx, 4); err != nil {
				return err
			}
		}
	}

	return nil
}

// hasColumn checks if a table has a specific column.
func (d *DB) hasColumn(ctx context.Context, table, column string) (bool, error) {
	var count int
	err := d.db.QueryRowContext(ctx,
		"SELECT COUNT(*) FROM pragma_table_info('"+table+"') WHERE name=?", column, //nolint:gosec // table is internal, not user input
	).Scan(&count)
	if err != nil {
		return false, err
	}
	return count > 0, nil
}

// Migrate loads embedded migrations and applies any that are pending.
// Returns nil if all migrations are already applied.
func (d *DB) Migrate(ctx context.Context) (*MigrateResult, error) {
	migrations, err := LoadMigrationsFromEmbed(EmbeddedMigrations())
	if err != nil {
		return nil, fmt.Errorf("%w: loading embedded migrations: %w", ErrMigration, err)
	}

	result, err := d.ApplyMigrations(ctx, migrations)
	if err != nil {
		return nil, err
	}

	return result, nil
}

// GetUserVersion reads the PRAGMA user_version value from the database.
func (d *DB) GetUserVersion(ctx context.Context) (int, error) {
	var version int
	if err := d.db.QueryRowContext(ctx, "PRAGMA user_version").Scan(&version); err != nil {
		return 0, fmt.Errorf("db: reading user_version: %w", err)
	}
	return version, nil
}

// NewDB opens a new database connection at the given path. Unlike Get, this
// does not use the singleton and each caller gets an independent connection.
// The caller is responsible for closing the returned DB.
func NewDB(path string) (*DB, error) {
	return newDB(path)
}

// Close closes the underlying database connection.
func (d *DB) Close() error {
	if err := d.db.Close(); err != nil {
		return fmt.Errorf("db: closing connection: %w", err)
	}
	return nil
}

// isBusyError checks whether an error indicates SQLite BUSY status.
func isBusyError(err error) bool {
	if err == nil {
		return false
	}
	msg := err.Error()
	return strings.Contains(msg, "SQLITE_BUSY") ||
		strings.Contains(msg, "database is locked")
}
