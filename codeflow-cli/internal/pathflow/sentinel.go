package pathflow

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

const sentinelPrefix = "pathflow-"

// Create creates a sentinel file at {sentinelDir}/pathflow-{name}.
// The sentinel directory is created if it does not exist.
// Idempotent: succeeds silently if the sentinel already exists.
func Create(sentinelDir, name string) error {
	if sentinelDir == "" {
		return fmt.Errorf("sentinel: empty sentinel directory")
	}
	if name == "" {
		return fmt.Errorf("sentinel: empty name")
	}

	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		return fmt.Errorf("sentinel: creating directory %s: %w", sentinelDir, err)
	}

	path := filepath.Join(sentinelDir, sentinelPrefix+name)
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("sentinel: creating file %s: %w", path, err)
	}
	return f.Close()
}

// Exists checks whether a sentinel file exists at {sentinelDir}/pathflow-{name}.
func Exists(sentinelDir, name string) bool {
	if sentinelDir == "" || name == "" {
		return false
	}
	path := filepath.Join(sentinelDir, sentinelPrefix+name)
	_, err := os.Stat(path)
	return err == nil
}

// List returns the names of all sentinel files in sentinelDir, with the
// "pathflow-" prefix stripped. Returns an empty slice if the directory
// does not exist or contains no sentinel files.
func List(sentinelDir string) ([]string, error) {
	if sentinelDir == "" {
		return nil, fmt.Errorf("sentinel: empty sentinel directory")
	}

	entries, err := os.ReadDir(sentinelDir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("sentinel: reading directory %s: %w", sentinelDir, err)
	}

	var names []string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		n := e.Name()
		if strings.HasPrefix(n, sentinelPrefix) {
			names = append(names, strings.TrimPrefix(n, sentinelPrefix))
		}
	}
	return names, nil
}
