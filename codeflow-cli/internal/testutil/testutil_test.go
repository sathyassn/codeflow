package testutil

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestTempProject(t *testing.T) {
	t.Run("creates expected directory structure", func(t *testing.T) {
		dir := TempProject(t)

		info, err := os.Stat(dir)
		if err != nil {
			t.Fatalf("TempProject root does not exist: %v", err)
		}
		if !info.IsDir() {
			t.Fatal("TempProject root is not a directory")
		}

		expectedDirs := []string{
			".codeflow/config",
			".codeflow/scripts",
			".claude",
			".state/db",
			".state/ledger",
		}

		for _, sub := range expectedDirs {
			path := filepath.Join(dir, sub)
			info, err := os.Stat(path)
			if err != nil {
				t.Errorf("expected directory %s does not exist: %v", sub, err)
				continue
			}
			if !info.IsDir() {
				t.Errorf("expected %s to be a directory, got file", sub)
			}
		}
	})

	t.Run("directory is writable", func(t *testing.T) {
		dir := TempProject(t)

		testFile := filepath.Join(dir, "test.txt")
		err := os.WriteFile(testFile, []byte("hello"), 0o644)
		if err != nil {
			t.Fatalf("failed to write to TempProject dir: %v", err)
		}

		if _, err := os.Stat(testFile); err != nil {
			t.Fatalf("test file should exist: %v", err)
		}
	})

	t.Run("each call creates a unique directory", func(t *testing.T) {
		dir1 := TempProject(t)
		dir2 := TempProject(t)
		if dir1 == dir2 {
			t.Error("TempProject should return unique directories for each call")
		}
	})
}

func TestCopyFixture(t *testing.T) {
	t.Run("valid-project copies all contents", func(t *testing.T) {
		dest := CopyFixture(t, "valid-project")

		codeflowDir := filepath.Join(dest, ".codeflow", "config")
		info, err := os.Stat(codeflowDir)
		if err != nil {
			t.Fatalf("expected .codeflow/config in copied fixture: %v", err)
		}
		if !info.IsDir() {
			t.Fatal(".codeflow/config should be a directory")
		}

		claudeDir := filepath.Join(dest, ".claude")
		info, err = os.Stat(claudeDir)
		if err != nil {
			t.Fatalf("expected .claude in copied fixture: %v", err)
		}
		if !info.IsDir() {
			t.Fatal(".claude should be a directory")
		}
	})

	t.Run("valid-project preserves file content", func(t *testing.T) {
		dest := CopyFixture(t, "valid-project")

		// The valid-project fixture contains a pathflow-config.json with real content.
		configPath := filepath.Join(dest, ".codeflow", "config", "pathflow-config.json")
		data, err := os.ReadFile(configPath)
		if err != nil {
			t.Fatalf("expected pathflow-config.json to exist: %v", err)
		}
		if len(data) == 0 {
			t.Fatal("pathflow-config.json should not be empty")
		}

		// Verify content matches the source fixture.
		srcPath := filepath.Join(findModuleRoot(t), "testdata", "projects", "valid-project",
			".codeflow", "config", "pathflow-config.json")
		srcData, err := os.ReadFile(srcPath)
		if err != nil {
			t.Fatalf("failed to read source fixture: %v", err)
		}
		if string(data) != string(srcData) {
			t.Error("copied file content does not match source fixture")
		}
	})

	t.Run("missing-config has no codeflow dir", func(t *testing.T) {
		dest := CopyFixture(t, "missing-config")

		codeflowDir := filepath.Join(dest, ".codeflow")
		_, err := os.Stat(codeflowDir)
		if err == nil {
			t.Fatal("missing-config fixture should not have .codeflow directory")
		}
		if !os.IsNotExist(err) {
			t.Fatalf("unexpected error checking .codeflow: %v", err)
		}
	})

	t.Run("no-claude fixture has codeflow but no claude", func(t *testing.T) {
		dest := CopyFixture(t, "no-claude")

		// Should have .codeflow
		codeflowDir := filepath.Join(dest, ".codeflow")
		if _, err := os.Stat(codeflowDir); err != nil {
			t.Fatalf("no-claude fixture should have .codeflow: %v", err)
		}

		// Should not have .claude
		claudeDir := filepath.Join(dest, ".claude")
		_, err := os.Stat(claudeDir)
		if err == nil {
			t.Fatal("no-claude fixture should not have .claude directory")
		}
		if !os.IsNotExist(err) {
			t.Fatalf("unexpected error checking .claude: %v", err)
		}
	})

	t.Run("nonexistent fixture fails", func(t *testing.T) {
		// Verify that CopyFixture calls t.Fatalf for a missing fixture.
		// We cannot directly observe t.Fatalf in a sub-test without aborting,
		// so we verify the precondition: the fixture path does not exist.
		fixturePath := filepath.Join(findModuleRoot(t), "testdata", "projects", "nonexistent-fixture-xyz")
		_, err := os.Stat(fixturePath)
		if !os.IsNotExist(err) {
			t.Fatalf("expected nonexistent fixture path, got: %v", err)
		}
	})

	t.Run("corrupted-db fixture copies invalid file", func(t *testing.T) {
		dest := CopyFixture(t, "corrupted-db")

		dbPath := filepath.Join(dest, ".codeflow", "codeflow.db")
		data, err := os.ReadFile(dbPath)
		if err != nil {
			t.Fatalf("expected corrupted db file to be copied: %v", err)
		}
		if len(data) == 0 {
			t.Fatal("corrupted db file should not be empty")
		}
	})

	t.Run("valid-project preserves file permissions", func(t *testing.T) {
		dest := CopyFixture(t, "valid-project")

		// Verify that CopyFixture preserves file mode (exercises d.Info() path).
		configPath := filepath.Join(dest, ".codeflow", "config", "pathflow-config.json")
		info, err := os.Stat(configPath)
		if err != nil {
			t.Fatalf("expected pathflow-config.json: %v", err)
		}
		// File should be a regular file with some permissions set.
		if info.Mode().IsDir() {
			t.Error("pathflow-config.json should be a regular file, not a directory")
		}
		if info.Mode().Perm() == 0 {
			t.Error("pathflow-config.json should have non-zero permissions")
		}
	})

	t.Run("copied fixture is independent from source", func(t *testing.T) {
		dest := CopyFixture(t, "valid-project")

		// Write a new file in the copy.
		newFile := filepath.Join(dest, "new-file.txt")
		if err := os.WriteFile(newFile, []byte("test"), 0o644); err != nil {
			t.Fatalf("failed to write to copied fixture: %v", err)
		}

		// Verify the source fixture does not have the new file.
		srcPath := filepath.Join(findModuleRoot(t), "testdata", "projects", "valid-project", "new-file.txt")
		if _, err := os.Stat(srcPath); err == nil {
			t.Fatal("source fixture should not be modified by writes to the copy")
		}
	})
}

func TestCopyDir(t *testing.T) {
	t.Run("nonexistent source returns error", func(t *testing.T) {
		dest := t.TempDir()
		err := copyDir("/nonexistent-path-xyz-12345", dest)
		if err == nil {
			t.Error("copyDir should return error for nonexistent source")
		}
	})

	t.Run("unwritable destination returns error", func(t *testing.T) {
		src := filepath.Join(findModuleRoot(t), "testdata", "projects", "valid-project")
		dest := t.TempDir()
		// Make dest unwritable so MkdirAll/WriteFile inside it fails.
		if err := os.Chmod(dest, 0o444); err != nil {
			t.Fatalf("failed to chmod dest: %v", err)
		}
		t.Cleanup(func() { os.Chmod(dest, 0o755) })

		err := copyDir(src, dest)
		if err == nil {
			t.Error("copyDir should return error for unwritable destination")
		}
	})

	t.Run("unreadable file returns error", func(t *testing.T) {
		src := t.TempDir()
		dest := t.TempDir()
		// Create a file that cannot be read.
		unreadable := filepath.Join(src, "secret.txt")
		if err := os.WriteFile(unreadable, []byte("data"), 0o000); err != nil {
			t.Fatalf("failed to create unreadable file: %v", err)
		}
		t.Cleanup(func() { os.Chmod(unreadable, 0o644) })

		err := copyDir(src, dest)
		if err == nil {
			t.Error("copyDir should return error for unreadable file")
		}
	})

	t.Run("unwritable subdirectory returns error on file write", func(t *testing.T) {
		src := t.TempDir()
		dest := t.TempDir()
		// Create src with a file inside a subdirectory.
		if err := os.MkdirAll(filepath.Join(src, "sub"), 0o755); err != nil {
			t.Fatalf("failed to create src subdir: %v", err)
		}
		if err := os.WriteFile(filepath.Join(src, "sub", "file.txt"), []byte("data"), 0o644); err != nil {
			t.Fatalf("failed to create src file: %v", err)
		}
		// Create the matching subdirectory in dest but make it unwritable,
		// so directory traversal succeeds but file WriteFile fails.
		destSub := filepath.Join(dest, "sub")
		if err := os.MkdirAll(destSub, 0o755); err != nil {
			t.Fatalf("failed to create dest subdir: %v", err)
		}
		if err := os.Chmod(destSub, 0o555); err != nil {
			t.Fatalf("failed to chmod dest subdir: %v", err)
		}
		t.Cleanup(func() { os.Chmod(destSub, 0o755) })

		err := copyDir(src, dest)
		if err == nil {
			t.Error("copyDir should return error when file write to unwritable directory fails")
		}
	})

	t.Run("successful copy returns nil", func(t *testing.T) {
		src := filepath.Join(findModuleRoot(t), "testdata", "projects", "valid-project")
		dest := t.TempDir()
		err := copyDir(src, dest)
		if err != nil {
			t.Errorf("copyDir should succeed for valid source: %v", err)
		}
	})
}

func TestAssertNoError(t *testing.T) {
	t.Run("nil error passes", func(t *testing.T) {
		AssertNoError(t, nil, "should not fail")
	})

	t.Run("typed nil error passes", func(t *testing.T) {
		var err error
		AssertNoError(t, err, "typed nil error")
	})

	t.Run("non-nil error fails the test", func(t *testing.T) {
		// Subprocess pattern: re-run this test binary with a flag that triggers the failure.
		// This is the standard Go technique for testing code that calls t.Fatal / os.Exit.
		if os.Getenv("TEST_ASSERT_NO_ERROR_FAIL") == "1" {
			AssertNoError(t, os.ErrNotExist, "expected failure")
			return
		}
		cmd := exec.Command(os.Args[0], "-test.run=^TestAssertNoError$/non-nil_error_fails_the_test")
		cmd.Env = append(os.Environ(), "TEST_ASSERT_NO_ERROR_FAIL=1")
		out, err := cmd.CombinedOutput()
		// The subprocess should exit non-zero because AssertNoError calls t.Fatalf.
		if err == nil {
			t.Fatal("expected subprocess to fail, but it succeeded")
		}
		if !strings.Contains(string(out), "expected failure") {
			t.Errorf("expected error message in output, got: %s", out)
		}
	})
}

func TestCopyFixtureNonexistent(t *testing.T) {
	// Subprocess pattern: test that CopyFixture calls t.Fatalf for a missing fixture.
	if os.Getenv("TEST_COPY_FIXTURE_NONEXISTENT") == "1" {
		CopyFixture(t, "absolutely-nonexistent-fixture-xyz")
		return
	}
	cmd := exec.Command(os.Args[0], "-test.run=^TestCopyFixtureNonexistent$")
	cmd.Env = append(os.Environ(), "TEST_COPY_FIXTURE_NONEXISTENT=1")
	out, err := cmd.CombinedOutput()
	if err == nil {
		t.Fatal("expected subprocess to fail for nonexistent fixture")
	}
	if !strings.Contains(string(out), "not found") {
		t.Errorf("expected 'not found' in output, got: %s", out)
	}
}

func TestFindModuleRoot(t *testing.T) {
	t.Run("returns directory containing go.mod", func(t *testing.T) {
		root := findModuleRoot(t)

		goMod := filepath.Join(root, "go.mod")
		if _, err := os.Stat(goMod); err != nil {
			t.Fatalf("findModuleRoot returned %s which does not contain go.mod: %v", root, err)
		}
	})

	t.Run("returns absolute path", func(t *testing.T) {
		root := findModuleRoot(t)
		if !filepath.IsAbs(root) {
			t.Errorf("findModuleRoot should return absolute path, got: %s", root)
		}
	})

	t.Run("result is consistent across calls", func(t *testing.T) {
		root1 := findModuleRoot(t)
		root2 := findModuleRoot(t)
		if root1 != root2 {
			t.Errorf("findModuleRoot returned different paths: %s vs %s", root1, root2)
		}
	})
}
