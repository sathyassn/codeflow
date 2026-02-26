package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/spf13/cobra"
)

// defaultDBPath returns the default database path relative to the repo root.
func defaultDBPath() string {
	return filepath.Join(".state", "db", "codeflow.db")
}

// defaultLedgerDir returns the default ledger directory relative to the repo root.
func defaultLedgerDir() string {
	return filepath.Join(".state", "ledger")
}

// defaultMigrationsDir returns the default migrations directory.
func defaultMigrationsDir() string {
	return filepath.Join("codeflow-cli", "internal", "db", "migrations")
}

// newDBCmd creates the top-level "db" command with all subcommands.
func newDBCmd() *cobra.Command {
	dbCmd := &cobra.Command{
		Use:   "db",
		Short: "Database management commands",
		Long:  "Manage the CodeFlow SQLite database: initialize, migrate, sync from JSONL, query, and more.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	dbCmd.AddCommand(newDBInitCmd())
	dbCmd.AddCommand(newDBMigrateCmd())
	dbCmd.AddCommand(newDBSyncCmd())
	dbCmd.AddCommand(newDBQueryCmd())
	dbCmd.AddCommand(newDBExecCmd())
	dbCmd.AddCommand(newDBCheckCmd())
	dbCmd.AddCommand(newDBVersionCmd())
	dbCmd.AddCommand(newDBBackupCmd())

	return dbCmd
}

// newDBInitCmd creates the "db init" subcommand.
func newDBInitCmd() *cobra.Command {
	var dbPath string

	cmd := &cobra.Command{
		Use:   "init",
		Short: "Initialize the database from embedded schema",
		Long:  "Creates the CodeFlow database and applies the embedded schema.sql to set up all tables.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runDBInit(cmd.OutOrStdout(), dbPath)
		},
	}
	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")

	return cmd
}

// runDBInit implements the db init logic.
func runDBInit(w io.Writer, dbPath string) error {
	// Ensure parent directory exists.
	if err := os.MkdirAll(filepath.Dir(dbPath), 0o755); err != nil {
		return &exitError{code: ExitConfigError, err: fmt.Errorf("creating database directory: %w", err)}
	}

	ctx := context.Background()
	d, err := openDB(dbPath)
	if err != nil {
		return &exitError{code: ExitConfigError, err: err}
	}
	defer closeDB(d)

	if err := d.InitFromSchema(ctx); err != nil {
		return &exitError{code: ExitConfigError, err: fmt.Errorf("initializing schema: %w", err)}
	}

	// Set initial user_version to 1.
	if err := d.SetUserVersion(ctx, 1); err != nil {
		return &exitError{code: ExitConfigError, err: fmt.Errorf("setting initial version: %w", err)}
	}

	fmt.Fprintf(w, "Database initialized at %s\n", dbPath)
	return nil
}

// newDBMigrateCmd creates the "db migrate" subcommand.
func newDBMigrateCmd() *cobra.Command {
	var dbPath string
	var migrationsDir string

	cmd := &cobra.Command{
		Use:   "migrate",
		Short: "Apply pending database migrations",
		Long:  "Reads migration files from the migrations directory and applies any that haven't been run yet, using PRAGMA user_version to track progress.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runDBMigrate(cmd.OutOrStdout(), dbPath, migrationsDir)
		},
	}
	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&migrationsDir, "migrations", defaultMigrationsDir(), "path to migrations directory")

	return cmd
}

// runDBMigrate implements the db migrate logic.
func runDBMigrate(w io.Writer, dbPath, migrationsDir string) error {
	ctx := context.Background()
	d, err := openDB(dbPath)
	if err != nil {
		return err
	}
	defer closeDB(d)

	migrations, err := db.LoadMigrationsFromDir(migrationsDir)
	if err != nil {
		return fmt.Errorf("loading migrations: %w", err)
	}

	result, err := d.ApplyMigrations(ctx, migrations)
	if err != nil {
		return fmt.Errorf("applying migrations: %w", err)
	}

	if len(result.Applied) == 0 {
		fmt.Fprintf(w, "No pending migrations. Current version: %d\n", result.CurrentVersion)
	} else {
		fmt.Fprintf(w, "Applied %d migration(s). Version: %d\n", len(result.Applied), result.CurrentVersion)
		for _, v := range result.Applied {
			fmt.Fprintf(w, "  - Applied migration %d\n", v)
		}
	}

	return nil
}

// newDBSyncCmd creates the "db sync" subcommand.
func newDBSyncCmd() *cobra.Command {
	var dbPath string
	var ledgerDir string

	cmd := &cobra.Command{
		Use:   "sync",
		Short: "Rebuild database from JSONL ledger files",
		Long:  "Reads all 4 canonical JSONL files, normalizes events to canonical form, and rebuilds the SQLite database.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runDBSync(cmd.OutOrStdout(), dbPath, ledgerDir)
		},
	}
	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "path to JSONL ledger directory")

	return cmd
}

// runDBSync implements the db sync logic.
func runDBSync(w io.Writer, dbPath, ledgerDir string) error {
	ctx := context.Background()
	d, err := openDB(dbPath)
	if err != nil {
		return err
	}
	defer closeDB(d)

	result, err := d.SyncFromJSONL(ctx, ledgerDir, nil)
	if err != nil {
		return fmt.Errorf("syncing from JSONL: %w", err)
	}

	fmt.Fprintf(w, "Sync complete: %d files, %d events read, %d applied, %d skipped\n",
		result.FilesProcessed, result.EventsRead, result.EventsApplied, result.EventsSkipped)

	for file, count := range result.FileStats {
		fmt.Fprintf(w, "  %s: %d events\n", file, count)
	}

	if len(result.Errors) > 0 {
		fmt.Fprintf(w, "\nWarnings (%d):\n", len(result.Errors))
		for _, e := range result.Errors {
			fmt.Fprintf(w, "  - %s\n", e)
		}
	}

	return nil
}

// newDBQueryCmd creates the "db query" subcommand.
func newDBQueryCmd() *cobra.Command {
	var dbPath string

	cmd := &cobra.Command{
		Use:   "query SQL",
		Short: "Execute a SELECT query and return JSON output",
		Long:  "Runs a read-only SQL query against the database and returns results as a JSON array.",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return runDBQuery(cmd.OutOrStdout(), dbPath, args[0])
		},
	}
	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")

	return cmd
}

// runDBQuery implements the db query logic.
func runDBQuery(w io.Writer, dbPath, query string) error {
	// Basic safety: reject obvious write statements.
	upper := strings.ToUpper(strings.TrimSpace(query))
	for _, prefix := range []string{"INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "REPLACE", "ATTACH"} {
		if strings.HasPrefix(upper, prefix) {
			return fmt.Errorf("query command only supports SELECT statements; use 'db exec' for writes")
		}
	}

	ctx := context.Background()
	d, err := openDB(dbPath)
	if err != nil {
		return err
	}
	defer closeDB(d)

	data, err := d.QueryToJSON(ctx, query)
	if err != nil {
		return fmt.Errorf("executing query: %w", err)
	}

	fmt.Fprintln(w, string(data))
	return nil
}

// newDBExecCmd creates the "db exec" subcommand.
func newDBExecCmd() *cobra.Command {
	var dbPath string
	var argsJSON string

	cmd := &cobra.Command{
		Use:   "exec SQL",
		Short: "Execute a write statement with parameterized args",
		Long:  "Runs an INSERT, UPDATE, or DELETE statement. Use --args to pass a JSON array of parameters.",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return runDBExec(cmd.OutOrStdout(), dbPath, args[0], argsJSON)
		},
	}
	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&argsJSON, "args", "[]", "JSON array of query parameters")

	return cmd
}

// runDBExec implements the db exec logic.
func runDBExec(w io.Writer, dbPath, query, argsJSON string) error {
	ctx := context.Background()
	d, err := openDB(dbPath)
	if err != nil {
		return err
	}
	defer closeDB(d)

	// Parse args from JSON array.
	var params []any
	if err := json.Unmarshal([]byte(argsJSON), &params); err != nil {
		return fmt.Errorf("parsing --args JSON: %w", err)
	}

	result, err := d.ExecWithResult(ctx, query, params...)
	if err != nil {
		return fmt.Errorf("executing statement: %w", err)
	}

	fmt.Fprintf(w, "Rows affected: %d\n", result.RowsAffected)
	return nil
}

// newDBCheckCmd creates the "db check" subcommand.
func newDBCheckCmd() *cobra.Command {
	var dbPath string

	cmd := &cobra.Command{
		Use:   "check",
		Short: "Run database integrity and foreign key checks",
		Long:  "Runs PRAGMA integrity_check and PRAGMA foreign_key_check to verify database health.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runDBCheck(cmd.OutOrStdout(), dbPath)
		},
	}
	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")

	return cmd
}

// runDBCheck implements the db check logic.
func runDBCheck(w io.Writer, dbPath string) error {
	ctx := context.Background()
	d, err := openDB(dbPath)
	if err != nil {
		return err
	}
	defer closeDB(d)

	report, err := d.CheckHealth(ctx)
	if err != nil {
		return fmt.Errorf("running health check: %w", err)
	}

	if report.IsHealthy() {
		fmt.Fprintln(w, "Database health: OK")
		fmt.Fprintln(w, "  integrity_check: ok")
		fmt.Fprintln(w, "  foreign_key_check: ok")
	} else {
		fmt.Fprintln(w, "Database health: ISSUES FOUND")
		if !report.IntegrityOK {
			fmt.Fprintln(w, "  integrity_check: FAILED")
		} else {
			fmt.Fprintln(w, "  integrity_check: ok")
		}
		if !report.ForeignKeyOK {
			fmt.Fprintln(w, "  foreign_key_check: FAILED")
		} else {
			fmt.Fprintln(w, "  foreign_key_check: ok")
		}
		for _, e := range report.Errors {
			fmt.Fprintf(w, "  - %s\n", e)
		}
		return fmt.Errorf("database health check failed")
	}

	return nil
}

// newDBVersionCmd creates the "db version" subcommand.
func newDBVersionCmd() *cobra.Command {
	var dbPath string

	cmd := &cobra.Command{
		Use:   "version",
		Short: "Display current database schema version",
		Long:  "Reads and displays the current PRAGMA user_version value from the database.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runDBVersion(cmd.OutOrStdout(), dbPath)
		},
	}
	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")

	return cmd
}

// runDBVersion implements the db version logic.
func runDBVersion(w io.Writer, dbPath string) error {
	ctx := context.Background()
	d, err := openDB(dbPath)
	if err != nil {
		return err
	}
	defer closeDB(d)

	version, err := d.GetUserVersion(ctx)
	if err != nil {
		return fmt.Errorf("reading database version: %w", err)
	}

	fmt.Fprintf(w, "Database version: %d\n", version)
	return nil
}

// newDBBackupCmd creates the "db backup" subcommand.
func newDBBackupCmd() *cobra.Command {
	var dbPath string

	cmd := &cobra.Command{
		Use:   "backup",
		Short: "Create a timestamped backup of the database",
		Long:  "Copies the database file to .state/db/codeflow-{timestamp}.db.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runDBBackup(cmd.OutOrStdout(), dbPath)
		},
	}
	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")

	return cmd
}

// runDBBackup implements the db backup logic using VACUUM INTO for WAL-safe backup.
func runDBBackup(w io.Writer, dbPath string) error {
	if _, err := os.Stat(dbPath); os.IsNotExist(err) {
		return fmt.Errorf("database file not found: %s", dbPath)
	}

	timestamp := time.Now().Format("20060102-150405")
	backupDir := filepath.Dir(dbPath)
	backupPath := filepath.Join(backupDir, fmt.Sprintf("codeflow-%s.db", timestamp))

	// Check if backup path already exists to avoid overwriting.
	if _, err := os.Stat(backupPath); err == nil {
		return fmt.Errorf("backup file already exists: %s", backupPath)
	}

	ctx := context.Background()
	d, err := openDB(dbPath)
	if err != nil {
		return err
	}
	defer closeDB(d)

	// VACUUM INTO creates a consistent, WAL-safe backup in a single operation.
	// Unlike a naive file copy, this includes all WAL data and produces a valid database.
	if _, err := d.Execute(ctx, "VACUUM INTO ?", backupPath); err != nil {
		return fmt.Errorf("creating backup via VACUUM INTO: %w", err)
	}

	fmt.Fprintf(w, "Backup created: %s\n", backupPath)
	return nil
}

// openDB creates a new DB connection for CLI commands.
func openDB(path string) (*db.DB, error) {
	return db.NewDB(path)
}

// closeDB closes a DB connection, ignoring errors.
func closeDB(d *db.DB) {
	_ = d.Close()
}
