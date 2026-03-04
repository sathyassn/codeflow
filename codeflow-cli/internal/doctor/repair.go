package doctor

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/codeflow/codeflow-cli/internal/db"
)

// RepairDatabase rebuilds the SQLite database from JSONL ledger files.
func RepairDatabase(ctx context.Context, opts *Options) error {
	if opts.DBPath == "" {
		return fmt.Errorf("repair database: database path not configured")
	}
	if opts.LedgerDir == "" {
		return fmt.Errorf("repair database: ledger directory not configured")
	}

	// Ensure the parent directory exists.
	if err := os.MkdirAll(filepath.Dir(opts.DBPath), 0o755); err != nil {
		return fmt.Errorf("repair database: creating directory: %w", err)
	}

	// Remove the existing database to start fresh.
	if _, err := os.Stat(opts.DBPath); err == nil {
		if err := os.Remove(opts.DBPath); err != nil {
			return fmt.Errorf("repair database: removing existing database: %w", err)
		}
		// Also remove WAL and SHM files if they exist.
		os.Remove(opts.DBPath + "-wal")
		os.Remove(opts.DBPath + "-shm")
	}

	d, err := db.NewDB(opts.DBPath)
	if err != nil {
		return fmt.Errorf("repair database: opening new database: %w", err)
	}
	defer d.Close()

	// Initialize the schema and apply any pending migrations.
	if err := d.InitFromSchema(ctx); err != nil {
		return fmt.Errorf("repair database: initializing schema: %w", err)
	}
	if _, err := d.Migrate(ctx); err != nil {
		return fmt.Errorf("repair database: applying migrations: %w", err)
	}

	// Sync from JSONL.
	_, err = d.SyncFromJSONL(ctx, opts.LedgerDir, nil)
	if err != nil {
		return fmt.Errorf("repair database: syncing from JSONL: %w", err)
	}

	return nil
}

// RepairPermissions fixes file permissions on .state/ and hook scripts.
func RepairPermissions(_ context.Context, opts *Options) error {
	stateDir := opts.StateDir
	if stateDir == "" {
		stateDir = filepath.Join(opts.ProjectDir, ".state")
	}

	// Ensure .state/ directory and subdirectories are writable.
	requiredDirs := []string{"db", "ledger", "logs", "runtime", "sentinels"}
	for _, dir := range requiredDirs {
		path := filepath.Join(stateDir, dir)
		if err := os.MkdirAll(path, 0o755); err != nil {
			return fmt.Errorf("repair permissions: creating %s: %w", dir, err)
		}
	}

	// Make hook scripts executable.
	hooksDir := filepath.Join(opts.ProjectDir, ".claude", "hooks", "codeflow")
	if _, err := os.Stat(hooksDir); err == nil {
		err := filepath.WalkDir(hooksDir, func(path string, d os.DirEntry, err error) error {
			if err != nil || d.IsDir() {
				return nil
			}
			if !strings.HasSuffix(d.Name(), ".sh") {
				return nil
			}
			info, err := os.Stat(path)
			if err != nil {
				return nil
			}
			if info.Mode()&0o111 == 0 {
				if err := os.Chmod(path, info.Mode()|0o755); err != nil {
					return fmt.Errorf("setting executable: %s: %w", path, err)
				}
			}
			return nil
		})
		if err != nil {
			return fmt.Errorf("repair permissions: fixing hooks: %w", err)
		}
	}

	return nil
}

// RepairConfig regenerates missing config directories and files.
func RepairConfig(_ context.Context, opts *Options) error {
	configDir := filepath.Join(opts.ProjectDir, ".codeflow", "config")

	// Ensure the config directory structure exists.
	requiredDirs := []string{
		filepath.Join(configDir, "enforcement"),
		filepath.Join(configDir, "pathflow"),
	}

	for _, dir := range requiredDirs {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return fmt.Errorf("repair config: creating %s: %w", dir, err)
		}
	}

	return nil
}
