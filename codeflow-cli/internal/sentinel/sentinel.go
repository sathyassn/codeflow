package sentinel

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

// Scope identifies the sentinel storage scope.
type Scope string

const (
	// ScopePathFlow stores sentinels under .state/sentinels/pathflow/{sessionID}/.
	ScopePathFlow Scope = "pathflow"
)

// sentinelPrefix is prepended to sentinel file names.
const sentinelPrefix = "pathflow-"

// nameRe validates sentinel names: alphanumeric, hyphens, and dots only.
var nameRe = regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9._-]*$`)

// Sentinel errors.
var (
	// ErrInvalidScope indicates an unsupported scope value.
	ErrInvalidScope = errors.New("sentinel: invalid scope")

	// ErrEmptySessionID indicates a missing session ID for pathflow scope.
	ErrEmptySessionID = errors.New("sentinel: session ID required for pathflow scope")

	// ErrEmptyName indicates a missing sentinel name.
	ErrEmptyName = errors.New("sentinel: name must not be empty")

	// ErrInvalidName indicates the sentinel name contains invalid characters.
	ErrInvalidName = errors.New("sentinel: name contains invalid characters")
)

// Manager manages sentinel file operations.
type Manager struct {
	// BaseDir is the project root directory (e.g., the git repo root).
	// Sentinel files are stored under BaseDir/.state/sentinels/.
	BaseDir string
}

// ResolveScopeDir returns the directory path for the given scope and session.
// For PathFlow scope, the path is {BaseDir}/.state/sentinels/pathflow/{sessionID}/.
func (m *Manager) ResolveScopeDir(scope Scope, sessionID string) (string, error) {
	if scope != ScopePathFlow {
		return "", fmt.Errorf("%w: %q", ErrInvalidScope, scope)
	}
	if sessionID == "" {
		return "", ErrEmptySessionID
	}
	return filepath.Join(m.BaseDir, ".state", "sentinels", "pathflow", sessionID), nil
}

// Create creates a sentinel file. The sentinel is an empty file named
// "pathflow-{name}" in the scope directory. Create is idempotent: creating
// an already-existing sentinel overwrites it silently.
func (m *Manager) Create(scope Scope, sessionID, name string) error {
	if err := validateName(name); err != nil {
		return err
	}
	scopeDir, err := m.ResolveScopeDir(scope, sessionID)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(scopeDir, 0o755); err != nil {
		return fmt.Errorf("sentinel: create directory: %w", err)
	}
	path := filepath.Join(scopeDir, sentinelPrefix+name)
	return os.WriteFile(path, nil, 0o644)
}

// Check returns true if the sentinel exists.
func (m *Manager) Check(scope Scope, sessionID, name string) (bool, error) {
	if err := validateName(name); err != nil {
		return false, err
	}
	scopeDir, err := m.ResolveScopeDir(scope, sessionID)
	if err != nil {
		return false, err
	}
	path := filepath.Join(scopeDir, sentinelPrefix+name)
	_, statErr := os.Stat(path)
	if statErr == nil {
		return true, nil
	}
	if os.IsNotExist(statErr) {
		return false, nil
	}
	return false, fmt.Errorf("sentinel: check: %w", statErr)
}

// List returns all sentinel names in the scope directory, sorted
// alphabetically. The "pathflow-" prefix is stripped from the returned names.
// If the directory does not exist, List returns an empty slice (no error).
func (m *Manager) List(scope Scope, sessionID string) ([]string, error) {
	scopeDir, err := m.ResolveScopeDir(scope, sessionID)
	if err != nil {
		return nil, err
	}
	entries, readErr := os.ReadDir(scopeDir)
	if readErr != nil {
		if os.IsNotExist(readErr) {
			return nil, nil
		}
		return nil, fmt.Errorf("sentinel: list: %w", readErr)
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
	sort.Strings(names)
	return names, nil
}

// Delete removes a sentinel file. Delete is idempotent: deleting a
// non-existent sentinel is not an error.
func (m *Manager) Delete(scope Scope, sessionID, name string) error {
	if err := validateName(name); err != nil {
		return err
	}
	scopeDir, err := m.ResolveScopeDir(scope, sessionID)
	if err != nil {
		return err
	}
	path := filepath.Join(scopeDir, sentinelPrefix+name)
	if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("sentinel: delete: %w", err)
	}
	return nil
}

// validateName checks that the sentinel name is non-empty and contains
// only safe characters (no path separators or special characters).
func validateName(name string) error {
	if name == "" {
		return ErrEmptyName
	}
	if !nameRe.MatchString(name) {
		return fmt.Errorf("%w: %q", ErrInvalidName, name)
	}
	return nil
}
