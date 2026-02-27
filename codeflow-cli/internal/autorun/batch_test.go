package autorun

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestParseBatchFile_ValidYAML(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	batchPath := filepath.Join(dir, "valid.yaml")

	content := `
name: test-batch
max_workers: 2
auto_merge: false
target: integration
tasks:
  - id: task-001
  - id: task-002
    depends_on:
      - task-001
  - id: task-003
    depends_on:
      - task-001
`
	if err := os.WriteFile(batchPath, []byte(content), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	batch, err := ParseBatchFile(batchPath)
	if err != nil {
		t.Fatalf("ParseBatchFile: %v", err)
	}

	if batch.Name != "test-batch" {
		t.Errorf("Name = %q, want %q", batch.Name, "test-batch")
	}
	if batch.MaxWorkers != 2 {
		t.Errorf("MaxWorkers = %d, want 2", batch.MaxWorkers)
	}
	if batch.AutoMerge {
		t.Error("AutoMerge = true, want false")
	}
	if batch.Target != "integration" {
		t.Errorf("Target = %q, want %q", batch.Target, "integration")
	}
	if len(batch.Tasks) != 3 {
		t.Errorf("len(Tasks) = %d, want 3", len(batch.Tasks))
	}

	// Verify topological order: task-001 must come before task-002 and task-003.
	orderMap := make(map[string]int)
	for i, id := range batch.Order {
		orderMap[id] = i
	}
	if orderMap["task-001"] >= orderMap["task-002"] {
		t.Errorf("task-001 (pos %d) should come before task-002 (pos %d)", orderMap["task-001"], orderMap["task-002"])
	}
	if orderMap["task-001"] >= orderMap["task-003"] {
		t.Errorf("task-001 (pos %d) should come before task-003 (pos %d)", orderMap["task-001"], orderMap["task-003"])
	}
}

func TestParseBatchFile_InvalidSyntax(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	batchPath := filepath.Join(dir, "invalid.yaml")

	content := `
name: broken
tasks:
  - id: task-001
    depends_on: [
`
	if err := os.WriteFile(batchPath, []byte(content), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	_, err := ParseBatchFile(batchPath)
	if err == nil {
		t.Fatal("expected error for invalid YAML, got nil")
	}
	if !errors.Is(err, ErrInvalidBatch) {
		t.Errorf("error = %v, want ErrInvalidBatch", err)
	}
}

func TestParseBatchFile_MissingTasks(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	batchPath := filepath.Join(dir, "empty.yaml")

	content := `
name: empty-batch
tasks: []
`
	if err := os.WriteFile(batchPath, []byte(content), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	_, err := ParseBatchFile(batchPath)
	if err == nil {
		t.Fatal("expected error for empty tasks, got nil")
	}
	if !errors.Is(err, ErrInvalidBatch) {
		t.Errorf("error = %v, want ErrInvalidBatch", err)
	}
}

func TestParseBatchFile_DependencyCycle(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	batchPath := filepath.Join(dir, "cycle.yaml")

	content := `
name: cycle-batch
tasks:
  - id: task-a
    depends_on:
      - task-b
  - id: task-b
    depends_on:
      - task-c
  - id: task-c
    depends_on:
      - task-a
`
	if err := os.WriteFile(batchPath, []byte(content), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	_, err := ParseBatchFile(batchPath)
	if err == nil {
		t.Fatal("expected error for dependency cycle, got nil")
	}
	if !errors.Is(err, ErrDependencyCycle) {
		t.Errorf("error = %v, want ErrDependencyCycle", err)
	}
}

func TestParseBatchFile_MissingDependency(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	batchPath := filepath.Join(dir, "missing-dep.yaml")

	content := `
name: missing-dep
tasks:
  - id: task-001
    depends_on:
      - nonexistent
`
	if err := os.WriteFile(batchPath, []byte(content), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	_, err := ParseBatchFile(batchPath)
	if err == nil {
		t.Fatal("expected error for missing dependency, got nil")
	}
	if !errors.Is(err, ErrMissingTask) {
		t.Errorf("error = %v, want ErrMissingTask", err)
	}
}

func TestParseBatchFile_DefaultMaxWorkers(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	batchPath := filepath.Join(dir, "defaults.yaml")

	content := `
tasks:
  - id: task-001
`
	if err := os.WriteFile(batchPath, []byte(content), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	batch, err := ParseBatchFile(batchPath)
	if err != nil {
		t.Fatalf("ParseBatchFile: %v", err)
	}

	if batch.MaxWorkers != DefaultMaxWorkers {
		t.Errorf("MaxWorkers = %d, want default %d", batch.MaxWorkers, DefaultMaxWorkers)
	}

	// Name should default to filename without extension.
	if batch.Name != "defaults" {
		t.Errorf("Name = %q, want %q", batch.Name, "defaults")
	}
}

func TestParseBatchFile_ProtectedBranchAutoMerge(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name   string
		target string
	}{
		{"main", "main"},
		{"master", "master"},
		{"production", "production"},
		{"release branch", "release/v1.0"},
		{"empty target defaults to main", ""},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			dir := t.TempDir()
			batchPath := filepath.Join(dir, "protected.yaml")

			content := "auto_merge: true\n"
			if tc.target != "" {
				content += "target: " + tc.target + "\n"
			}
			content += "tasks:\n  - id: task-001\n"

			if err := os.WriteFile(batchPath, []byte(content), 0o644); err != nil {
				t.Fatalf("writing batch file: %v", err)
			}

			_, err := ParseBatchFile(batchPath)
			if err == nil {
				t.Fatal("expected error for auto_merge + protected branch, got nil")
			}
			if !errors.Is(err, ErrProtectedMerge) {
				t.Errorf("error = %v, want ErrProtectedMerge", err)
			}
		})
	}
}

func TestParseBatchFile_DuplicateTaskID(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	batchPath := filepath.Join(dir, "duplicate.yaml")

	content := `
tasks:
  - id: task-001
  - id: task-001
`
	if err := os.WriteFile(batchPath, []byte(content), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	_, err := ParseBatchFile(batchPath)
	if err == nil {
		t.Fatal("expected error for duplicate task ID, got nil")
	}
	if !errors.Is(err, ErrInvalidBatch) {
		t.Errorf("error = %v, want ErrInvalidBatch", err)
	}
}

func TestParseBatchFile_SelfDependency(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	batchPath := filepath.Join(dir, "self-dep.yaml")

	content := `
tasks:
  - id: task-001
    depends_on:
      - task-001
`
	if err := os.WriteFile(batchPath, []byte(content), 0o644); err != nil {
		t.Fatalf("writing batch file: %v", err)
	}

	_, err := ParseBatchFile(batchPath)
	if err == nil {
		t.Fatal("expected error for self dependency, got nil")
	}
	if !errors.Is(err, ErrDependencyCycle) {
		t.Errorf("error = %v, want ErrDependencyCycle", err)
	}
}

func TestParseBatchFile_FileNotFound(t *testing.T) {
	t.Parallel()

	_, err := ParseBatchFile("/nonexistent/batch.yaml")
	if err == nil {
		t.Fatal("expected error for missing file, got nil")
	}
}

func TestTopologicalSort_DiamondDependency(t *testing.T) {
	t.Parallel()

	// Diamond: A -> B, A -> C, B -> D, C -> D
	tasks := []TaskSpec{
		{ID: "A"},
		{ID: "B", DependsOn: []string{"A"}},
		{ID: "C", DependsOn: []string{"A"}},
		{ID: "D", DependsOn: []string{"B", "C"}},
	}

	order, err := topologicalSort(tasks)
	if err != nil {
		t.Fatalf("topologicalSort: %v", err)
	}

	if len(order) != 4 {
		t.Fatalf("len(order) = %d, want 4", len(order))
	}

	pos := make(map[string]int)
	for i, id := range order {
		pos[id] = i
	}

	// A must come first, D must come last.
	if pos["A"] != 0 {
		t.Errorf("A at position %d, want 0", pos["A"])
	}
	if pos["D"] != 3 {
		t.Errorf("D at position %d, want 3", pos["D"])
	}
	if pos["B"] >= pos["D"] {
		t.Error("B should come before D")
	}
	if pos["C"] >= pos["D"] {
		t.Error("C should come before D")
	}
}
