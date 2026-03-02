package workgraph

import (
	"bytes"
	"encoding/json"
	"errors"
	"testing"
)

// Tests for query.go functions (Query with various modes).

func TestQuery_GetEpic(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"Query Epic","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var buf bytes.Buffer
	params := QueryParams{Mode: "get-epic", ID: epicResult.ID}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query get-epic: %v", err)
	}

	var result map[string]any
	if err := json.Unmarshal(buf.Bytes(), &result); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if result["title"] != "Query Epic" {
		t.Errorf("title = %v, want %q", result["title"], "Query Epic")
	}
}

func TestQuery_GetEpic_NotFound(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var buf bytes.Buffer
	params := QueryParams{Mode: "get-epic", ID: "nonexistent"}
	err := svc.Query(ctx, params, &buf)
	if err == nil {
		t.Error("expected error for nonexistent epic")
	}
	if !errors.Is(err, ErrNotFound) {
		t.Errorf("error = %v, want ErrNotFound", err)
	}
}

func TestQuery_GetEpic_MissingID(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var buf bytes.Buffer
	err := svc.Query(ctx, QueryParams{Mode: "get-epic"}, &buf)
	if err == nil {
		t.Error("expected error for missing id")
	}
	if !errors.Is(err, ErrInvalidInput) {
		t.Errorf("error = %v, want ErrInvalidInput", err)
	}
}

func TestQuery_GetEpic_ByFormatID(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"FmtQuery","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var buf bytes.Buffer
	params := QueryParams{Mode: "get-epic", FormatID: epicResult.FormatID}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query get-epic by format_id: %v", err)
	}

	var result map[string]any
	json.Unmarshal(buf.Bytes(), &result)
	if result["title"] != "FmtQuery" {
		t.Errorf("title = %v, want %q", result["title"], "FmtQuery")
	}
}

func TestQuery_GetTask(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"Query Task","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	var buf bytes.Buffer
	params := QueryParams{Mode: "get-task", FormatID: taskResult.FormatID}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query get-task: %v", err)
	}

	var result map[string]any
	json.Unmarshal(buf.Bytes(), &result)
	if result["title"] != "Query Task" {
		t.Errorf("title = %v, want %q", result["title"], "Query Task")
	}
}

func TestQuery_GetTask_ByFormatID(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	var taskBuf bytes.Buffer
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"FmtTask","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &taskBuf)
	var taskResult CreateTaskResult
	json.Unmarshal(taskBuf.Bytes(), &taskResult)

	var buf bytes.Buffer
	params := QueryParams{Mode: "get-task", FormatID: taskResult.FormatID}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query get-task by format_id: %v", err)
	}

	var result map[string]any
	json.Unmarshal(buf.Bytes(), &result)
	if result["title"] != "FmtTask" {
		t.Errorf("title = %v, want %q", result["title"], "FmtTask")
	}
}

func TestQuery_GetTask_NotFound(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var buf bytes.Buffer
	err := svc.Query(ctx, QueryParams{Mode: "get-task", ID: "nonexistent"}, &buf)
	if err == nil {
		t.Error("expected error for nonexistent task")
	}
	if !errors.Is(err, ErrNotFound) {
		t.Errorf("error = %v, want ErrNotFound", err)
	}
}

func TestQuery_GetTask_MissingID(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var buf bytes.Buffer
	err := svc.Query(ctx, QueryParams{Mode: "get-task"}, &buf)
	if err == nil {
		t.Error("expected error for missing id")
	}
	if !errors.Is(err, ErrInvalidInput) {
		t.Errorf("error = %v, want ErrInvalidInput", err)
	}
}

func TestQuery_ListEpics(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// Create 3 epics.
	for i := 0; i < 3; i++ {
		var buf bytes.Buffer
		svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &buf)
	}

	var buf bytes.Buffer
	params := QueryParams{Mode: "list-epics", Area: "INF", Limit: 10}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query list-epics: %v", err)
	}

	var results []map[string]any
	json.Unmarshal(buf.Bytes(), &results)
	if len(results) != 3 {
		t.Errorf("got %d epics, want 3", len(results))
	}
}

func TestQuery_ListEpics_WithFilters(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// Create epics in different areas.
	svc.CreateEpic(ctx, `{"title":"E1","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &bytes.Buffer{})
	svc.CreateEpic(ctx, `{"title":"E2","area_type":"FRT","work_type":"FEAT","domain":"GENL"}`, &bytes.Buffer{})

	var buf bytes.Buffer
	params := QueryParams{Mode: "list-epics", Area: "FRT", Limit: 10}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query list-epics: %v", err)
	}

	var results []map[string]any
	json.Unmarshal(buf.Bytes(), &results)
	if len(results) != 1 {
		t.Errorf("got %d epics for area=FRT, want 1", len(results))
	}
}

func TestQuery_ListEpics_AllFilters(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &bytes.Buffer{})

	var buf bytes.Buffer
	params := QueryParams{
		Mode:     "list-epics",
		Area:     "INF",
		Status:   "draft",
		Domain:   "GENL",
		WorkType: "RFCT",
		Limit:    10,
	}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query list-epics all filters: %v", err)
	}

	var results []map[string]any
	json.Unmarshal(buf.Bytes(), &results)
	if len(results) != 1 {
		t.Errorf("got %d epics, want 1", len(results))
	}
}

func TestQuery_ListEpics_Empty(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var buf bytes.Buffer
	params := QueryParams{Mode: "list-epics", Limit: 10}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query list-epics empty: %v", err)
	}

	var results []map[string]any
	json.Unmarshal(buf.Bytes(), &results)
	if len(results) != 0 {
		t.Errorf("got %d epics, want 0", len(results))
	}
}

func TestQuery_ListTasks(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	// Create 2 tasks.
	for i := 0; i < 2; i++ {
		var buf bytes.Buffer
		svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &buf)
	}

	var buf bytes.Buffer
	params := QueryParams{Mode: "list-tasks", EpicID: epicResult.ID, Limit: 10}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query list-tasks: %v", err)
	}

	var results []map[string]any
	json.Unmarshal(buf.Bytes(), &results)
	if len(results) != 2 {
		t.Errorf("got %d tasks, want 2", len(results))
	}
}

func TestQuery_ListTasks_WithStatusFilter(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// Create epic + two tasks with different statuses.
	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"Todo","area_type":"INF","work_type":"RFCT","domain":"GENL","status":"todo"}`, &bytes.Buffer{})
	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"Done","area_type":"INF","work_type":"RFCT","domain":"GENL","status":"complete"}`, &bytes.Buffer{})

	// Query only "todo" tasks.
	var buf bytes.Buffer
	params := QueryParams{Mode: "list-tasks", EpicID: epicResult.ID, Status: "todo"}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query list-tasks with status filter: %v", err)
	}

	var results []map[string]any
	json.Unmarshal(buf.Bytes(), &results)
	if len(results) != 1 {
		t.Fatalf("expected 1 task, got %d", len(results))
	}
	if results[0]["title"] != "Todo" {
		t.Errorf("title = %v, want %q", results[0]["title"], "Todo")
	}
}

func TestQuery_ListTasks_Limit(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	for i := 0; i < 5; i++ {
		svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &bytes.Buffer{})
	}

	var buf bytes.Buffer
	params := QueryParams{Mode: "list-tasks", Limit: 2}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query list-tasks: %v", err)
	}

	var results []map[string]any
	json.Unmarshal(buf.Bytes(), &results)
	if len(results) != 2 {
		t.Errorf("got %d tasks with limit=2, want 2", len(results))
	}
}

func TestQuery_ListTasks_AllFilters(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var epicBuf bytes.Buffer
	svc.CreateEpic(ctx, `{"title":"E","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &epicBuf)
	var epicResult CreateEpicResult
	json.Unmarshal(epicBuf.Bytes(), &epicResult)

	svc.CreateTask(ctx, `{"epic_id":"`+epicResult.ID+`","title":"T","area_type":"INF","work_type":"RFCT","domain":"GENL"}`, &bytes.Buffer{})

	var buf bytes.Buffer
	params := QueryParams{
		Mode:     "list-tasks",
		EpicID:   epicResult.ID,
		Area:     "INF",
		Status:   "todo",
		Domain:   "GENL",
		WorkType: "RFCT",
		Limit:    10,
	}
	if err := svc.Query(ctx, params, &buf); err != nil {
		t.Fatalf("Query list-tasks all filters: %v", err)
	}

	var results []map[string]any
	json.Unmarshal(buf.Bytes(), &results)
	if len(results) != 1 {
		t.Errorf("got %d tasks, want 1", len(results))
	}
}

func TestQuery_InvalidMode(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	var buf bytes.Buffer
	err := svc.Query(ctx, QueryParams{Mode: "invalid"}, &buf)
	if err == nil {
		t.Error("expected error for invalid mode")
	}
	if !errors.Is(err, ErrInvalidInput) {
		t.Errorf("error = %v, want ErrInvalidInput", err)
	}
}
