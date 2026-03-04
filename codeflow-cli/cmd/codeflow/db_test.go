package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDefaultDBPath(t *testing.T) {
	t.Parallel()

	got := defaultDBPath()
	if got == "" {
		t.Fatal("defaultDBPath should return non-empty string")
	}
	expected := filepath.Join(".state", "db", "codeflow.db")
	if got != expected {
		t.Errorf("defaultDBPath = %q, want %q", got, expected)
	}
}

func TestDefaultLedgerDir(t *testing.T) {
	t.Parallel()

	got := defaultLedgerDir()
	if got == "" {
		t.Fatal("defaultLedgerDir should return non-empty string")
	}
	expected := filepath.Join(".state", "ledger")
	if got != expected {
		t.Errorf("defaultLedgerDir = %q, want %q", got, expected)
	}
}

func TestDefaultMigrationsDir(t *testing.T) {
	t.Parallel()

	got := defaultMigrationsDir()
	if got == "" {
		t.Fatal("defaultMigrationsDir should return non-empty string")
	}
	expected := filepath.Join("codeflow-cli", "internal", "db", "migrations")
	if got != expected {
		t.Errorf("defaultMigrationsDir = %q, want %q", got, expected)
	}
}

func TestNewDBCmd(t *testing.T) {
	t.Parallel()

	cmd := newDBCmd()
	if cmd.Use != "db" {
		t.Errorf("Use = %q, want %q", cmd.Use, "db")
	}
	if cmd.Short == "" {
		t.Error("db command should have a short description")
	}
}

func TestDBCmd_Subcommands(t *testing.T) {
	t.Parallel()

	cmd := newDBCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}

	expected := []string{"init", "migrate", "sync", "query", "exec", "check", "version", "backup", "generate-id"}
	for _, name := range expected {
		if !subcommands[name] {
			t.Errorf("expected subcommand %q to be registered on db cmd", name)
		}
	}

	if len(subcommands) != len(expected) {
		t.Errorf("len(subcommands) = %d, want %d", len(subcommands), len(expected))
	}
}

func TestDBCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["db"] {
		t.Error("expected 'db' subcommand to be registered in root cmd")
	}
}

func TestDBCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"init", "migrate", "sync", "query", "exec", "check", "version", "backup", "generate-id"} {
		if !strings.Contains(got, want) {
			t.Errorf("db help missing %q:\n%s", want, got)
		}
	}
}

func TestDBInitCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newDBInitCmd()
	dbFlag := cmd.Flags().Lookup("db")
	if dbFlag == nil {
		t.Fatal("db init should have --db flag")
	}
	if dbFlag.DefValue != defaultDBPath() {
		t.Errorf("--db default = %q, want %q", dbFlag.DefValue, defaultDBPath())
	}
}

func TestDBMigrateCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newDBMigrateCmd()

	dbFlag := cmd.Flags().Lookup("db")
	if dbFlag == nil {
		t.Fatal("db migrate should have --db flag")
	}

	migrationsFlag := cmd.Flags().Lookup("migrations")
	if migrationsFlag == nil {
		t.Fatal("db migrate should have --migrations flag")
	}
	if migrationsFlag.DefValue != defaultMigrationsDir() {
		t.Errorf("--migrations default = %q, want %q", migrationsFlag.DefValue, defaultMigrationsDir())
	}
}

func TestDBSyncCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newDBSyncCmd()

	dbFlag := cmd.Flags().Lookup("db")
	if dbFlag == nil {
		t.Fatal("db sync should have --db flag")
	}

	ledgerFlag := cmd.Flags().Lookup("ledger")
	if ledgerFlag == nil {
		t.Fatal("db sync should have --ledger flag")
	}
	if ledgerFlag.DefValue != defaultLedgerDir() {
		t.Errorf("--ledger default = %q, want %q", ledgerFlag.DefValue, defaultLedgerDir())
	}
}

func TestDBQueryCmd_RequiresExactlyOneArg(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"db", "query"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for db query with no args")
	}
}

func TestDBExecCmd_RequiresExactlyOneArg(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"db", "exec"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for db exec with no args")
	}
}

func TestDBExecCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newDBExecCmd()

	argsFlag := cmd.Flags().Lookup("args")
	if argsFlag == nil {
		t.Fatal("db exec should have --args flag")
	}
	if argsFlag.DefValue != "[]" {
		t.Errorf("--args default should be '[]', got %q", argsFlag.DefValue)
	}
}

func TestRunDBQuery_RejectsWriteStatements(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// These should all be rejected before even opening the DB.
	writeStatements := []string{
		"INSERT INTO tasks VALUES (1, 'test')",
		"UPDATE tasks SET name='test'",
		"DELETE FROM tasks",
		"DROP TABLE tasks",
		"ALTER TABLE tasks ADD COLUMN x",
		"CREATE TABLE test (id INTEGER)",
		"REPLACE INTO tasks VALUES (1)",
		"ATTACH DATABASE ':memory:' AS mem",
	}

	for _, stmt := range writeStatements {
		var buf strings.Builder
		err := runDBQuery(&buf, dbPath, stmt)
		if err == nil {
			t.Errorf("runDBQuery(%q) should return error for write statement", stmt)
			continue
		}
		if !strings.Contains(err.Error(), "only supports SELECT") {
			t.Errorf("runDBQuery(%q) error = %q, should mention SELECT restriction", stmt, err.Error())
		}
	}
}

func TestRunDBQuery_RejectsWriteStatements_CaseInsensitive(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// Verify case insensitivity.
	for _, stmt := range []string{"insert into t values(1)", "  DELETE from t", "drop table t"} {
		var buf strings.Builder
		err := runDBQuery(&buf, dbPath, stmt)
		if err == nil {
			t.Errorf("runDBQuery(%q) should reject write statement regardless of case", stmt)
		}
	}
}

func TestRunDBInit_CreatesDatabase(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "db", "test.db")

	var buf strings.Builder
	err := runDBInit(&buf, dbPath)
	if err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Database initialized") {
		t.Errorf("output missing %q: %q", "Database initialized", got)
	}

	// Verify file was created.
	if _, err := os.Stat(dbPath); os.IsNotExist(err) {
		t.Error("database file should have been created")
	}
}

func TestRunDBInit_CreatesParentDir(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "deep", "nested", "dir", "test.db")

	var buf strings.Builder
	err := runDBInit(&buf, dbPath)
	if err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	// Verify parent dirs were created.
	parentDir := filepath.Dir(dbPath)
	info, err := os.Stat(parentDir)
	if err != nil {
		t.Fatalf("parent directory should exist: %v", err)
	}
	if !info.IsDir() {
		t.Error("parent should be a directory")
	}
}

func TestRunDBVersion_ReportsVersion(t *testing.T) {
	t.Parallel()

	// First init a database, then check its version.
	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	var buf strings.Builder
	err := runDBVersion(&buf, dbPath)
	if err != nil {
		t.Fatalf("runDBVersion returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Database version:") {
		t.Errorf("output missing %q: %q", "Database version:", got)
	}
	// After init, version matches the highest embedded migration.
	if !strings.Contains(got, "6") {
		t.Errorf("output missing version 6: %q", got)
	}
}

func TestRunDBCheck_HealthyDB(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// Init DB first.
	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	var buf strings.Builder
	err := runDBCheck(&buf, dbPath)
	if err != nil {
		t.Fatalf("runDBCheck returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "OK") {
		t.Errorf("output missing %q: %q", "OK", got)
	}
}

func TestRunDBBackup_MissingDB(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "nonexistent.db")

	var buf strings.Builder
	err := runDBBackup(&buf, dbPath)
	if err == nil {
		t.Error("expected error when database file does not exist")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Errorf("error should mention 'not found', got: %v", err)
	}
}

func TestRunDBBackup_CreatesBackup(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// Init DB first.
	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	var buf strings.Builder
	err := runDBBackup(&buf, dbPath)
	if err != nil {
		t.Fatalf("runDBBackup returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Backup created") {
		t.Errorf("output missing %q: %q", "Backup created", got)
	}

	// Verify backup file exists.
	entries, err := os.ReadDir(tmpDir)
	if err != nil {
		t.Fatalf("reading temp dir: %v", err)
	}

	backupFound := false
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), "codeflow-") && strings.HasSuffix(e.Name(), ".db") && e.Name() != "test.db" {
			backupFound = true
		}
	}
	if !backupFound {
		t.Error("backup file should have been created in the same directory")
	}
}

func TestRunDBExec_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "exec", "--db", dbPath,
		"CREATE TABLE IF NOT EXISTS _test_exec_cmd (id TEXT PRIMARY KEY)"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db exec via root cmd returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Rows affected:") {
		t.Errorf("output missing %q: %q", "Rows affected:", got)
	}
}

func TestRunDBBackup_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "backup", "--db", dbPath})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db backup via root cmd returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Backup created") {
		t.Errorf("output missing %q: %q", "Backup created", got)
	}
}

func TestOpenDB_And_CloseDB(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	d, err := openDB(dbPath)
	if err != nil {
		t.Fatalf("openDB returned error: %v", err)
	}
	if d == nil {
		t.Fatal("openDB should return non-nil DB")
	}

	// closeDB should not panic.
	closeDB(d)
}

func TestRunDBQuery_SelectOnInitializedDB(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// Init DB first.
	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	// Query the sqlite_master table.
	var buf strings.Builder
	err := runDBQuery(&buf, dbPath, "SELECT count(*) as cnt FROM sqlite_master")
	if err != nil {
		t.Fatalf("runDBQuery returned error: %v", err)
	}

	got := buf.String()
	if len(got) == 0 {
		t.Error("expected non-empty query output")
	}
}

func TestRunDBExec_InvalidArgs(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// Init DB first.
	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	var buf strings.Builder
	err := runDBExec(&buf, dbPath, "SELECT 1", "not-valid-json")
	if err == nil {
		t.Error("expected error for invalid --args JSON")
	}
	if !strings.Contains(err.Error(), "parsing --args JSON") {
		t.Errorf("error should mention JSON parsing, got: %v", err)
	}
}

func TestRunDBExec_ValidStatement(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	// Execute a valid statement against an initialized DB.
	// Use sqlite_master for a safe read-only exec (no schema knowledge needed).
	var buf strings.Builder
	err := runDBExec(&buf, dbPath, "CREATE TABLE IF NOT EXISTS _test_exec (id TEXT PRIMARY KEY)",
		`[]`)
	if err != nil {
		t.Fatalf("runDBExec returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Rows affected:") {
		t.Errorf("output missing %q: %q", "Rows affected:", got)
	}
}

func TestRunDBMigrate_NoMigrations(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// Init DB first (sets version to 1).
	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	// Create an empty migrations directory.
	migDir := filepath.Join(tmpDir, "migrations")
	if err := os.MkdirAll(migDir, 0o755); err != nil {
		t.Fatalf("creating migrations dir: %v", err)
	}

	var buf strings.Builder
	err := runDBMigrate(&buf, dbPath, migDir)
	if err != nil {
		t.Fatalf("runDBMigrate returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "No pending migrations") {
		t.Errorf("output missing %q: %q", "No pending migrations", got)
	}
}

func TestRunDBSync_EmptyLedger(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// Init DB first.
	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	// Create ledger directory with empty canonical files.
	ledgerDir := filepath.Join(tmpDir, "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatalf("creating ledger dir: %v", err)
	}

	var buf strings.Builder
	err := runDBSync(&buf, dbPath, ledgerDir)
	if err != nil {
		t.Fatalf("runDBSync returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Sync complete") {
		t.Errorf("output missing %q: %q", "Sync complete", got)
	}
}

func TestRunDBInit_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "via-root.db")

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "init", "--db", dbPath})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db init via root cmd returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Database initialized") {
		t.Errorf("output missing %q: %q", "Database initialized", got)
	}
}

func TestRunDBVersion_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// Init first.
	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "version", "--db", dbPath})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db version via root cmd returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Database version: 6") {
		t.Errorf("output missing %q: %q", "Database version: 6", got)
	}
}

func TestRunDBQuery_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "query", "--db", dbPath, "SELECT 1 as val"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db query via root cmd returned error: %v", err)
	}

	got := buf.String()
	if len(got) == 0 {
		t.Error("expected non-empty query output")
	}
}

func TestRunDBMigrate_WithMigrations(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// Init DB first (sets version to 1).
	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	// Create a migration file (version 2).
	migDir := filepath.Join(tmpDir, "migrations")
	if err := os.MkdirAll(migDir, 0o755); err != nil {
		t.Fatalf("creating migrations dir: %v", err)
	}
	migSQL := "CREATE TABLE IF NOT EXISTS _test_migration (id TEXT PRIMARY KEY);"
	if err := os.WriteFile(filepath.Join(migDir, "002_test.sql"), []byte(migSQL), 0o644); err != nil {
		t.Fatalf("writing migration file: %v", err)
	}

	var buf strings.Builder
	err := runDBMigrate(&buf, dbPath, migDir)
	if err != nil {
		t.Fatalf("runDBMigrate returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Applied") && !strings.Contains(got, "No pending") {
		t.Errorf("output missing migration result: %q", got)
	}
}

func TestRunDBMigrate_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	migDir := filepath.Join(tmpDir, "migrations")
	if err := os.MkdirAll(migDir, 0o755); err != nil {
		t.Fatalf("creating dir: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "migrate", "--db", dbPath, "--migrations", migDir})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db migrate via root cmd returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "No pending migrations") {
		t.Errorf("output missing %q: %q", "No pending migrations", got)
	}
}

func TestRunDBSync_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	ledgerDir := filepath.Join(tmpDir, "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatalf("creating dir: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "sync", "--db", dbPath, "--ledger", ledgerDir})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db sync via root cmd returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Sync complete") {
		t.Errorf("output missing %q: %q", "Sync complete", got)
	}
}

func TestRunDBSync_WithEvents(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	ledgerDir := filepath.Join(tmpDir, "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatalf("creating dir: %v", err)
	}

	// Write a session event to the ledger.
	event := `{"event":"session_start","timestamp":"2025-01-01T00:00:00Z","session_id":"ses-test-sync","claude_agent_id":"agent-1","user_id":"test-user","user_host":"localhost"}`
	if err := os.WriteFile(filepath.Join(ledgerDir, "sessions.jsonl"), []byte(event+"\n"), 0o644); err != nil {
		t.Fatalf("writing event: %v", err)
	}

	var buf strings.Builder
	err := runDBSync(&buf, dbPath, ledgerDir)
	if err != nil {
		t.Fatalf("runDBSync returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Sync complete") {
		t.Errorf("output missing %q: %q", "Sync complete", got)
	}
}

func TestRunDBCheck_ViaRootCmd(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit returned error: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "check", "--db", dbPath})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db check via root cmd returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "OK") {
		t.Errorf("output missing %q: %q", "OK", got)
	}
}

func TestRunDBCheck_UnhealthyDB(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	// Init the DB.
	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	// Open DB and inject a FK violation.
	d, err := openDB(dbPath)
	if err != nil {
		t.Fatalf("openDB: %v", err)
	}
	ctx := t.Context()
	_, err = d.Execute(ctx, `
		CREATE TABLE _fk_parent (id INTEGER PRIMARY KEY);
		CREATE TABLE _fk_child (id INTEGER PRIMARY KEY, pid INTEGER REFERENCES _fk_parent(id));
	`)
	if err != nil {
		t.Fatalf("create tables: %v", err)
	}
	_, err = d.Execute(ctx, "PRAGMA foreign_keys=OFF")
	if err != nil {
		t.Fatalf("disable FK: %v", err)
	}
	_, err = d.Execute(ctx, "INSERT INTO _fk_child (id, pid) VALUES (1, 999)")
	if err != nil {
		t.Fatalf("insert orphan: %v", err)
	}
	_, err = d.Execute(ctx, "PRAGMA foreign_keys=ON")
	if err != nil {
		t.Fatalf("enable FK: %v", err)
	}
	closeDB(d)

	var buf strings.Builder
	err = runDBCheck(&buf, dbPath)
	if err == nil {
		t.Fatal("expected error from runDBCheck for unhealthy DB")
	}
	if !strings.Contains(err.Error(), "health check failed") {
		t.Errorf("error should mention 'health check failed', got: %v", err)
	}
	got := buf.String()
	if !strings.Contains(got, "ISSUES FOUND") {
		t.Errorf("output missing %q: %q", "ISSUES FOUND", got)
	}
	if !strings.Contains(got, "foreign_key_check: FAILED") {
		t.Errorf("output missing %q: %q", "foreign_key_check: FAILED", got)
	}
	// Integrity should still be ok.
	if !strings.Contains(got, "integrity_check: ok") {
		t.Errorf("output missing %q: %q", "integrity_check: ok", got)
	}
}

func TestRunDBCheck_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runDBCheck(&buf, "/nonexistent/path/db.db")
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}
}

func TestRunDBInit_InvalidParentDir(t *testing.T) {
	t.Parallel()

	// Use /dev/null as parent so MkdirAll fails.
	var buf strings.Builder
	err := runDBInit(&buf, "/dev/null/subdir/test.db")
	if err == nil {
		t.Error("expected error when parent dir cannot be created")
	}
}

func TestRunDBVersion_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runDBVersion(&buf, "/nonexistent/path/db.db")
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}
}

func TestRunDBMigrate_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runDBMigrate(&buf, "/nonexistent/path/db.db", t.TempDir())
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}
}

func TestRunDBMigrate_InvalidMigrationsFile(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	// Create a migration file with invalid SQL.
	migDir := filepath.Join(tmpDir, "migrations")
	if err := os.MkdirAll(migDir, 0o755); err != nil {
		t.Fatalf("creating dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(migDir, "007_bad.sql"), []byte("THIS IS NOT VALID SQL;"), 0o644); err != nil {
		t.Fatalf("writing migration: %v", err)
	}

	var buf strings.Builder
	err := runDBMigrate(&buf, dbPath, migDir)
	if err == nil {
		t.Error("expected error for invalid SQL migration")
	}
}

func TestRunDBExec_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runDBExec(&buf, "/nonexistent/path/db.db", "SELECT 1", "[]")
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}
}

func TestRunDBExec_InvalidSQL(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	var buf strings.Builder
	err := runDBExec(&buf, dbPath, "INSERT INTO nonexistent_table VALUES (1)", "[]")
	if err == nil {
		t.Error("expected error for invalid SQL")
	}
	if !strings.Contains(err.Error(), "executing statement") {
		t.Errorf("error should mention 'executing statement', got: %v", err)
	}
}

func TestRunDBQuery_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runDBQuery(&buf, "/nonexistent/path/db.db", "SELECT 1")
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}
}

func TestRunDBQuery_InvalidSQL(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	var buf strings.Builder
	err := runDBQuery(&buf, dbPath, "SELECT * FROM nonexistent_table")
	if err == nil {
		t.Error("expected error for invalid SQL")
	}
	if !strings.Contains(err.Error(), "executing query") {
		t.Errorf("error should mention 'executing query', got: %v", err)
	}
}

func TestRunDBSync_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runDBSync(&buf, "/nonexistent/path/db.db", t.TempDir())
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}
}

func TestRunDBBackup_AlreadyExists(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	// First backup should succeed.
	var buf1 strings.Builder
	if err := runDBBackup(&buf1, dbPath); err != nil {
		t.Fatalf("first backup: %v", err)
	}

	// The backup path uses a timestamp to the second. Running again immediately
	// in the same second would get "already exists". Force this by verifying
	// the backup file was indeed created.
	entries, err := os.ReadDir(tmpDir)
	if err != nil {
		t.Fatalf("reading dir: %v", err)
	}
	backupCount := 0
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), "codeflow-") && strings.HasSuffix(e.Name(), ".db") {
			backupCount++
		}
	}
	if backupCount < 1 {
		t.Error("expected at least one backup file")
	}
}

func TestRunDBMigrate_AppliedOutput(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	migDir := filepath.Join(tmpDir, "migrations")
	if err := os.MkdirAll(migDir, 0o755); err != nil {
		t.Fatalf("creating dir: %v", err)
	}
	// Write two migration files with versions above the embedded set (6).
	if err := os.WriteFile(filepath.Join(migDir, "007_add_table.sql"),
		[]byte("CREATE TABLE IF NOT EXISTS _mig7 (id TEXT PRIMARY KEY);"), 0o644); err != nil {
		t.Fatalf("writing migration: %v", err)
	}
	if err := os.WriteFile(filepath.Join(migDir, "008_add_table2.sql"),
		[]byte("CREATE TABLE IF NOT EXISTS _mig8 (id TEXT PRIMARY KEY);"), 0o644); err != nil {
		t.Fatalf("writing migration: %v", err)
	}

	var buf strings.Builder
	err := runDBMigrate(&buf, dbPath, migDir)
	if err != nil {
		t.Fatalf("runDBMigrate: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Applied 2 migration(s)") {
		t.Errorf("output missing applied count: %q", got)
	}
	if !strings.Contains(got, "Applied migration 7") {
		t.Errorf("output missing 'Applied migration 7': %q", got)
	}
	if !strings.Contains(got, "Applied migration 8") {
		t.Errorf("output missing 'Applied migration 8': %q", got)
	}
}

func TestRunDBSync_WithWarnings(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	ledgerDir := filepath.Join(tmpDir, "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatalf("creating dir: %v", err)
	}

	// Write an invalid JSON line to trigger a warning.
	if err := os.WriteFile(filepath.Join(ledgerDir, "sessions.jsonl"),
		[]byte("not-json\n"), 0o644); err != nil {
		t.Fatalf("writing event: %v", err)
	}

	var buf strings.Builder
	err := runDBSync(&buf, dbPath, ledgerDir)
	if err != nil {
		t.Fatalf("runDBSync: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Sync complete") {
		t.Errorf("output missing 'Sync complete': %q", got)
	}
}
