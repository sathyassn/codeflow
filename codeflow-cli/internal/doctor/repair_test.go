package doctor

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestRepairDatabase(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	dbDir := filepath.Join(projectDir, ".state", "db")
	ledgerDir := filepath.Join(projectDir, ".state", "ledger")

	if err := os.MkdirAll(dbDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create valid JSONL files (empty but valid).
	for _, f := range canonicalJSONLFiles {
		if err := os.WriteFile(filepath.Join(ledgerDir, f), []byte(""), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	dbPath := filepath.Join(dbDir, "codeflow.db")

	opts := &Options{
		DBPath:    dbPath,
		LedgerDir: ledgerDir,
	}

	if err := RepairDatabase(ctx, opts); err != nil {
		t.Fatalf("RepairDatabase failed: %v", err)
	}

	// Verify the database was created.
	if _, err := os.Stat(dbPath); os.IsNotExist(err) {
		t.Fatal("database file was not created")
	}

	// Verify the database is healthy by running a check.
	checkOpts := &Options{DBPath: dbPath}
	checkOpts.applyDefaults()
	result := checkDatabase(ctx, checkOpts)
	if result.Status != StatusPass {
		t.Errorf("repaired database is not healthy: %s", result.Message)
	}
}

func TestRepairDatabase_MissingPaths(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	t.Run("empty_db_path", func(t *testing.T) {
		t.Parallel()
		opts := &Options{DBPath: "", LedgerDir: t.TempDir()}
		if err := RepairDatabase(ctx, opts); err == nil {
			t.Fatal("expected error for empty db path")
		}
	})

	t.Run("empty_ledger_dir", func(t *testing.T) {
		t.Parallel()
		opts := &Options{DBPath: filepath.Join(t.TempDir(), "test.db"), LedgerDir: ""}
		if err := RepairDatabase(ctx, opts); err == nil {
			t.Fatal("expected error for empty ledger dir")
		}
	})
}

func TestRepairDatabase_ReplacesExisting(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	dbDir := filepath.Join(projectDir, ".state", "db")
	ledgerDir := filepath.Join(projectDir, ".state", "ledger")
	os.MkdirAll(dbDir, 0o755)
	os.MkdirAll(ledgerDir, 0o755)

	dbPath := filepath.Join(dbDir, "codeflow.db")

	// Create a "corrupted" database (just garbage bytes).
	if err := os.WriteFile(dbPath, []byte("corrupted data"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create empty but valid JSONL files.
	for _, f := range canonicalJSONLFiles {
		if err := os.WriteFile(filepath.Join(ledgerDir, f), []byte(""), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	opts := &Options{
		DBPath:    dbPath,
		LedgerDir: ledgerDir,
	}

	if err := RepairDatabase(ctx, opts); err != nil {
		t.Fatalf("RepairDatabase failed on existing file: %v", err)
	}

	// Verify it's now healthy.
	checkOpts := &Options{DBPath: dbPath}
	checkOpts.applyDefaults()
	result := checkDatabase(ctx, checkOpts)
	if result.Status != StatusPass {
		t.Errorf("repaired database is not healthy: %s", result.Message)
	}
}

func TestRepairPermissions(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	stateDir := filepath.Join(projectDir, ".state")

	// Don't pre-create subdirectories — repair should create them.
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create a non-executable hook script.
	hooksDir := filepath.Join(projectDir, ".claude", "hooks", "codeflow", "pre-tool-use")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}
	scriptPath := filepath.Join(hooksDir, "test.sh")
	if err := os.WriteFile(scriptPath, []byte("#!/bin/bash\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		ProjectDir: projectDir,
		StateDir:   stateDir,
	}

	if err := RepairPermissions(ctx, opts); err != nil {
		t.Fatalf("RepairPermissions failed: %v", err)
	}

	// Verify state subdirectories were created.
	for _, dir := range []string{"db", "ledger", "logs", "runtime", "sentinels"} {
		path := filepath.Join(stateDir, dir)
		info, err := os.Stat(path)
		if err != nil {
			t.Errorf("expected directory %s to exist: %v", dir, err)
			continue
		}
		if !info.IsDir() {
			t.Errorf("%s should be a directory", dir)
		}
	}

	// Verify the hook script is now executable.
	info, err := os.Stat(scriptPath)
	if err != nil {
		t.Fatalf("stat hook script: %v", err)
	}
	if info.Mode()&0o111 == 0 {
		t.Error("hook script should be executable after repair")
	}
}

func TestRepairConfig(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()

	opts := &Options{ProjectDir: projectDir}

	if err := RepairConfig(ctx, opts); err != nil {
		t.Fatalf("RepairConfig failed: %v", err)
	}

	// Verify the config directories were created.
	expectedDirs := []string{
		filepath.Join(projectDir, ".codeflow", "config", "enforcement"),
		filepath.Join(projectDir, ".codeflow", "config", "pathflow"),
	}

	for _, dir := range expectedDirs {
		info, err := os.Stat(dir)
		if err != nil {
			t.Errorf("expected directory %s to exist: %v", dir, err)
			continue
		}
		if !info.IsDir() {
			t.Errorf("%s should be a directory", dir)
		}
	}
}

func TestRepairConfig_ExistingStructure(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	os.MkdirAll(configDir, 0o755)

	// Write a config file.
	data, _ := json.MarshalIndent(map[string]string{"key": "value"}, "", "  ")
	configFile := filepath.Join(configDir, "test.json")
	os.WriteFile(configFile, data, 0o644)

	opts := &Options{ProjectDir: projectDir}

	if err := RepairConfig(ctx, opts); err != nil {
		t.Fatalf("RepairConfig failed: %v", err)
	}

	// Verify existing config file was not destroyed.
	readData, err := os.ReadFile(configFile)
	if err != nil {
		t.Fatalf("config file was destroyed: %v", err)
	}
	if string(readData) != string(data) {
		t.Error("config file content was modified")
	}
}
