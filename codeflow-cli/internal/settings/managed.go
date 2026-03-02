package settings

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

// ManagedSettingsFile is the filename for the managed settings.
const ManagedSettingsFile = "managed-settings.json"

// SetupResult holds the outcome of a managed settings setup operation.
type SetupResult struct {
	Installed   bool
	BackedUp    bool
	BackupPath  string
	TargetPath  string
	SourcePath  string
	WasExisting bool
}

// sudoOps holds the sudo operation functions. Package-level variables enable
// test injection without requiring interface changes to the public API.
var sudoOps = struct {
	readFile func(path string) ([]byte, error)
	copyFile func(src, dst string) error
	mkdirAll func(dir string) error
	chmod    func(path, mode string) error
}{
	readFile: defaultReadWithSudo,
	copyFile: defaultSudoCopy,
	mkdirAll: defaultSudoMkdir,
	chmod:    defaultSudoChmod,
}

// ManagedSettingsDir returns the platform-specific directory for managed settings.
func ManagedSettingsDir() (string, error) {
	switch runtime.GOOS {
	case "darwin":
		return "/Library/Application Support/ClaudeCode", nil
	case "linux":
		return "/etc/claude-code", nil
	default:
		return "", fmt.Errorf("unsupported operating system: %s", runtime.GOOS)
	}
}

// SetupManagedSettings installs or updates the system-level managed-settings.json
// file. It creates a timestamped backup if a file already exists.
//
// sourceFile is the path to the managed-settings.json source.
// dryRun prevents actual file operations when true.
func SetupManagedSettings(sourceFile string, dryRun bool) (*SetupResult, error) {
	settingsDir, err := ManagedSettingsDir()
	if err != nil {
		return nil, err
	}

	targetPath := filepath.Join(settingsDir, ManagedSettingsFile)

	result := &SetupResult{
		TargetPath: targetPath,
		SourcePath: sourceFile,
	}

	// Verify source file exists.
	if _, err := os.Stat(sourceFile); err != nil {
		return nil, fmt.Errorf("source file not found: %s: %w", sourceFile, err)
	}

	// Check for existing installation.
	existingData, existErr := sudoOps.readFile(targetPath)
	result.WasExisting = existErr == nil && len(existingData) > 0

	if dryRun {
		result.Installed = false
		return result, nil
	}

	// Backup existing file if present.
	if result.WasExisting {
		backupPath := fmt.Sprintf("%s.bak.%s", targetPath, time.Now().Format("20060102-150405"))
		if err := sudoOps.copyFile(targetPath, backupPath); err != nil {
			return nil, fmt.Errorf("backup existing file: %w", err)
		}
		result.BackedUp = true
		result.BackupPath = backupPath
	}

	// Create target directory.
	if err := sudoOps.mkdirAll(settingsDir); err != nil {
		return nil, fmt.Errorf("create settings directory: %w", err)
	}

	// Copy settings file.
	if err := sudoOps.copyFile(sourceFile, targetPath); err != nil {
		return nil, fmt.Errorf("install settings file: %w", err)
	}

	// Set permissions (Unix-like).
	if err := sudoOps.chmod(targetPath, "644"); err != nil {
		return nil, fmt.Errorf("set permissions: %w", err)
	}

	result.Installed = true
	return result, nil
}

// SourceFilePath returns the default source file path for managed settings
// relative to a project directory.
func SourceFilePath(projectDir string) string {
	return filepath.Join(projectDir, ".codeflow", "docs", "security", "claude-enterprise", ManagedSettingsFile)
}

// FormatSetupOutput returns a human-readable summary of a setup result.
func FormatSetupOutput(r *SetupResult) string {
	var b strings.Builder

	b.WriteString("Managed Settings Setup\n")
	b.WriteString(strings.Repeat("=", 50) + "\n\n")

	b.WriteString(fmt.Sprintf("Source: %s\n", r.SourcePath))
	b.WriteString(fmt.Sprintf("Target: %s\n", r.TargetPath))
	b.WriteString(fmt.Sprintf("Existing: %v\n", r.WasExisting))

	if r.BackedUp {
		b.WriteString(fmt.Sprintf("Backup: %s\n", r.BackupPath))
	}

	if r.Installed {
		b.WriteString("\nInstallation successful.\n")
		b.WriteString("Restart Claude Code for settings to take effect.\n")
	} else {
		b.WriteString("\nDry run complete. No changes were made.\n")
	}

	return b.String()
}
