package session

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCleanRuntimeFiles_SkipsEnvFileOwnedByDifferentSession(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()

	// Write env file owned by session-A.
	envContent := "export CODEFLOW_SESSION_ID='ses-aaaa'\nexport CF_PROJECT_ROOT='codeflow'\n"
	envPath := filepath.Join(runtimeDir, EnvFile)
	if err := os.WriteFile(envPath, []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create lock file.
	lockPath := filepath.Join(runtimeDir, "session.lock")
	if err := os.WriteFile(lockPath, []byte(""), 0o644); err != nil {
		t.Fatal(err)
	}

	// Clean as session-B — env file should NOT be removed.
	CleanRuntimeFiles(runtimeDir, "ses-bbbb")

	// Env file should still exist (owned by different session).
	if _, err := os.Stat(envPath); os.IsNotExist(err) {
		t.Error("env file was removed despite belonging to a different session")
	}

	// Lock file should still be removed.
	if _, err := os.Stat(lockPath); !os.IsNotExist(err) {
		t.Error("lock file should be removed regardless of session ownership")
	}
}

func TestCleanRuntimeFiles_RemovesEnvFileOwnedBySameSession(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()

	envContent := "export CODEFLOW_SESSION_ID='ses-aaaa'\nexport CF_PROJECT_ROOT='codeflow'\n"
	envPath := filepath.Join(runtimeDir, EnvFile)
	if err := os.WriteFile(envPath, []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	lockPath := filepath.Join(runtimeDir, "session.lock")
	if err := os.WriteFile(lockPath, []byte(""), 0o644); err != nil {
		t.Fatal(err)
	}

	// Clean as same session — env file SHOULD be removed.
	CleanRuntimeFiles(runtimeDir, "ses-aaaa")

	if _, err := os.Stat(envPath); !os.IsNotExist(err) {
		t.Error("env file should be removed when owned by same session")
	}
	if _, err := os.Stat(lockPath); !os.IsNotExist(err) {
		t.Error("lock file should be removed")
	}
}

func TestCleanRuntimeFiles_RemovesEnvFileWhenSessionIDEmpty(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()

	envPath := filepath.Join(runtimeDir, EnvFile)
	if err := os.WriteFile(envPath, []byte("export CODEFLOW_SESSION_ID='ses-xxx'\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Empty sessionID means no ownership check — always remove.
	CleanRuntimeFiles(runtimeDir, "")

	if _, err := os.Stat(envPath); !os.IsNotExist(err) {
		t.Error("env file should be removed when sessionID is empty")
	}
}

func TestCleanRuntimeFiles_EnvFileMissing(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()

	// No env file, no lock file — should not panic.
	CleanRuntimeFiles(runtimeDir, "ses-test")
}

func TestCleanRuntimeFiles_EnvFileCorrupted(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()

	envPath := filepath.Join(runtimeDir, EnvFile)
	if err := os.WriteFile(envPath, []byte("corrupted content without session ID"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Corrupted env file (no CODEFLOW_SESSION_ID line) — fileSID will be empty,
	// so the ownership check is skipped and env file is removed.
	CleanRuntimeFiles(runtimeDir, "ses-test")

	if _, err := os.Stat(envPath); !os.IsNotExist(err) {
		t.Error("corrupted env file should be removed (empty fileSID)")
	}
}

func TestAcquireAndReleaseSessionLock(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()

	lockFile, err := AcquireSessionLock(runtimeDir)
	if err != nil {
		t.Fatalf("AcquireSessionLock() error: %v", err)
	}
	if lockFile == nil {
		t.Fatal("AcquireSessionLock() returned nil file")
	}

	// Lock file should exist.
	lockPath := filepath.Join(runtimeDir, "session.lock")
	if _, err := os.Stat(lockPath); os.IsNotExist(err) {
		t.Error("session.lock file should be created")
	}

	// Release the lock.
	ReleaseSessionLock(lockFile)

	// Release nil should not panic.
	ReleaseSessionLock(nil)
}

func TestAcquireSessionLock_CreatesDirectory(t *testing.T) {
	t.Parallel()

	runtimeDir := filepath.Join(t.TempDir(), "nested", "runtime")

	lockFile, err := AcquireSessionLock(runtimeDir)
	if err != nil {
		t.Fatalf("AcquireSessionLock() error: %v", err)
	}
	defer ReleaseSessionLock(lockFile)

	// Directory should be created.
	if _, err := os.Stat(runtimeDir); os.IsNotExist(err) {
		t.Error("runtime directory should be created by AcquireSessionLock")
	}
}

func TestWriteEnvFile_Success(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()
	projectDir := "/fake/project/codeflow"

	if err := WriteEnvFile(runtimeDir, "ses-test123", projectDir); err != nil {
		t.Fatalf("WriteEnvFile() error: %v", err)
	}

	envPath := filepath.Join(runtimeDir, EnvFile)
	data, err := os.ReadFile(envPath)
	if err != nil {
		t.Fatalf("read env file: %v", err)
	}

	content := string(data)
	if got, _ := parseEnvFileSessionID(envPath); got != "ses-test123" {
		t.Errorf("session ID = %q, want %q", got, "ses-test123")
	}
	if !contains(content, "CF_PROJECT_ROOT='codeflow'") {
		t.Errorf("env file missing CF_PROJECT_ROOT; content: %s", content)
	}
}

func TestWriteEnvFile_CreatesDirectory(t *testing.T) {
	t.Parallel()

	runtimeDir := filepath.Join(t.TempDir(), "nested", "deep", "runtime")

	if err := WriteEnvFile(runtimeDir, "ses-test", "/fake/project"); err != nil {
		t.Fatalf("WriteEnvFile() error: %v", err)
	}

	envPath := filepath.Join(runtimeDir, EnvFile)
	if _, err := os.Stat(envPath); os.IsNotExist(err) {
		t.Error("env file should be created in nested directory")
	}
}

func TestWriteEnvFile_AtomicNoTmpRemains(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()

	if err := WriteEnvFile(runtimeDir, "ses-test", "/fake"); err != nil {
		t.Fatalf("WriteEnvFile() error: %v", err)
	}

	tmpPath := filepath.Join(runtimeDir, EnvFile+".tmp")
	if _, err := os.Stat(tmpPath); !os.IsNotExist(err) {
		t.Error("tmp file should not remain after successful write")
	}
}

func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(s) > 0 && containsSubstring(s, substr))
}

func containsSubstring(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
