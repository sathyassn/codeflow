package workgraph

import (
	"bytes"
	"encoding/json"
	"errors"
	"strings"
	"testing"
)

// Tests for create.go functions (CreateEpic, CreateTask).

func TestCreateEpic_HappyPath(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	data := `{
		"title": "Test Epic",
		"area_type": "INF",
		"work_type": "RFCT",
		"domain": "GENL",
		"summary": "A test epic"
	}`

	var buf bytes.Buffer
	if err := svc.CreateEpic(ctx, data, &buf); err != nil {
		t.Fatalf("CreateEpic error: %v", err)
	}

	var result CreateEpicResult
	if err := json.Unmarshal(buf.Bytes(), &result); err != nil {
		t.Fatalf("unmarshal result: %v", err)
	}

	if !strings.HasPrefix(result.ID, "epic-") {
		t.Errorf("ID = %q, want prefix 'epic-'", result.ID)
	}
	if !strings.HasPrefix(result.FormatID, "INF-EPC-") {
		t.Errorf("FormatID = %q, want prefix 'INF-EPC-'", result.FormatID)
	}
	if result.Title != "Test Epic" {
		t.Errorf("Title = %q, want %q", result.Title, "Test Epic")
	}

	// Verify DB row exists.
	var dbTitle string
	err := svc.DB.QueryRow(ctx, "SELECT title FROM epics WHERE id = ?", result.ID).Scan(&dbTitle)
	if err != nil {
		t.Fatalf("query epic: %v", err)
	}
	if dbTitle != "Test Epic" {
		t.Errorf("DB title = %q, want %q", dbTitle, "Test Epic")
	}
}

func TestCreateEpic_MissingRequired(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	tests := []struct {
		name string
		data string
	}{
		{"missing title", `{"area_type":"INF","work_type":"RFCT","domain":"GENL"}`},
		{"missing area_type", `{"title":"T","work_type":"RFCT","domain":"GENL"}`},
		{"missing work_type", `{"title":"T","area_type":"INF","domain":"GENL"}`},
		{"missing domain", `{"title":"T","area_type":"INF","work_type":"RFCT"}`},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			var buf bytes.Buffer
			err := svc.CreateEpic(ctx, tt.data, &buf)
			if err == nil {
				t.Error("expected error for missing required field")
			}
			if !errors.Is(err, ErrInvalidInput) {
				t.Errorf("error = %v, want ErrInvalidInput", err)
			}
		})
	}
}

func TestCreateEpic_InvalidStatus(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	data := `{"title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL","status":"invalid"}`
	var buf bytes.Buffer
	err := svc.CreateEpic(ctx, data, &buf)
	if err == nil {
		t.Error("expected error for invalid status")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestCreateEpic_InvalidAreaType(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	data := `{"title":"T","area_type":"NOPE","work_type":"RFCT","domain":"GENL"}`
	var buf bytes.Buffer
	err := svc.CreateEpic(ctx, data, &buf)
	if err == nil {
		t.Error("expected error for invalid area_type")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestCreateEpic_SequentialFormatIDs(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var ids []string
	for i := 1; i <= 3; i++ {
		var buf bytes.Buffer
		data := `{"title":"E` + string(rune('0'+i)) + `","area_type":"INF","work_type":"RFCT","domain":"GENL"}`
		if err := svc.CreateEpic(ctx, data, &buf); err != nil {
			t.Fatalf("CreateEpic #%d: %v", i, err)
		}
		var r CreateEpicResult
		json.Unmarshal(buf.Bytes(), &r)
		ids = append(ids, r.FormatID)
	}

	// Should be INF-EPC-001, INF-EPC-002, INF-EPC-003.
	for i, id := range ids {
		want := "INF-EPC-" + strings.Repeat("0", 2) + string(rune('1'+i))
		if id != want {
			t.Errorf("FormatID[%d] = %q, want %q", i, id, want)
		}
	}
}

func TestCreateEpic_InvalidPriority(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	data := `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL","priority":"bogus"}`
	var buf bytes.Buffer
	err := svc.CreateEpic(ctx, data, &buf)
	if err == nil {
		t.Fatal("expected error for invalid priority")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestCreateEpic_WithAllOptionalFields(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	data := `{
		"title": "Full Epic",
		"area_type": "INF",
		"work_type": "RFCT",
		"domain": "GENL",
		"status": "in_progress",
		"priority": "high",
		"summary": "A detailed summary",
		"is_ongoing": true,
		"file_scope": ["src/", "lib/"]
	}`
	var buf bytes.Buffer
	if err := svc.CreateEpic(ctx, data, &buf); err != nil {
		t.Fatalf("CreateEpic with all optional fields: %v", err)
	}

	var result CreateEpicResult
	if err := json.Unmarshal(buf.Bytes(), &result); err != nil {
		t.Fatalf("unmarshal result: %v", err)
	}
	if result.Title != "Full Epic" {
		t.Errorf("title = %q, want %q", result.Title, "Full Epic")
	}

	// Verify optional fields stored.
	var dbStatus, dbPriority string
	var dbIsOngoing bool
	svc.DB.QueryRow(ctx, "SELECT status, priority, is_ongoing FROM epics WHERE id = ?",
		result.ID).Scan(&dbStatus, &dbPriority, &dbIsOngoing)
	if dbStatus != "in_progress" {
		t.Errorf("DB status = %q, want %q", dbStatus, "in_progress")
	}
	if dbPriority != "high" {
		t.Errorf("DB priority = %q, want %q", dbPriority, "high")
	}
	if !dbIsOngoing {
		t.Error("DB is_ongoing should be true")
	}
}

func TestCreateEpic_Defaults(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var buf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &buf)
	var result CreateEpicResult
	json.Unmarshal(buf.Bytes(), &result)

	// Check defaults were applied.
	var status, priority string
	var isOngoing bool
	svc.DB.QueryRow(ctx, "SELECT status, priority, is_ongoing FROM epics WHERE id = ?", result.ID).Scan(&status, &priority, &isOngoing)
	if status != "draft" {
		t.Errorf("default status = %q, want %q", status, "draft")
	}
	if priority != "normal" {
		t.Errorf("default priority = %q, want %q", priority, "normal")
	}
	if isOngoing {
		t.Error("default is_ongoing should be false")
	}
}

func TestCreateTask_HappyPath(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// First create an epic.
	var epicBuf bytes.Buffer
	epicData := `{"title":"Parent Epic","area_type":"INF","work_type":"RFCT","domain":"GENL"}`
	if err := svc.CreateEpic(ctx, epicData, &epicBuf); err != nil {
		t.Fatalf("CreateEpic: %v", err)
	}
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	// Create a task under the epic.
	taskData := `{
		"epic_id": "` + epicResult.ID + `",
		"title": "Test Task",
		"area_type": "INF",
		"work_type": "RFCT",
		"domain": "GENL"
	}`

	var taskBuf bytes.Buffer
	if err := svc.CreateTask(ctx, taskData, &taskBuf); err != nil {
		t.Fatalf("CreateTask: %v", err)
	}

	var taskResult CreateTaskResult
	if err := json.Unmarshal(taskBuf.Bytes(), &taskResult); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if !strings.HasPrefix(taskResult.ID, "task-") {
		t.Errorf("ID = %q, want prefix 'task-'", taskResult.ID)
	}
	if !strings.HasPrefix(taskResult.FormatID, "INF-TSK-001-") {
		t.Errorf("FormatID = %q, want prefix 'INF-TSK-001-'", taskResult.FormatID)
	}
	if taskResult.EpicID != epicResult.ID {
		t.Errorf("EpicID = %q, want %q", taskResult.EpicID, epicResult.ID)
	}
	if taskResult.Title != "Test Task" {
		t.Errorf("Title = %q, want %q", taskResult.Title, "Test Task")
	}
}

func TestCreateTask_InvalidEpicID(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	data := `{"epic_id":"nonexistent","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`
	var buf bytes.Buffer
	err := svc.CreateTask(ctx, data, &buf)
	if err == nil {
		t.Error("expected error for nonexistent epic_id")
	}
	if !errors.Is(err, ErrNotFound) {
		t.Errorf("error = %v, want ErrNotFound", err)
	}
}

func TestCreateTask_MissingRequired(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	tests := []struct {
		name string
		data string
	}{
		{"missing epic_id", `{"title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`},
		{"missing title", `{"epic_id":"x","area_type":"INF","work_type":"RFCT","domain":"GENL"}`},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			var buf bytes.Buffer
			err := svc.CreateTask(ctx, tt.data, &buf)
			if err == nil {
				t.Error("expected error")
			}
		})
	}
}

func TestCreateTask_InvalidStatus(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// Create an epic first.
	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	data := `{"epic_id":"` + epicResult.ID + `","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL","status":"invalid"}`
	var buf bytes.Buffer
	err := svc.CreateTask(ctx, data, &buf)
	if err == nil {
		t.Error("expected error for invalid status")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestCreateTask_InvalidScopePolicy(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// Create epic first.
	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	data := `{"epic_id":"` + epicResult.ID + `","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL","scope_policy":"bogus"}`
	var buf bytes.Buffer
	err := svc.CreateTask(ctx, data, &buf)
	if err == nil {
		t.Fatal("expected error for invalid scope_policy")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestCreateTask_WithOptionalFields(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	data := `{
		"epic_id": "` + epicResult.ID + `",
		"title": "Full Task",
		"area_type": "INF",
		"work_type": "RFCT",
		"domain": "GENL",
		"description": "A detailed description",
		"status": "todo",
		"origin": "planned",
		"scope_policy": "hard",
		"priority": "high",
		"autorun_eligible": true,
		"raise_pr": false,
		"auto_merge": true,
		"target_branch": "develop",
		"acceptance": ["criterion 1", "criterion 2"],
		"tests": ["test_a.sh"],
		"file_scope": ["src/"],
		"scope_root": "src",
		"stage": "dev",
		"stage_status": "pending"
	}`

	var buf bytes.Buffer
	if err := svc.CreateTask(ctx, data, &buf); err != nil {
		t.Fatalf("CreateTask with optional fields: %v", err)
	}

	var result CreateTaskResult
	json.Unmarshal(buf.Bytes(), &result)

	// Verify optional fields in DB.
	var desc, scopePolicy, priority string
	var autorunEligible, raisePR, autoMerge bool
	svc.DB.QueryRow(ctx,
		"SELECT description, scope_policy, priority, autorun_eligible, raise_pr, auto_merge FROM tasks WHERE id = ?",
		result.ID,
	).Scan(&desc, &scopePolicy, &priority, &autorunEligible, &raisePR, &autoMerge)

	if desc != "A detailed description" {
		t.Errorf("description = %q, want %q", desc, "A detailed description")
	}
	if scopePolicy != "hard" {
		t.Errorf("scope_policy = %q, want %q", scopePolicy, "hard")
	}
	if priority != "high" {
		t.Errorf("priority = %q, want %q", priority, "high")
	}
	if !autorunEligible {
		t.Error("autorun_eligible should be true")
	}
	if raisePR {
		t.Error("raise_pr should be false")
	}
	if !autoMerge {
		t.Error("auto_merge should be true")
	}
}

func TestCreateTask_WithEstimate(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	// Valid estimate.
	data := `{"epic_id":"` + epicResult.ID + `","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL","estimate":"M"}`
	var buf bytes.Buffer
	if err := svc.CreateTask(ctx, data, &buf); err != nil {
		t.Fatalf("CreateTask with estimate: %v", err)
	}

	// Invalid estimate.
	data = `{"epic_id":"` + epicResult.ID + `","title":"T2","area_type":"INF","work_type":"RFCT","domain":"GENL","estimate":"HUGE"}`
	err := svc.CreateTask(ctx, data, &bytes.Buffer{})
	if err == nil {
		t.Error("expected error for invalid estimate")
	}
}

func TestCreateTask_InvalidPriority(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// Create an epic first.
	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	data := `{"epic_id":"` + epicResult.ID + `","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL","priority":"bogus"}`
	var buf bytes.Buffer
	err := svc.CreateTask(ctx, data, &buf)
	if err == nil {
		t.Fatal("expected error for invalid priority")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestCreateTask_InvalidOrigin(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	data := `{"epic_id":"` + epicResult.ID + `","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL","origin":"bogus"}`
	var buf bytes.Buffer
	err := svc.CreateTask(ctx, data, &buf)
	if err == nil {
		t.Fatal("expected error for invalid origin")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}
