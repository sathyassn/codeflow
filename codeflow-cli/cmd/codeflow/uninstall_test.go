package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// Primary uninstall tests live in main_test.go. This file adds edge case
// coverage and satisfies the Go test file convention (uninstall.go -> uninstall_test.go).

func TestRunUninstall_NoBinaryWithConfigExists(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	binaryPath := filepath.Join(tmpDir, "nonexistent-binary")
	configDir := filepath.Join(tmpDir, "config")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatalf("creating config dir: %v", err)
	}

	// Binary absent, config exists, keepConfig=false: should prompt for
	// config removal since there is something to remove.
	var buf strings.Builder
	err := runUninstall(strings.NewReader("y\n"), &buf, binaryPath, configDir, false, false)
	if err != nil {
		t.Fatalf("runUninstall returned error: %v", err)
	}

	// Config should be removed after confirmation.
	if _, err := os.Stat(configDir); !os.IsNotExist(err) {
		t.Error("config dir should have been removed after 'y' confirmation")
	}
}

func TestRunUninstall_NoBinaryKeepConfig(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	binaryPath := filepath.Join(tmpDir, "nonexistent-binary")
	configDir := filepath.Join(tmpDir, "config")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatalf("creating config dir: %v", err)
	}

	// Binary absent, config exists, keepConfig=true: nothing to remove.
	var buf strings.Builder
	err := runUninstall(strings.NewReader(""), &buf, binaryPath, configDir, false, true)
	if err != nil {
		t.Fatalf("runUninstall returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "not found") {
		t.Errorf("output missing %q: %q", "not found", got)
	}

	// Config should still exist.
	if _, err := os.Stat(configDir); os.IsNotExist(err) {
		t.Error("config dir should be preserved when keepConfig=true")
	}
}

func TestRunUninstall_ForceRemoveConfigError(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()

	// Make config directory non-removable by placing it inside a read-only parent.
	protectedDir := filepath.Join(tmpDir, "protected")
	innerConfig := filepath.Join(protectedDir, "config")
	if err := os.MkdirAll(innerConfig, 0o755); err != nil {
		t.Fatalf("creating inner config dir: %v", err)
	}
	if err := os.Chmod(protectedDir, 0o555); err != nil {
		t.Fatalf("chmod: %v", err)
	}
	t.Cleanup(func() { os.Chmod(protectedDir, 0o755) })

	// Create a real binary so we get past the "not found" path.
	realBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(realBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("creating binary: %v", err)
	}

	var buf strings.Builder
	err := runUninstall(strings.NewReader(""), &buf, realBinary, innerConfig, true, false)
	if err == nil {
		t.Fatal("expected error when config dir cannot be removed")
	}
	if !strings.Contains(err.Error(), "removing config directory") {
		t.Errorf("error = %v, want it to contain %q", err, "removing config directory")
	}
}
