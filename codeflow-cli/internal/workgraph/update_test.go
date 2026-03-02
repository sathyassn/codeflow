package workgraph

import (
	"bytes"
	"encoding/json"
	"errors"
	"testing"
)

// Tests for update.go functions (UpdateEpic, UpdateTask).

func TestUpdateEpic_HappyPath(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// Create epic.
	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"Original","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	// Update status.
	updateData := `{"id":"` + epicResult.ID + `","status":"in_progress","priority":"high"}`
	var updateBuf bytes.Buffer
	if err := svc.UpdateEpic(ctx, updateData, &updateBuf); err != nil {
		t.Fatalf("UpdateEpic: %v", err)
	}

	var updateResult UpdateEpicResult
	json.Unmarshal(updateBuf.Bytes(), &updateResult)

	if updateResult.ID != epicResult.ID {
		t.Errorf("ID = %q, want %q", updateResult.ID, epicResult.ID)
	}
	// Check updated_fields contains our fields.
	found := map[string]bool{}
	for _, f := range updateResult.UpdatedFields {
		found[f] = true
	}
	if !found["status"] {
		t.Error("updated_fields should contain 'status'")
	}
	if !found["priority"] {
		t.Error("updated_fields should contain 'priority'")
	}

	// Verify DB.
	var dbStatus string
	svc.DB.QueryRow(ctx, "SELECT status FROM epics WHERE id = ?", epicResult.ID).Scan(&dbStatus)
	if dbStatus != "in_progress" {
		t.Errorf("DB status = %q, want %q", dbStatus, "in_progress")
	}
}

func TestUpdateEpic_NoFields(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	// Update with only id, no updatable fields.
	var buf bytes.Buffer
	err := svc.UpdateEpic(ctx, `{"id":"`+epicResult.ID+`"}`, &buf)
	if err == nil {
		t.Error("expected error for no fields to update")
	}
	if !errors.Is(err, ErrInvalidInput) {
		t.Errorf("error = %v, want ErrInvalidInput", err)
	}
}

func TestUpdateEpic_InvalidStatus(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	err := svc.UpdateEpic(ctx, `{"id":"`+epicResult.ID+`","status":"invalid"}`, &bytes.Buffer{})
	if err == nil {
		t.Error("expected error for invalid status")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestUpdateEpic_ByFormatID(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var buf bytes.Buffer
	err := svc.UpdateEpic(ctx, `{"format_id":"`+epicResult.FormatID+`","title":"Updated Title"}`, &buf)
	if err != nil {
		t.Fatalf("UpdateEpic by format_id: %v", err)
	}

	var dbTitle string
	svc.DB.QueryRow(ctx, "SELECT title FROM epics WHERE id = ?", epicResult.ID).Scan(&dbTitle)
	if dbTitle != "Updated Title" {
		t.Errorf("DB title = %q, want %q", dbTitle, "Updated Title")
	}
}

func TestUpdateEpic_WithPrNumber(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	// Update pr_number (non-isString, non-isBool field — exercises the default branch).
	var buf bytes.Buffer
	err := svc.UpdateEpic(ctx, `{"id":"`+epicResult.ID+`","pr_number":42}`, &buf)
	if err != nil {
		t.Fatalf("UpdateEpic with pr_number: %v", err)
	}

	var result UpdateEpicResult
	json.Unmarshal(buf.Bytes(), &result)
	found := false
	for _, f := range result.UpdatedFields {
		if f == "pr_number" {
			found = true
		}
	}
	if !found {
		t.Error("updated_fields should contain 'pr_number'")
	}

	// Verify DB.
	var dbPR int
	svc.DB.QueryRow(ctx, "SELECT pr_number FROM epics WHERE id = ?", epicResult.ID).Scan(&dbPR)
	if dbPR != 42 {
		t.Errorf("DB pr_number = %d, want 42", dbPR)
	}
}

func TestUpdateEpic_StatusChangeEmitsEvent(t *testing.T) {
	t.Parallel()

	svc, ledgerDir := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	// Update status — should emit epic_status_changed event.
	svc.UpdateEpic(ctx, `{"id":"`+epicResult.ID+`","status":"in_progress"}`, &bytes.Buffer{})

	// Read ledger file to verify event was written.
	entries, _ := readLedgerEvents(t, ledgerDir, "work-graph.jsonl")
	foundStatusChange := false
	for _, e := range entries {
		if e["event"] == "epic_status_changed" {
			foundStatusChange = true
			if e["epic_id"] != epicResult.ID {
				t.Errorf("epic_id = %v, want %v", e["epic_id"], epicResult.ID)
			}
			if e["old_status"] != "draft" {
				t.Errorf("old_status = %v, want draft", e["old_status"])
			}
			if e["new_status"] != "in_progress" {
				t.Errorf("new_status = %v, want in_progress", e["new_status"])
			}
		}
	}
	if !foundStatusChange {
		t.Error("expected epic_status_changed event in ledger")
	}
}

func TestUpdateEpic_SameStatusNoEvent(t *testing.T) {
	t.Parallel()

	svc, ledgerDir := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	// Update status to the same value — no event should be emitted.
	svc.UpdateEpic(ctx, `{"id":"`+epicResult.ID+`","status":"draft"}`, &bytes.Buffer{})

	entries, _ := readLedgerEvents(t, ledgerDir, "work-graph.jsonl")
	for _, e := range entries {
		if e["event"] == "epic_status_changed" {
			t.Error("unexpected epic_status_changed event when status unchanged")
		}
	}
}

func TestUpdateEpic_WithFileScope(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	// Update with file_scope as JSON array.
	var buf bytes.Buffer
	err := svc.UpdateEpic(ctx, `{"id":"`+epicResult.ID+`","file_scope":["src/","lib/"]}`, &buf)
	if err != nil {
		t.Fatalf("UpdateEpic with file_scope: %v", err)
	}

	var result UpdateEpicResult
	json.Unmarshal(buf.Bytes(), &result)
	found := false
	for _, f := range result.UpdatedFields {
		if f == "file_scope" {
			found = true
		}
	}
	if !found {
		t.Error("updated_fields should contain 'file_scope'")
	}
}

func TestUpdateEpic_WithIsOngoing(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	// Update is_ongoing (non-string, non-bool field in epic specs — exercises the else branch).
	var buf bytes.Buffer
	err := svc.UpdateEpic(ctx, `{"id":"`+epicResult.ID+`","is_ongoing":true}`, &buf)
	if err != nil {
		t.Fatalf("UpdateEpic with is_ongoing: %v", err)
	}

	var result UpdateEpicResult
	json.Unmarshal(buf.Bytes(), &result)
	found := false
	for _, f := range result.UpdatedFields {
		if f == "is_ongoing" {
			found = true
		}
	}
	if !found {
		t.Error("updated_fields should contain 'is_ongoing'")
	}

	// Verify DB.
	var isOngoing bool
	svc.DB.QueryRow(ctx, "SELECT is_ongoing FROM epics WHERE id = ?", epicResult.ID).Scan(&isOngoing)
	if !isOngoing {
		t.Error("DB is_ongoing should be true")
	}
}

func TestUpdateTask_HappyPath(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// Create epic + task.
	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	// Update task status to in_progress.
	var updateBuf bytes.Buffer
	err := svc.UpdateTask(ctx, `{"id":"`+taskResult.ID+`","status":"in_progress"}`, &updateBuf)
	if err != nil {
		t.Fatalf("UpdateTask: %v", err)
	}

	// Verify started_at was set.
	var startedAt *string
	svc.DB.QueryRow(ctx, "SELECT started_at FROM tasks WHERE id = ?", taskResult.ID).Scan(&startedAt)
	if startedAt == nil {
		t.Error("started_at should be set when status transitions to in_progress")
	}
}

func TestUpdateTask_CompletedAt(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	// Update to complete.
	var updateBuf bytes.Buffer
	err := svc.UpdateTask(ctx, `{"id":"`+taskResult.ID+`","status":"complete"}`, &updateBuf)
	if err != nil {
		t.Fatalf("UpdateTask to complete: %v", err)
	}

	var completedAt *string
	svc.DB.QueryRow(ctx, "SELECT completed_at FROM tasks WHERE id = ?", taskResult.ID).Scan(&completedAt)
	if completedAt == nil {
		t.Error("completed_at should be set when status transitions to complete")
	}
}

func TestUpdateTask_NoFields(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	err := svc.UpdateTask(ctx, `{"id":"`+taskResult.ID+`"}`, &bytes.Buffer{})
	if err == nil {
		t.Error("expected error for no fields to update")
	}
	if !errors.Is(err, ErrInvalidInput) {
		t.Errorf("error = %v, want ErrInvalidInput", err)
	}
}

func TestUpdateTask_InvalidStage(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	var buf bytes.Buffer
	err := svc.UpdateTask(ctx, `{"id":"`+taskResult.ID+`","stage":"bogus"}`, &buf)
	if err == nil {
		t.Fatal("expected error for invalid stage")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestUpdateTask_WithPrNumber(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	// Update pr_number (default branch in task switch — not isString, not isBool).
	var buf bytes.Buffer
	err := svc.UpdateTask(ctx, `{"id":"`+taskResult.ID+`","pr_number":99}`, &buf)
	if err != nil {
		t.Fatalf("UpdateTask with pr_number: %v", err)
	}

	var result UpdateTaskResult
	json.Unmarshal(buf.Bytes(), &result)
	found := false
	for _, f := range result.UpdatedFields {
		if f == "pr_number" {
			found = true
		}
	}
	if !found {
		t.Error("updated_fields should contain 'pr_number'")
	}
}

func TestUpdateTask_WithBoolFields(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	// Update with bool and string fields.
	updateData := `{
		"id": "` + taskResult.ID + `",
		"autorun_eligible": true,
		"raise_pr": false,
		"branch": "feat/test",
		"scope_policy": "hard",
		"stage": "dev",
		"stage_status": "in_progress"
	}`
	var updateBuf bytes.Buffer
	if err := svc.UpdateTask(ctx, updateData, &updateBuf); err != nil {
		t.Fatalf("UpdateTask with bool fields: %v", err)
	}

	var result UpdateTaskResult
	json.Unmarshal(updateBuf.Bytes(), &result)

	found := map[string]bool{}
	for _, f := range result.UpdatedFields {
		found[f] = true
	}
	if !found["autorun_eligible"] {
		t.Error("updated_fields should contain 'autorun_eligible'")
	}
	if !found["branch"] {
		t.Error("updated_fields should contain 'branch'")
	}
}

func TestUpdateTask_InvalidScopePolicy(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	err := svc.UpdateTask(ctx, `{"id":"`+taskResult.ID+`","scope_policy":"invalid"}`, &bytes.Buffer{})
	if err == nil {
		t.Error("expected error for invalid scope_policy")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestUpdateTask_InvalidEstimate(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	err := svc.UpdateTask(ctx, `{"id":"`+taskResult.ID+`","estimate":"HUGE"}`, &bytes.Buffer{})
	if err == nil {
		t.Error("expected error for invalid estimate FK")
	}
}

func TestUpdateTask_StatusChangeEmitsEvent(t *testing.T) {
	t.Parallel()

	svc, ledgerDir := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	svc.UpdateTask(ctx, `{"id":"`+taskResult.ID+`","status":"in_progress"}`, &bytes.Buffer{})

	entries, _ := readLedgerEvents(t, ledgerDir, "work-graph.jsonl")
	foundStatusChange := false
	for _, e := range entries {
		if e["event"] == "task_status_changed" {
			foundStatusChange = true
			// Must use "task_id" not "id" per sync.go compatibility.
			if e["task_id"] != taskResult.ID {
				t.Errorf("task_id = %v, want %v", e["task_id"], taskResult.ID)
			}
		}
	}
	if !foundStatusChange {
		t.Error("expected task_status_changed event in ledger")
	}
}

func TestUpdateTask_WithFileScopeArray(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// Create epic + task.
	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	// Update file_scope with a JSON array (exercises the non-string isString branch in UpdateTask).
	var buf bytes.Buffer
	err := svc.UpdateTask(ctx, `{"id":"`+taskResult.ID+`","file_scope":["src/main.go","lib/"]}`, &buf)
	if err != nil {
		t.Fatalf("UpdateTask with file_scope array: %v", err)
	}

	var result UpdateTaskResult
	json.Unmarshal(buf.Bytes(), &result)
	found := false
	for _, f := range result.UpdatedFields {
		if f == "file_scope" {
			found = true
		}
	}
	if !found {
		t.Error("updated_fields should contain 'file_scope'")
	}
}

