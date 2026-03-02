package main

import (
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/codeflow/codeflow-cli/internal/workgraph"
)

// --- Command structure tests ---

func TestNewWorkgraphCmd(t *testing.T) {
	t.Parallel()

	cmd := newWorkgraphCmd()
	if cmd.Use != "workgraph" {
		t.Errorf("Use = %q, want %q", cmd.Use, "workgraph")
	}
	if cmd.Short == "" {
		t.Error("workgraph command should have a short description")
	}
}

func TestWorkgraphCmd_Subcommands(t *testing.T) {
	t.Parallel()

	cmd := newWorkgraphCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}

	expected := []string{"create-epic", "create-task", "update-epic", "update-task", "query"}
	for _, name := range expected {
		if !subcommands[name] {
			t.Errorf("expected subcommand %q to be registered on workgraph cmd", name)
		}
	}

	if len(subcommands) != len(expected) {
		t.Errorf("len(subcommands) = %d, want %d", len(subcommands), len(expected))
	}
}

func TestWorkgraphCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}
	if !subcommands["workgraph"] {
		t.Error("expected 'workgraph' subcommand to be registered in root cmd")
	}
}

func TestWorkgraphCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"workgraph", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("workgraph --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"create-epic", "create-task", "update-epic", "update-task", "query"} {
		if !strings.Contains(got, want) {
			t.Errorf("workgraph help missing %q:\n%s", want, got)
		}
	}
}

// --- Flag tests ---

func TestWorkgraphCreateEpicCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newWorkgraphCreateEpicCmd()

	for _, name := range []string{"db", "ledger", "data"} {
		if cmd.Flags().Lookup(name) == nil {
			t.Errorf("create-epic should have --%s flag", name)
		}
	}

	if cmd.Flags().Lookup("db").DefValue != defaultDBPath() {
		t.Errorf("--db default = %q, want %q", cmd.Flags().Lookup("db").DefValue, defaultDBPath())
	}
	if cmd.Flags().Lookup("ledger").DefValue != defaultLedgerDir() {
		t.Errorf("--ledger default = %q, want %q", cmd.Flags().Lookup("ledger").DefValue, defaultLedgerDir())
	}
}

func TestWorkgraphCreateTaskCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newWorkgraphCreateTaskCmd()

	for _, name := range []string{"db", "ledger", "data"} {
		if cmd.Flags().Lookup(name) == nil {
			t.Errorf("create-task should have --%s flag", name)
		}
	}
}

func TestWorkgraphUpdateEpicCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newWorkgraphUpdateEpicCmd()

	for _, name := range []string{"db", "ledger", "data"} {
		if cmd.Flags().Lookup(name) == nil {
			t.Errorf("update-epic should have --%s flag", name)
		}
	}
}

func TestWorkgraphUpdateTaskCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newWorkgraphUpdateTaskCmd()

	for _, name := range []string{"db", "ledger", "data"} {
		if cmd.Flags().Lookup(name) == nil {
			t.Errorf("update-task should have --%s flag", name)
		}
	}
}

func TestWorkgraphQueryCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newWorkgraphQueryCmd()

	for _, name := range []string{"db", "mode", "id", "format-id", "epic-id", "area", "status", "domain", "work-type", "limit"} {
		if cmd.Flags().Lookup(name) == nil {
			t.Errorf("query should have --%s flag", name)
		}
	}

	if cmd.Flags().Lookup("limit").DefValue != "50" {
		t.Errorf("--limit default = %q, want %q", cmd.Flags().Lookup("limit").DefValue, "50")
	}
}

func TestWorkgraphQueryCmd_ModeRequired(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"workgraph", "query", "--db", "/nonexistent/path.db"})
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error when --mode not provided")
	}
}

// --- initTestDB creates a real SQLite DB for integration tests ---

func initTestDB(t *testing.T) string {
	t.Helper()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	d, err := db.NewDB(dbPath)
	if err != nil {
		t.Fatalf("initTestDB: NewDB: %v", err)
	}
	defer d.Close()

	ctx := t.Context()
	if err := d.InitFromSchema(ctx); err != nil {
		t.Fatalf("initTestDB: InitFromSchema: %v", err)
	}

	return dbPath
}

// --- runWorkgraphCreateEpic tests ---

func TestRunWorkgraphCreateEpic_Success(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	data := `{"area_type":"INF","title":"Test Epic","work_type":"RFCT","domain":"GENL","status":"draft"}`

	var buf strings.Builder
	err := runWorkgraphCreateEpic(t.Context(), &buf, dbPath, ledgerDir, data)
	if err != nil {
		t.Fatalf("runWorkgraphCreateEpic returned error: %v", err)
	}

	got := buf.String()
	if got == "" {
		t.Fatal("expected non-empty output")
	}

	var result map[string]any
	if err := json.Unmarshal([]byte(got), &result); err != nil {
		t.Fatalf("output is not valid JSON: %v\noutput: %s", err, got)
	}
	if result["id"] == nil {
		t.Error("output should contain 'id' field")
	}
	if result["format_id"] == nil {
		t.Error("output should contain 'format_id' field")
	}
}

func TestRunWorkgraphCreateEpic_InvalidInput(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	// Missing required fields.
	data := `{"title":"Missing area_type"}`

	var buf strings.Builder
	err := runWorkgraphCreateEpic(t.Context(), &buf, dbPath, ledgerDir, data)
	if err == nil {
		t.Fatal("expected error for invalid input")
	}

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitConfigError {
		t.Errorf("exit code = %d, want %d (ExitConfigError)", ee.code, ExitConfigError)
	}
}

func TestRunWorkgraphCreateEpic_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runWorkgraphCreateEpic(t.Context(), &buf, "/nonexistent/path.db", t.TempDir(), `{"area_type":"INF"}`)
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitConfigError {
		t.Errorf("exit code = %d, want %d (ExitConfigError)", ee.code, ExitConfigError)
	}
}

func TestRunWorkgraphCreateEpic_InvalidLedgerDir(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)

	var buf strings.Builder
	err := runWorkgraphCreateEpic(t.Context(), &buf, dbPath, "/dev/null/invalid", `{"area_type":"INF"}`)
	if err == nil {
		t.Error("expected error for invalid ledger dir")
	}

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitRuntimeError {
		t.Errorf("exit code = %d, want %d (ExitRuntimeError)", ee.code, ExitRuntimeError)
	}
}

// --- runWorkgraphCreateTask tests ---

func TestRunWorkgraphCreateTask_Success(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	// First create an epic to reference.
	var epicBuf strings.Builder
	epicData := `{"area_type":"INF","title":"Parent Epic","work_type":"FEAT","domain":"GENL","status":"draft"}`
	if err := runWorkgraphCreateEpic(t.Context(), &epicBuf, dbPath, ledgerDir, epicData); err != nil {
		t.Fatalf("creating parent epic: %v", err)
	}

	var epicResult map[string]any
	if err := json.Unmarshal([]byte(epicBuf.String()), &epicResult); err != nil {
		t.Fatalf("parsing epic result: %v", err)
	}
	epicID, ok := epicResult["id"].(string)
	if !ok || epicID == "" {
		t.Fatal("epic result should have string 'id'")
	}

	// Now create a task under this epic.
	taskData := `{"epic_id":"` + epicID + `","title":"Test Task","area_type":"INF","work_type":"FEAT","domain":"GENL","priority":"normal","origin":"planned"}`
	var taskBuf strings.Builder
	err := runWorkgraphCreateTask(t.Context(), &taskBuf, dbPath, ledgerDir, taskData)
	if err != nil {
		t.Fatalf("runWorkgraphCreateTask returned error: %v", err)
	}

	got := taskBuf.String()
	var taskResult map[string]any
	if err := json.Unmarshal([]byte(got), &taskResult); err != nil {
		t.Fatalf("output is not valid JSON: %v\noutput: %s", err, got)
	}
	if taskResult["id"] == nil {
		t.Error("output should contain 'id' field")
	}
	if taskResult["format_id"] == nil {
		t.Error("output should contain 'format_id' field")
	}
}

func TestRunWorkgraphCreateTask_InvalidInput(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	// Missing required fields.
	data := `{"title":"Missing epic_id"}`
	var buf strings.Builder
	err := runWorkgraphCreateTask(t.Context(), &buf, dbPath, ledgerDir, data)
	if err == nil {
		t.Fatal("expected error for invalid input")
	}

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
}

// --- runWorkgraphUpdateEpic tests ---

func TestRunWorkgraphUpdateEpic_Success(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	// Create an epic first.
	var epicBuf strings.Builder
	epicData := `{"area_type":"INF","title":"Epic to Update","work_type":"RFCT","domain":"GENL","status":"draft"}`
	if err := runWorkgraphCreateEpic(t.Context(), &epicBuf, dbPath, ledgerDir, epicData); err != nil {
		t.Fatalf("creating epic: %v", err)
	}

	var epicResult map[string]any
	if err := json.Unmarshal([]byte(epicBuf.String()), &epicResult); err != nil {
		t.Fatalf("parsing epic: %v", err)
	}
	epicID := epicResult["id"].(string)

	// Update it.
	updateData := `{"id":"` + epicID + `","status":"in_progress"}`
	var updateBuf strings.Builder
	err := runWorkgraphUpdateEpic(t.Context(), &updateBuf, dbPath, ledgerDir, updateData)
	if err != nil {
		t.Fatalf("runWorkgraphUpdateEpic returned error: %v", err)
	}

	got := updateBuf.String()
	if got == "" {
		t.Fatal("expected non-empty output")
	}
}

func TestRunWorkgraphUpdateEpic_NotFound(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	data := `{"id":"01NONEXISTENT000000000000","status":"in_progress"}`
	var buf strings.Builder
	err := runWorkgraphUpdateEpic(t.Context(), &buf, dbPath, ledgerDir, data)
	if err == nil {
		t.Fatal("expected error for nonexistent epic")
	}

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
}

// --- runWorkgraphUpdateTask tests ---

func TestRunWorkgraphUpdateTask_Success(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	// Create epic + task.
	var epicBuf strings.Builder
	if err := runWorkgraphCreateEpic(t.Context(), &epicBuf, dbPath, ledgerDir,
		`{"area_type":"INF","title":"Parent","work_type":"FEAT","domain":"GENL","status":"draft"}`); err != nil {
		t.Fatalf("creating epic: %v", err)
	}
	var epicResult map[string]any
	if err := json.Unmarshal([]byte(epicBuf.String()), &epicResult); err != nil {
		t.Fatalf("parsing epic: %v", err)
	}
	epicID := epicResult["id"].(string)

	var taskBuf strings.Builder
	if err := runWorkgraphCreateTask(t.Context(), &taskBuf, dbPath, ledgerDir,
		`{"epic_id":"`+epicID+`","title":"Task to Update","area_type":"INF","work_type":"FEAT","domain":"GENL","priority":"normal","origin":"planned"}`); err != nil {
		t.Fatalf("creating task: %v", err)
	}
	var taskResult map[string]any
	if err := json.Unmarshal([]byte(taskBuf.String()), &taskResult); err != nil {
		t.Fatalf("parsing task: %v", err)
	}
	taskID := taskResult["id"].(string)

	// Update the task.
	updateData := `{"id":"` + taskID + `","status":"in_progress"}`
	var updateBuf strings.Builder
	err := runWorkgraphUpdateTask(t.Context(), &updateBuf, dbPath, ledgerDir, updateData)
	if err != nil {
		t.Fatalf("runWorkgraphUpdateTask returned error: %v", err)
	}

	got := updateBuf.String()
	if got == "" {
		t.Fatal("expected non-empty output")
	}
}

func TestRunWorkgraphUpdateTask_InvalidInput(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	// No identifier provided.
	data := `{"status":"in_progress"}`
	var buf strings.Builder
	err := runWorkgraphUpdateTask(t.Context(), &buf, dbPath, ledgerDir, data)
	if err == nil {
		t.Fatal("expected error for missing identifier")
	}
}

// --- runWorkgraphQuery tests ---

func TestRunWorkgraphQuery_ListEpics(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	// Create an epic to list.
	var epicBuf strings.Builder
	if err := runWorkgraphCreateEpic(t.Context(), &epicBuf, dbPath, ledgerDir,
		`{"area_type":"INF","title":"Listable Epic","work_type":"FEAT","domain":"GENL","status":"draft"}`); err != nil {
		t.Fatalf("creating epic: %v", err)
	}

	var buf strings.Builder
	params := workgraph.QueryParams{Mode: "list-epics", Limit: 10}
	err := runWorkgraphQuery(t.Context(), &buf, dbPath, params)
	if err != nil {
		t.Fatalf("runWorkgraphQuery list-epics returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Listable Epic") {
		t.Errorf("output should contain 'Listable Epic':\n%s", got)
	}
}

func TestRunWorkgraphQuery_GetEpic(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	// Create an epic.
	var epicBuf strings.Builder
	if err := runWorkgraphCreateEpic(t.Context(), &epicBuf, dbPath, ledgerDir,
		`{"area_type":"INF","title":"Gettable Epic","work_type":"FEAT","domain":"GENL","status":"draft"}`); err != nil {
		t.Fatalf("creating epic: %v", err)
	}
	var epicResult map[string]any
	if err := json.Unmarshal([]byte(epicBuf.String()), &epicResult); err != nil {
		t.Fatalf("parsing epic: %v", err)
	}
	epicID := epicResult["id"].(string)

	var buf strings.Builder
	params := workgraph.QueryParams{Mode: "get-epic", ID: epicID}
	err := runWorkgraphQuery(t.Context(), &buf, dbPath, params)
	if err != nil {
		t.Fatalf("runWorkgraphQuery get-epic returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Gettable Epic") {
		t.Errorf("output should contain 'Gettable Epic':\n%s", got)
	}
}

func TestRunWorkgraphQuery_InvalidMode(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)

	var buf strings.Builder
	params := workgraph.QueryParams{Mode: "invalid-mode"}
	err := runWorkgraphQuery(t.Context(), &buf, dbPath, params)
	if err == nil {
		t.Fatal("expected error for invalid query mode")
	}

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitConfigError {
		t.Errorf("exit code = %d, want %d (ExitConfigError)", ee.code, ExitConfigError)
	}
}

func TestRunWorkgraphQuery_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	params := workgraph.QueryParams{Mode: "list-epics"}
	err := runWorkgraphQuery(t.Context(), &buf, "/nonexistent/path.db", params)
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitConfigError {
		t.Errorf("exit code = %d, want %d (ExitConfigError)", ee.code, ExitConfigError)
	}
}

func TestRunWorkgraphQuery_ListTasks(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	// Create epic + task.
	var epicBuf strings.Builder
	if err := runWorkgraphCreateEpic(t.Context(), &epicBuf, dbPath, ledgerDir,
		`{"area_type":"INF","title":"Parent","work_type":"FEAT","domain":"GENL","status":"draft"}`); err != nil {
		t.Fatalf("creating epic: %v", err)
	}
	var epicResult map[string]any
	if err := json.Unmarshal([]byte(epicBuf.String()), &epicResult); err != nil {
		t.Fatalf("parsing epic: %v", err)
	}
	epicID := epicResult["id"].(string)

	var taskBuf strings.Builder
	if err := runWorkgraphCreateTask(t.Context(), &taskBuf, dbPath, ledgerDir,
		`{"epic_id":"`+epicID+`","title":"Listable Task","area_type":"INF","work_type":"FEAT","domain":"GENL","priority":"normal","origin":"planned"}`); err != nil {
		t.Fatalf("creating task: %v", err)
	}

	var buf strings.Builder
	params := workgraph.QueryParams{Mode: "list-tasks", Limit: 10}
	err := runWorkgraphQuery(t.Context(), &buf, dbPath, params)
	if err != nil {
		t.Fatalf("runWorkgraphQuery list-tasks returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "Listable Task") {
		t.Errorf("output should contain 'Listable Task':\n%s", got)
	}
}

// --- newWorkgraphService tests ---

func TestNewWorkgraphService_InvalidDBPath(t *testing.T) {
	t.Parallel()

	_, _, err := newWorkgraphService("/nonexistent/path.db", t.TempDir())
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitConfigError {
		t.Errorf("exit code = %d, want %d (ExitConfigError)", ee.code, ExitConfigError)
	}
}

func TestNewWorkgraphService_InvalidLedgerDir(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)

	_, _, err := newWorkgraphService(dbPath, "/dev/null/invalid")
	if err == nil {
		t.Error("expected error for invalid ledger dir")
	}

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitRuntimeError {
		t.Errorf("exit code = %d, want %d (ExitRuntimeError)", ee.code, ExitRuntimeError)
	}
}

func TestNewWorkgraphService_Success(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	svc, cleanup, err := newWorkgraphService(dbPath, ledgerDir)
	if err != nil {
		t.Fatalf("newWorkgraphService returned error: %v", err)
	}
	defer cleanup()

	if svc == nil {
		t.Fatal("expected non-nil service")
	}
	if svc.DB == nil {
		t.Error("service.DB should not be nil")
	}
	if svc.Writer == nil {
		t.Error("service.Writer should not be nil")
	}
}

// --- classifyWorkgraphError tests ---

func TestClassifyWorkgraphError_InvalidInput(t *testing.T) {
	t.Parallel()

	err := classifyWorkgraphError(workgraph.ErrInvalidInput)

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitConfigError {
		t.Errorf("exit code = %d, want %d (ExitConfigError)", ee.code, ExitConfigError)
	}
}

func TestClassifyWorkgraphError_NotFound(t *testing.T) {
	t.Parallel()

	err := classifyWorkgraphError(workgraph.ErrNotFound)

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitGeneralError {
		t.Errorf("exit code = %d, want %d (ExitGeneralError)", ee.code, ExitGeneralError)
	}
}

func TestClassifyWorkgraphError_Constraint(t *testing.T) {
	t.Parallel()

	err := classifyWorkgraphError(workgraph.ErrConstraint)

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitConfigError {
		t.Errorf("exit code = %d, want %d (ExitConfigError)", ee.code, ExitConfigError)
	}
}

func TestClassifyWorkgraphError_Unknown(t *testing.T) {
	t.Parallel()

	err := classifyWorkgraphError(errors.New("some unknown error"))

	var ee *exitError
	if !errors.As(err, &ee) {
		t.Fatalf("error should be *exitError, got %T: %v", err, err)
	}
	if ee.code != ExitRuntimeError {
		t.Errorf("exit code = %d, want %d (ExitRuntimeError)", ee.code, ExitRuntimeError)
	}
}

// --- Via root command integration tests ---

func TestWorkgraphCreateEpic_ViaRootCmd(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"workgraph", "create-epic",
		"--db", dbPath,
		"--ledger", ledgerDir,
		"--data", `{"area_type":"INF","title":"CLI Epic","work_type":"RFCT","domain":"GENL","status":"draft"}`})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("workgraph create-epic via root cmd returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "id") || !strings.Contains(got, "format_id") {
		t.Errorf("output missing id/format_id fields:\n%s", got)
	}
}

func TestWorkgraphQuery_ViaRootCmd(t *testing.T) {
	t.Parallel()

	dbPath := initTestDB(t)
	ledgerDir := t.TempDir()

	// Create an epic first.
	cmd := newRootCmd()
	var createBuf strings.Builder
	cmd.SetOut(&createBuf)
	cmd.SetArgs([]string{"workgraph", "create-epic",
		"--db", dbPath,
		"--ledger", ledgerDir,
		"--data", `{"area_type":"INF","title":"CLI Query Epic","work_type":"RFCT","domain":"GENL","status":"draft"}`})
	if err := cmd.Execute(); err != nil {
		t.Fatalf("create-epic: %v", err)
	}

	// Query.
	cmd2 := newRootCmd()
	var queryBuf strings.Builder
	cmd2.SetOut(&queryBuf)
	cmd2.SetArgs([]string{"workgraph", "query",
		"--db", dbPath,
		"--mode", "list-epics"})
	if err := cmd2.Execute(); err != nil {
		t.Fatalf("query list-epics via root cmd returned error: %v", err)
	}

	got := queryBuf.String()
	if !strings.Contains(got, "CLI Query Epic") {
		t.Errorf("output should contain 'CLI Query Epic':\n%s", got)
	}
}
