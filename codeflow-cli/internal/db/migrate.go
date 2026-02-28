package db

import (
	"context"
	"embed"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

//go:embed migrations/*.sql
var migrationsFS embed.FS

// Migration represents a single database migration.
type Migration struct {
	Version int
	Name    string
	SQL     string
}

// MigrateResult contains the outcome of a migration run.
type MigrateResult struct {
	Applied       []int `json:"applied"`
	CurrentVersion int  `json:"current_version"`
	PendingCount  int   `json:"pending_count"`
}

// SetUserVersion sets the PRAGMA user_version to the given value.
func (d *DB) SetUserVersion(ctx context.Context, version int) error {
	// PRAGMA user_version cannot use parameter binding.
	query := fmt.Sprintf("PRAGMA user_version = %d", version) //nolint:gosec // version is int, safe
	if _, err := d.db.ExecContext(ctx, query); err != nil {
		return fmt.Errorf("db: setting user_version to %d: %w", version, err)
	}
	return nil
}

// EmbeddedMigrations returns the embedded migrations filesystem.
// This allows the CLI to use migrations from the compiled binary
// without requiring the migrations directory on disk.
func EmbeddedMigrations() embed.FS {
	return migrationsFS
}

// LoadMigrationsFromEmbed reads .sql files from an fs.FS and parses them
// into Migration structs. The FS must contain files under "migrations/"
// following the naming convention: {NNN}_{description}.sql.
func LoadMigrationsFromEmbed(fsys fs.FS) ([]Migration, error) {
	entries, err := fs.ReadDir(fsys, "migrations")
	if err != nil {
		return nil, fmt.Errorf("db: reading embedded migrations: %w", err)
	}

	var migrations []Migration
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".sql") {
			continue
		}
		m, err := parseMigrationEntry(fsys, "migrations", entry)
		if err != nil {
			return nil, err
		}
		migrations = append(migrations, m)
	}

	sort.Slice(migrations, func(i, j int) bool {
		return migrations[i].Version < migrations[j].Version
	})

	return migrations, nil
}

// LoadMigrationsFromDir reads .sql files from a directory and parses them
// into Migration structs. Files must follow the naming convention:
// {NNN}_{description}.sql where NNN is a zero-padded version number.
func LoadMigrationsFromDir(dir string) ([]Migration, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("db: reading migrations dir %s: %w", dir, err)
	}

	var migrations []Migration
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".sql") {
			continue
		}
		m, err := parseMigrationFile(dir, entry)
		if err != nil {
			return nil, err
		}
		migrations = append(migrations, m)
	}

	sort.Slice(migrations, func(i, j int) bool {
		return migrations[i].Version < migrations[j].Version
	})

	return migrations, nil
}

// parseMigrationFile reads a single migration file from disk and extracts its version number.
func parseMigrationFile(dir string, entry fs.DirEntry) (Migration, error) {
	name := entry.Name()
	version, baseName, err := parseMigrationFilename(name)
	if err != nil {
		return Migration{}, err
	}

	content, err := os.ReadFile(filepath.Join(dir, name))
	if err != nil {
		return Migration{}, fmt.Errorf("db: reading migration %s: %w", name, err)
	}

	return Migration{
		Version: version,
		Name:    baseName,
		SQL:     string(content),
	}, nil
}

// parseMigrationEntry reads a single migration file from an fs.FS.
func parseMigrationEntry(fsys fs.FS, dir string, entry fs.DirEntry) (Migration, error) {
	name := entry.Name()
	version, baseName, err := parseMigrationFilename(name)
	if err != nil {
		return Migration{}, err
	}

	content, err := fs.ReadFile(fsys, dir+"/"+name)
	if err != nil {
		return Migration{}, fmt.Errorf("db: reading embedded migration %s: %w", name, err)
	}

	return Migration{
		Version: version,
		Name:    baseName,
		SQL:     string(content),
	}, nil
}

// parseMigrationFilename extracts the version number and description from a filename.
func parseMigrationFilename(name string) (int, string, error) {
	parts := strings.SplitN(name, "_", 2)
	if len(parts) < 2 {
		return 0, "", fmt.Errorf("db: invalid migration filename %q: expected NNN_description.sql", name)
	}

	version, err := strconv.Atoi(parts[0])
	if err != nil {
		return 0, "", fmt.Errorf("db: invalid migration version in %q: %w", name, err)
	}

	return version, strings.TrimSuffix(parts[1], ".sql"), nil
}

// ApplyMigrations applies all pending migrations from the given slice.
// A migration is pending if its version is greater than the current user_version.
func (d *DB) ApplyMigrations(ctx context.Context, migrations []Migration) (*MigrateResult, error) {
	currentVersion, err := d.GetUserVersion(ctx)
	if err != nil {
		return nil, fmt.Errorf("%w: reading current version: %w", ErrMigration, err)
	}

	result := &MigrateResult{
		CurrentVersion: currentVersion,
	}

	// Count pending.
	for _, m := range migrations {
		if m.Version > currentVersion {
			result.PendingCount++
		}
	}

	// Apply pending migrations in order.
	for _, m := range migrations {
		if m.Version <= currentVersion {
			continue
		}

		if _, err := d.db.ExecContext(ctx, m.SQL); err != nil {
			return result, fmt.Errorf("%w: applying migration %d (%s): %w",
				ErrMigration, m.Version, m.Name, err)
		}

		result.Applied = append(result.Applied, m.Version)
		result.CurrentVersion = m.Version
	}

	// Set user_version to the highest applied migration.
	if len(result.Applied) > 0 {
		highestVersion := result.Applied[len(result.Applied)-1]
		if err := d.SetUserVersion(ctx, highestVersion); err != nil {
			return result, fmt.Errorf("%w: %w", ErrMigration, err)
		}
	}

	return result, nil
}

// PendingMigrations returns migrations that have not yet been applied.
func (d *DB) PendingMigrations(ctx context.Context, migrations []Migration) ([]Migration, error) {
	currentVersion, err := d.GetUserVersion(ctx)
	if err != nil {
		return nil, fmt.Errorf("db: checking pending migrations: %w", err)
	}

	var pending []Migration
	for _, m := range migrations {
		if m.Version > currentVersion {
			pending = append(pending, m)
		}
	}
	return pending, nil
}
