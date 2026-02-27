package autorun

import (
	"context"
	"fmt"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
)

// newTestDB creates a temporary database with the full schema initialized.
func newTestDB(t *testing.T) *db.DB {
	t.Helper()
	path := filepath.Join(t.TempDir(), "test.db")
	d, err := db.NewDB(path)
	if err != nil {
		t.Fatalf("newTestDB: %v", err)
	}
	t.Cleanup(func() { d.Close() })

	ctx := t.Context()
	if err := d.InitFromSchema(ctx); err != nil {
		t.Fatalf("InitFromSchema: %v", err)
	}
	return d
}

// testRefOnce ensures reference rows are inserted only once per test DB.
var testRefOnce sync.Map

// insertTestRefs inserts required reference data for FK constraints.
func insertTestRefs(t *testing.T, d *db.DB) {
	t.Helper()
	ctx := t.Context()

	// Use a unique key per DB instance to avoid duplicate inserts.
	key := fmt.Sprintf("%p", d)
	if _, loaded := testRefOnce.LoadOrStore(key, true); loaded {
		return
	}

	refs := []string{
		`INSERT OR IGNORE INTO area_types (code, name) VALUES ('INF', 'Infrastructure')`,
		`INSERT OR IGNORE INTO work_types (code, name, branch_prefix) VALUES ('FEAT', 'Feature', 'feat/')`,
		`INSERT OR IGNORE INTO domains (code, name) VALUES ('GENL', 'General')`,
		`INSERT OR IGNORE INTO epics (id, format_id, title, status, area_type, work_type, domain, priority)
		 VALUES ('epic-test', 'INF-EPC-999', 'Test Epic', 'in_progress', 'INF', 'FEAT', 'GENL', 'normal')`,
	}
	for _, stmt := range refs {
		if _, err := d.Execute(ctx, stmt); err != nil {
			t.Fatalf("insertTestRefs: %v", err)
		}
	}
}

// insertTestTask inserts a minimal task record into the DB for FK satisfaction.
func insertTestTask(t *testing.T, d *db.DB, taskID string) {
	t.Helper()
	ctx := t.Context()

	insertTestRefs(t, d)

	// Generate a unique format_id suffix from the task ID.
	suffix := taskID
	if len(suffix) > 10 {
		suffix = suffix[len(suffix)-10:]
	}

	_, err := d.Execute(ctx,
		`INSERT OR IGNORE INTO tasks (id, format_id, epic_id, title, status, area_type, work_type, domain, origin, scope_policy, priority)
		 VALUES (?, ?, 'epic-test', 'Test Task', 'todo', 'INF', 'FEAT', 'GENL', 'planned', 'hard', 'normal')`,
		taskID, "TSK-"+suffix,
	)
	if err != nil {
		t.Fatalf("insertTestTask(%s): %v", taskID, err)
	}
}

// mockRunner implements WorkerRunner for testing.
type mockRunner struct {
	mu      sync.Mutex
	calls   []WorkerConfig
	results map[string]*WorkerResult
	delay   time.Duration
}

func newMockRunner() *mockRunner {
	return &mockRunner{
		results: make(map[string]*WorkerResult),
	}
}

func (m *mockRunner) addResult(taskID string, result *WorkerResult) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.results[taskID] = result
}

func (m *mockRunner) Run(_ context.Context, cfg WorkerConfig) (*WorkerResult, error) {
	m.mu.Lock()
	m.calls = append(m.calls, cfg)
	result := m.results[cfg.TaskID]
	delay := m.delay
	m.mu.Unlock()

	if delay > 0 {
		time.Sleep(delay)
	}

	if result != nil {
		result.WorkerID = cfg.WorkerID
		result.TaskID = cfg.TaskID
		return result, nil
	}

	return &WorkerResult{
		WorkerID: cfg.WorkerID,
		TaskID:   cfg.TaskID,
		Status:   "completed",
		ExitCode: 0,
	}, nil
}

func TestOrchestratorStart_CreatesSession(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	insertTestTask(t, d, "task-001")
	insertTestTask(t, d, "task-002")

	runner := newMockRunner()
	orch := NewOrchestrator(d, runner)

	batch := &ParsedBatch{
		Name:       "test-batch",
		FilePath:   "test.yaml",
		MaxWorkers: 2,
		Tasks: []TaskSpec{
			{ID: "task-001"},
			{ID: "task-002", DependsOn: []string{"task-001"}},
		},
		Order: []string{"task-001", "task-002"},
	}

	sessionID, err := orch.Start(ctx, batch)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	if !strings.HasPrefix(sessionID, "ars-") {
		t.Errorf("session ID = %q, want prefix ars-", sessionID)
	}

	// Verify session record in DB.
	session, err := GetSessionStatus(ctx, d, sessionID)
	if err != nil {
		t.Fatalf("GetSessionStatus: %v", err)
	}

	if session.TotalTasks != 2 {
		t.Errorf("TotalTasks = %d, want 2", session.TotalTasks)
	}
	if session.MaxSessionWorkers != 2 {
		t.Errorf("MaxSessionWorkers = %d, want 2", session.MaxSessionWorkers)
	}
}

func TestOrchestratorStart_TaskSequencing(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	insertTestTask(t, d, "task-a")
	insertTestTask(t, d, "task-b")
	insertTestTask(t, d, "task-c")

	runner := newMockRunner()
	runner.delay = 10 * time.Millisecond // Small delay to ensure ordering is observable.
	orch := NewOrchestrator(d, runner)

	batch := &ParsedBatch{
		Name:       "seq-test",
		FilePath:   "test.yaml",
		MaxWorkers: 1, // Force sequential execution.
		Tasks: []TaskSpec{
			{ID: "task-a"},
			{ID: "task-b", DependsOn: []string{"task-a"}},
			{ID: "task-c", DependsOn: []string{"task-b"}},
		},
		Order: []string{"task-a", "task-b", "task-c"},
	}

	sessionID, err := orch.Start(ctx, batch)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Wait for execution to complete.
	waitForSessionCompletion(t, ctx, d, sessionID, 10*time.Second)

	// Verify all workers were called.
	runner.mu.Lock()
	callCount := len(runner.calls)
	runner.mu.Unlock()

	if callCount != 3 {
		t.Errorf("worker call count = %d, want 3", callCount)
	}

	// Verify session completed successfully.
	session, err := GetSessionStatus(ctx, d, sessionID)
	if err != nil {
		t.Fatalf("GetSessionStatus: %v", err)
	}
	if session.CompletedTasks != 3 {
		t.Errorf("CompletedTasks = %d, want 3", session.CompletedTasks)
	}
}

func TestOrchestratorStart_DependencyResolution(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	insertTestTask(t, d, "dep-a")
	insertTestTask(t, d, "dep-b")
	insertTestTask(t, d, "dep-c")

	var executionOrder []string
	var orderMu sync.Mutex

	runner := &recordingRunner{
		fn: func(cfg WorkerConfig) *WorkerResult {
			orderMu.Lock()
			executionOrder = append(executionOrder, cfg.TaskID)
			orderMu.Unlock()
			time.Sleep(5 * time.Millisecond) // Simulate work.
			return &WorkerResult{
				WorkerID: cfg.WorkerID,
				TaskID:   cfg.TaskID,
				Status:   "completed",
			}
		},
	}

	orch := NewOrchestrator(d, runner)
	batch := &ParsedBatch{
		Name:       "dep-test",
		FilePath:   "test.yaml",
		MaxWorkers: 3,
		Tasks: []TaskSpec{
			{ID: "dep-a"},
			{ID: "dep-b", DependsOn: []string{"dep-a"}},
			{ID: "dep-c", DependsOn: []string{"dep-a"}},
		},
		Order: []string{"dep-a", "dep-b", "dep-c"},
	}

	sessionID, err := orch.Start(ctx, batch)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	waitForSessionCompletion(t, ctx, d, sessionID, 10*time.Second)

	orderMu.Lock()
	defer orderMu.Unlock()

	if len(executionOrder) != 3 {
		t.Fatalf("execution order length = %d, want 3", len(executionOrder))
	}

	// dep-a must always be first since dep-b and dep-c depend on it.
	if executionOrder[0] != "dep-a" {
		t.Errorf("first executed task = %q, want dep-a", executionOrder[0])
	}
}

func TestOrchestratorStart_MaxWorkersEnforced(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	for i := 1; i <= 5; i++ {
		insertTestTask(t, d, taskIDForNum(i))
	}

	var maxConcurrent int
	var currentConcurrent int
	var concMu sync.Mutex

	runner := &recordingRunner{
		fn: func(cfg WorkerConfig) *WorkerResult {
			concMu.Lock()
			currentConcurrent++
			if currentConcurrent > maxConcurrent {
				maxConcurrent = currentConcurrent
			}
			concMu.Unlock()

			time.Sleep(50 * time.Millisecond)

			concMu.Lock()
			currentConcurrent--
			concMu.Unlock()

			return &WorkerResult{
				WorkerID: cfg.WorkerID,
				TaskID:   cfg.TaskID,
				Status:   "completed",
			}
		},
	}

	maxAllowed := 2
	orch := NewOrchestrator(d, runner)
	batch := &ParsedBatch{
		Name:       "concurrency-test",
		FilePath:   "test.yaml",
		MaxWorkers: maxAllowed,
		Tasks: []TaskSpec{
			{ID: taskIDForNum(1)},
			{ID: taskIDForNum(2)},
			{ID: taskIDForNum(3)},
			{ID: taskIDForNum(4)},
			{ID: taskIDForNum(5)},
		},
		Order: []string{taskIDForNum(1), taskIDForNum(2), taskIDForNum(3), taskIDForNum(4), taskIDForNum(5)},
	}

	sessionID, err := orch.Start(ctx, batch)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	waitForSessionCompletion(t, ctx, d, sessionID, 30*time.Second)

	concMu.Lock()
	observed := maxConcurrent
	concMu.Unlock()

	if observed > maxAllowed {
		t.Errorf("max concurrent workers = %d, exceeded limit of %d", observed, maxAllowed)
	}
}

func TestOrchestratorStart_ErrorHandling(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	insertTestTask(t, d, "err-a")
	insertTestTask(t, d, "err-b")

	runner := newMockRunner()
	runner.addResult("err-a", &WorkerResult{
		Status:   "failed",
		ExitCode: 1,
		Error:    "build failed",
	})

	orch := NewOrchestrator(d, runner)
	batch := &ParsedBatch{
		Name:       "error-test",
		FilePath:   "test.yaml",
		MaxWorkers: 2,
		Tasks: []TaskSpec{
			{ID: "err-a"},
			{ID: "err-b", DependsOn: []string{"err-a"}},
		},
		Order: []string{"err-a", "err-b"},
	}

	sessionID, err := orch.Start(ctx, batch)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	waitForSessionCompletion(t, ctx, d, sessionID, 10*time.Second)

	session, err := GetSessionStatus(ctx, d, sessionID)
	if err != nil {
		t.Fatalf("GetSessionStatus: %v", err)
	}

	// err-a failed, err-b should be skipped.
	if session.FailedTasks != 2 {
		t.Errorf("FailedTasks = %d, want 2 (1 failed + 1 skipped)", session.FailedTasks)
	}
	if session.Status != "failed" {
		t.Errorf("Status = %q, want failed", session.Status)
	}
}

func TestOrchestratorStart_StatusTracking(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	insertTestTask(t, d, "st-001")

	runner := newMockRunner()
	orch := NewOrchestrator(d, runner)

	batch := &ParsedBatch{
		Name:       "status-test",
		FilePath:   "test.yaml",
		MaxWorkers: 1,
		Tasks:      []TaskSpec{{ID: "st-001"}},
		Order:      []string{"st-001"},
	}

	sessionID, err := orch.Start(ctx, batch)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Verify initial state.
	session, err := GetSessionStatus(ctx, d, sessionID)
	if err != nil {
		t.Fatalf("GetSessionStatus: %v", err)
	}
	if session.Status != "running" {
		t.Errorf("initial status = %q, want running", session.Status)
	}

	waitForSessionCompletion(t, ctx, d, sessionID, 10*time.Second)

	// Verify final state.
	session, err = GetSessionStatus(ctx, d, sessionID)
	if err != nil {
		t.Fatalf("GetSessionStatus: %v", err)
	}
	if session.Status != "completed" {
		t.Errorf("final status = %q, want completed", session.Status)
	}
	if session.CompletedTasks != 1 {
		t.Errorf("CompletedTasks = %d, want 1", session.CompletedTasks)
	}
}

func TestListSessions(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	// Insert two sessions.
	for _, id := range []string{"ars-session1", "ars-session2"} {
		_, err := d.Execute(ctx,
			`INSERT INTO autorun_sessions (id, batch_file, status, max_session_workers, total_tasks)
			 VALUES (?, ?, ?, ?, ?)`,
			id, "test.yaml", "completed", 3, 1,
		)
		if err != nil {
			t.Fatalf("inserting session: %v", err)
		}
	}

	sessions, err := ListSessions(ctx, d, 10)
	if err != nil {
		t.Fatalf("ListSessions: %v", err)
	}
	if len(sessions) != 2 {
		t.Errorf("len(sessions) = %d, want 2", len(sessions))
	}
}

func TestGetSessionWorkers(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	insertTestTask(t, d, "w-task-001")

	// Insert an autorun session and worker.
	_, err := d.Execute(ctx,
		`INSERT INTO autorun_sessions (id, batch_file, status, max_session_workers, total_tasks)
		 VALUES (?, ?, ?, ?, ?)`,
		"ars-w-test", "test.yaml", "running", 3, 1,
	)
	if err != nil {
		t.Fatalf("inserting autorun session: %v", err)
	}

	_, err = d.Execute(ctx,
		`INSERT INTO autorun_workers (id, session_id, worker_num, task_id, status)
		 VALUES (?, ?, ?, ?, ?)`,
		"arw-001", "ars-w-test", 1, "w-task-001", "running",
	)
	if err != nil {
		t.Fatalf("inserting worker: %v", err)
	}

	workers, err := GetSessionWorkers(ctx, d, "ars-w-test")
	if err != nil {
		t.Fatalf("GetSessionWorkers: %v", err)
	}
	if len(workers) != 1 {
		t.Fatalf("len(workers) = %d, want 1", len(workers))
	}
	if workers[0].Status != "running" {
		t.Errorf("worker status = %q, want running", workers[0].Status)
	}
}

// recordingRunner records function call order for testing.
type recordingRunner struct {
	fn func(cfg WorkerConfig) *WorkerResult
}

func (r *recordingRunner) Run(_ context.Context, cfg WorkerConfig) (*WorkerResult, error) {
	return r.fn(cfg), nil
}

// taskIDForNum generates a task ID for test setup.
func taskIDForNum(n int) string {
	return fmt.Sprintf("task-%03d", n)
}

// waitForSessionCompletion polls the session status until complete or timeout.
func waitForSessionCompletion(t *testing.T, ctx context.Context, d *db.DB, sessionID string, timeout time.Duration) {
	t.Helper()
	deadline := time.Now().Add(timeout)

	for time.Now().Before(deadline) {
		session, err := GetSessionStatus(ctx, d, sessionID)
		if err != nil {
			t.Fatalf("GetSessionStatus: %v", err)
		}

		switch session.Status {
		case "completed", "failed", "cancelled":
			return
		}

		time.Sleep(100 * time.Millisecond)
	}

	t.Fatalf("session %s did not complete within %v", sessionID, timeout)
}

func TestGetSessionStatus_NotFound(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	_, err := GetSessionStatus(ctx, d, "nonexistent")
	if err == nil {
		t.Fatal("expected error for nonexistent session, got nil")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Errorf("error = %v, want 'not found' in message", err)
	}
}

func TestNullHelpers(t *testing.T) {
	t.Run("nullString empty", func(t *testing.T) {
		ns := nullString("")
		if ns.Valid {
			t.Error("nullString('') should be invalid")
		}
	})

	t.Run("nullString non-empty", func(t *testing.T) {
		ns := nullString("hello")
		if !ns.Valid || ns.String != "hello" {
			t.Errorf("nullString('hello') = %+v, want Valid=true String='hello'", ns)
		}
	})

	t.Run("nullInt64 zero", func(t *testing.T) {
		ni := nullInt64(0)
		if ni.Valid {
			t.Error("nullInt64(0) should be invalid")
		}
	})

	t.Run("nullInt64 non-zero", func(t *testing.T) {
		ni := nullInt64(42)
		if !ni.Valid || ni.Int64 != 42 {
			t.Errorf("nullInt64(42) = %+v, want Valid=true Int64=42", ni)
		}
	})
}

func TestGenerateID(t *testing.T) {
	id := generateID("test-")
	if !strings.HasPrefix(id, "test-") {
		t.Errorf("generateID prefix = %q, want test-", id)
	}
	// ULID is 26 chars lowercase.
	suffix := id[5:]
	if len(suffix) != 26 {
		t.Errorf("ULID suffix length = %d, want 26", len(suffix))
	}
	if suffix != strings.ToLower(suffix) {
		t.Errorf("ULID suffix should be lowercase: %q", suffix)
	}
}

func TestGetLatestSession(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	// No sessions yet.
	_, err := GetLatestSession(ctx, d)
	if err == nil {
		t.Fatal("expected error when no sessions exist, got nil")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Errorf("error = %v, want 'not found' in message", err)
	}

	// Insert two sessions with different timestamps.
	for i, id := range []string{"ars-oldest", "ars-newest"} {
		_, err := d.Execute(ctx,
			`INSERT INTO autorun_sessions (id, batch_file, status, max_session_workers, total_tasks, created_at)
			 VALUES (?, ?, ?, ?, ?, ?)`,
			id, "test.yaml", "completed", 2, 1,
			fmt.Sprintf("2025-01-0%dT00:00:00Z", i+1),
		)
		if err != nil {
			t.Fatalf("inserting session %s: %v", id, err)
		}
	}

	latest, err := GetLatestSession(ctx, d)
	if err != nil {
		t.Fatalf("GetLatestSession: %v", err)
	}
	if latest.ID != "ars-newest" {
		t.Errorf("latest session ID = %q, want ars-newest", latest.ID)
	}
}

func TestListSessions_Empty(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	sessions, err := ListSessions(ctx, d, 10)
	if err != nil {
		t.Fatalf("ListSessions: %v", err)
	}
	if len(sessions) != 0 {
		t.Errorf("len(sessions) = %d, want 0", len(sessions))
	}
}

func TestListSessions_DefaultLimit(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	// Test that 0 or negative limit defaults to 20.
	sessions, err := ListSessions(ctx, d, 0)
	if err != nil {
		t.Fatalf("ListSessions: %v", err)
	}
	if len(sessions) != 0 {
		t.Errorf("len(sessions) = %d, want 0", len(sessions))
	}
}

func TestOrchestratorStart_Cancellation(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	insertTestTask(t, d, "cancel-001")
	insertTestTask(t, d, "cancel-002")

	runner := newMockRunner()
	runner.delay = 200 * time.Millisecond
	orch := NewOrchestrator(d, runner)

	cancelCtx, cancel := context.WithCancel(ctx)

	batch := &ParsedBatch{
		Name:       "cancel-test",
		FilePath:   "test.yaml",
		MaxWorkers: 1,
		Tasks: []TaskSpec{
			{ID: "cancel-001"},
			{ID: "cancel-002"},
		},
		Order: []string{"cancel-001", "cancel-002"},
	}

	sessionID, err := orch.Start(cancelCtx, batch)
	if err != nil {
		t.Fatalf("Start: %v", err)
	}

	// Cancel after a short delay.
	time.Sleep(50 * time.Millisecond)
	cancel()

	// Wait for the session to reach a terminal state.
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		session, err := GetSessionStatus(ctx, d, sessionID)
		if err != nil {
			t.Fatalf("GetSessionStatus: %v", err)
		}
		if session.Status == "cancelled" || session.Status == "completed" || session.Status == "failed" {
			return
		}
		time.Sleep(50 * time.Millisecond)
	}

	t.Fatal("session did not reach terminal state after cancellation")
}

func TestGetSessionWorkers_Empty(t *testing.T) {
	d := newTestDB(t)
	ctx := t.Context()

	// Insert a session with no workers.
	_, err := d.Execute(ctx,
		`INSERT INTO autorun_sessions (id, batch_file, status, max_session_workers, total_tasks)
		 VALUES (?, ?, ?, ?, ?)`,
		"ars-empty", "test.yaml", "running", 1, 0,
	)
	if err != nil {
		t.Fatalf("inserting session: %v", err)
	}

	workers, err := GetSessionWorkers(ctx, d, "ars-empty")
	if err != nil {
		t.Fatalf("GetSessionWorkers: %v", err)
	}
	if len(workers) != 0 {
		t.Errorf("len(workers) = %d, want 0", len(workers))
	}
}
