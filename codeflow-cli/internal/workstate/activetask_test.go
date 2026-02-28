package workstate

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestSetActiveTask(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		task ActiveTask
	}{
		{
			name: "minimal fields",
			task: ActiveTask{
				TaskID:   "task-abc123",
				FormatID: "INF-TSK-001-001",
			},
		},
		{
			name: "all fields",
			task: ActiveTask{
				TaskID:       "task-full",
				FormatID:     "INF-TSK-002-001",
				EpicFormatID: "INF-EPC-002",
				Title:        "Test task",
				Status:       "in_progress",
				Branch:       "feat/test",
				SessionID:    "ses-123",
				CreatedAt:    "2026-01-01T00:00:00Z",
				UpdatedAt:    "2026-01-01T01:00:00Z",
				CurrentStage: "WS-DEV",
				TeamName:     "test-team",
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			dir := t.TempDir()

			if err := SetActiveTask(dir, tt.task); err != nil {
				t.Fatalf("SetActiveTask() error = %v", err)
			}

			// Verify file exists and contains valid JSON.
			data, err := os.ReadFile(filepath.Join(dir, ActiveTaskFile))
			if err != nil {
				t.Fatalf("reading task file: %v", err)
			}

			var got ActiveTask
			if err := json.Unmarshal(data, &got); err != nil {
				t.Fatalf("unmarshaling task: %v", err)
			}

			if got.TaskID != tt.task.TaskID {
				t.Errorf("TaskID = %q, want %q", got.TaskID, tt.task.TaskID)
			}
			if got.FormatID != tt.task.FormatID {
				t.Errorf("FormatID = %q, want %q", got.FormatID, tt.task.FormatID)
			}
		})
	}
}

func TestSetActiveTask_CreatesDirectory(t *testing.T) {
	t.Parallel()

	base := t.TempDir()
	nested := filepath.Join(base, "deep", "nested", "dir")

	task := ActiveTask{TaskID: "task-nested", FormatID: "INF-TSK-001-001"}
	if err := SetActiveTask(nested, task); err != nil {
		t.Fatalf("SetActiveTask() error = %v", err)
	}

	// Verify file was created in the nested directory.
	if _, err := os.Stat(filepath.Join(nested, ActiveTaskFile)); err != nil {
		t.Fatalf("file not found in nested dir: %v", err)
	}
}

func TestSetActiveTask_OverwritesExisting(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()

	first := ActiveTask{TaskID: "task-first", FormatID: "F-001"}
	if err := SetActiveTask(dir, first); err != nil {
		t.Fatalf("first SetActiveTask() error = %v", err)
	}

	second := ActiveTask{TaskID: "task-second", FormatID: "F-002"}
	if err := SetActiveTask(dir, second); err != nil {
		t.Fatalf("second SetActiveTask() error = %v", err)
	}

	got, err := GetActiveTask(dir)
	if err != nil {
		t.Fatalf("GetActiveTask() error = %v", err)
	}

	if got.TaskID != "task-second" {
		t.Errorf("TaskID = %q, want %q", got.TaskID, "task-second")
	}
}

func TestSetActiveTask_ReadOnlyDir(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	// Create the directory but make it read-only so WriteFile fails.
	target := filepath.Join(dir, "readonly")
	if err := os.MkdirAll(target, 0o555); err != nil {
		t.Fatalf("creating readonly dir: %v", err)
	}

	task := ActiveTask{TaskID: "task-fail", FormatID: "F-001"}
	err := SetActiveTask(target, task)
	if err == nil {
		t.Fatal("SetActiveTask() expected error for read-only dir, got nil")
	}
}

func TestSetActiveTask_MkdirAllFails(t *testing.T) {
	t.Parallel()

	// Use /dev/null as the parent path -- cannot create subdirectory under it.
	badPath := filepath.Join("/dev/null", "subdir")

	task := ActiveTask{TaskID: "task-fail", FormatID: "F-001"}
	err := SetActiveTask(badPath, task)
	if err == nil {
		t.Fatal("SetActiveTask() expected error for invalid parent, got nil")
	}
}

func TestGetActiveTask(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	want := ActiveTask{
		TaskID:       "task-get",
		FormatID:     "INF-TSK-003-001",
		EpicFormatID: "INF-EPC-003",
		Title:        "Get test",
		Status:       "in_progress",
		Branch:       "feat/get",
		SessionID:    "ses-456",
	}

	if err := SetActiveTask(dir, want); err != nil {
		t.Fatalf("SetActiveTask() error = %v", err)
	}

	got, err := GetActiveTask(dir)
	if err != nil {
		t.Fatalf("GetActiveTask() error = %v", err)
	}

	if got.TaskID != want.TaskID {
		t.Errorf("TaskID = %q, want %q", got.TaskID, want.TaskID)
	}
	if got.FormatID != want.FormatID {
		t.Errorf("FormatID = %q, want %q", got.FormatID, want.FormatID)
	}
	if got.EpicFormatID != want.EpicFormatID {
		t.Errorf("EpicFormatID = %q, want %q", got.EpicFormatID, want.EpicFormatID)
	}
	if got.Title != want.Title {
		t.Errorf("Title = %q, want %q", got.Title, want.Title)
	}
	if got.Status != want.Status {
		t.Errorf("Status = %q, want %q", got.Status, want.Status)
	}
	if got.Branch != want.Branch {
		t.Errorf("Branch = %q, want %q", got.Branch, want.Branch)
	}
	if got.SessionID != want.SessionID {
		t.Errorf("SessionID = %q, want %q", got.SessionID, want.SessionID)
	}
}

func TestGetActiveTask_ErrNoActiveTask(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()

	_, err := GetActiveTask(dir)
	if !errors.Is(err, ErrNoActiveTask) {
		t.Fatalf("GetActiveTask() error = %v, want %v", err, ErrNoActiveTask)
	}
}

func TestGetActiveTask_InvalidJSON(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, ActiveTaskFile), []byte("{bad json"), 0o644); err != nil {
		t.Fatalf("writing invalid file: %v", err)
	}

	_, err := GetActiveTask(dir)
	if err == nil {
		t.Fatal("GetActiveTask() expected error for invalid JSON, got nil")
	}
	if errors.Is(err, ErrNoActiveTask) {
		t.Fatal("GetActiveTask() should not return ErrNoActiveTask for invalid JSON")
	}
}

func TestClearActiveTask(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	task := ActiveTask{TaskID: "task-clear", FormatID: "C-001"}
	if err := SetActiveTask(dir, task); err != nil {
		t.Fatalf("SetActiveTask() error = %v", err)
	}

	// Verify file exists before clearing.
	if _, err := os.Stat(filepath.Join(dir, ActiveTaskFile)); err != nil {
		t.Fatalf("file should exist before clear: %v", err)
	}

	if err := ClearActiveTask(dir); err != nil {
		t.Fatalf("ClearActiveTask() error = %v", err)
	}

	// Verify file was removed.
	if _, err := os.Stat(filepath.Join(dir, ActiveTaskFile)); !os.IsNotExist(err) {
		t.Fatal("file should not exist after clear")
	}
}

func TestClearActiveTask_NonExistent(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()

	// Clearing a non-existent file should not error.
	if err := ClearActiveTask(dir); err != nil {
		t.Fatalf("ClearActiveTask() error = %v, want nil", err)
	}
}

func TestClearActiveTask_PermissionError(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	taskPath := filepath.Join(dir, ActiveTaskFile)

	// Create the file.
	if err := os.WriteFile(taskPath, []byte(`{}`), 0o644); err != nil {
		t.Fatalf("writing file: %v", err)
	}

	// Make directory read-only so Remove fails.
	if err := os.Chmod(dir, 0o555); err != nil {
		t.Fatalf("chmod: %v", err)
	}
	t.Cleanup(func() {
		_ = os.Chmod(dir, 0o755)
	})

	err := ClearActiveTask(dir)
	if err == nil {
		t.Fatal("ClearActiveTask() expected error for permission denied, got nil")
	}
}

func TestSetGetClearRoundTrip(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	want := ActiveTask{
		TaskID:       "task-roundtrip",
		FormatID:     "RT-001",
		EpicFormatID: "RT-EPC-001",
		Title:        "Round trip test",
		Status:       "todo",
		Branch:       "feat/roundtrip",
		SessionID:    "ses-rt",
		CurrentStage: "WS-DEV",
		TeamName:     "rt-team",
	}

	// Set.
	if err := SetActiveTask(dir, want); err != nil {
		t.Fatalf("SetActiveTask() error = %v", err)
	}

	// Get.
	got, err := GetActiveTask(dir)
	if err != nil {
		t.Fatalf("GetActiveTask() error = %v", err)
	}
	if got.TaskID != want.TaskID {
		t.Errorf("TaskID = %q, want %q", got.TaskID, want.TaskID)
	}
	if got.CurrentStage != want.CurrentStage {
		t.Errorf("CurrentStage = %q, want %q", got.CurrentStage, want.CurrentStage)
	}
	if got.TeamName != want.TeamName {
		t.Errorf("TeamName = %q, want %q", got.TeamName, want.TeamName)
	}

	// Clear.
	if err := ClearActiveTask(dir); err != nil {
		t.Fatalf("ClearActiveTask() error = %v", err)
	}

	// Get again should fail.
	_, err = GetActiveTask(dir)
	if !errors.Is(err, ErrNoActiveTask) {
		t.Fatalf("GetActiveTask() after clear: error = %v, want %v", err, ErrNoActiveTask)
	}
}

func TestActiveTaskJSONTags(t *testing.T) {
	t.Parallel()

	// Verify JSON tags match the expected active-task.json schema.
	task := ActiveTask{
		TaskID:       "task-tags",
		FormatID:     "T-001",
		EpicFormatID: "E-001",
		Title:        "Tag test",
		Status:       "in_progress",
		Branch:       "feat/tags",
		SessionID:    "ses-tags",
	}

	data, err := json.Marshal(task)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}

	var m map[string]any
	if err := json.Unmarshal(data, &m); err != nil {
		t.Fatalf("Unmarshal() error = %v", err)
	}

	// Check expected keys exist with correct names matching active-task.json schema.
	expectedKeys := []string{"task_id", "task_format_id", "epic_format_id", "title", "status", "branch", "session_id"}
	for _, key := range expectedKeys {
		if _, ok := m[key]; !ok {
			t.Errorf("expected JSON key %q not found in output", key)
		}
	}

	// Check omitempty fields are absent when zero.
	taskMinimal := ActiveTask{TaskID: "task-min"}
	data, err = json.Marshal(taskMinimal)
	if err != nil {
		t.Fatalf("Marshal(minimal) error = %v", err)
	}

	var mMin map[string]any
	if err := json.Unmarshal(data, &mMin); err != nil {
		t.Fatalf("Unmarshal(minimal) error = %v", err)
	}

	omitemptyKeys := []string{"current_stage", "team_name", "created_at", "updated_at"}
	for _, key := range omitemptyKeys {
		if _, ok := mMin[key]; ok {
			t.Errorf("omitempty key %q should be absent when empty, but found in output", key)
		}
	}
}
