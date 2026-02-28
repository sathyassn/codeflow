package autorun

import (
	"path/filepath"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/db"
)

// setupTestDB creates a temporary database with schema and seed data for
// testing the autorun command functions. The database is created in
// t.TempDir() and closed automatically via t.Cleanup.
//
// The returned path can be passed directly to runSessions, runStatus, etc.
//
// Seed data:
//   - 1 epic (INF-EPC-999) with area_type=INF, work_type=FEAT, domain=GENL
//   - 2 tasks (task-001, task-002) linked to the epic
//   - 2 autorun sessions:
//     session "ars-session-001" (running, with batch_name, 2 workers)
//     session "ars-session-002" (completed, no batch_name, completed_at set)
//   - 2 workers for session-001:
//     worker 1 (running, tmux_session set)
//     worker 2 (completed, pr_number set)
func setupTestDB(t *testing.T) string {
	t.Helper()

	dbPath := filepath.Join(t.TempDir(), "test.db")
	d, err := db.NewDB(dbPath)
	if err != nil {
		t.Fatalf("opening test db: %v", err)
	}
	t.Cleanup(func() { d.Close() })

	ctx := t.Context()
	if err := d.InitFromSchema(ctx); err != nil {
		t.Fatalf("initializing schema: %v", err)
	}

	// Insert an epic (required by tasks FK).
	if _, err := d.Execute(ctx,
		`INSERT INTO epics (id, format_id, title, status, area_type, work_type, domain)
		 VALUES ('epic-001', 'INF-EPC-999', 'Test Epic', 'in_progress', 'INF', 'FEAT', 'GENL')`,
	); err != nil {
		t.Fatalf("inserting epic: %v", err)
	}

	// Insert tasks (required by autorun_workers FK).
	for _, taskID := range []string{"task-001", "task-002"} {
		formatID := "INF-TSK-999-001"
		if taskID == "task-002" {
			formatID = "INF-TSK-999-002"
		}
		if _, err := d.Execute(ctx,
			`INSERT INTO tasks (id, format_id, epic_id, title, status, area_type, work_type, domain)
			 VALUES (?, ?, 'epic-001', 'Test Task', 'todo', 'INF', 'FEAT', 'GENL')`,
			taskID, formatID,
		); err != nil {
			t.Fatalf("inserting task %s: %v", taskID, err)
		}
	}

	// Insert autorun sessions.
	if _, err := d.Execute(ctx,
		`INSERT INTO autorun_sessions (id, batch_file, batch_name, status, max_session_workers, total_tasks, completed_tasks, failed_tasks, created_at)
		 VALUES ('ars-session-001', '/path/to/batch.yaml', 'integration-tests', 'running', 3, 2, 0, 0, '2025-06-01T10:00:00Z')`,
	); err != nil {
		t.Fatalf("inserting session-001: %v", err)
	}
	if _, err := d.Execute(ctx,
		`INSERT INTO autorun_sessions (id, batch_file, batch_name, status, max_session_workers, total_tasks, completed_tasks, failed_tasks, created_at, completed_at)
		 VALUES ('ars-session-002', '/path/to/batch2.yaml', NULL, 'completed', 2, 5, 4, 1, '2025-06-02T10:00:00Z', '2025-06-02T12:00:00Z')`,
	); err != nil {
		t.Fatalf("inserting session-002: %v", err)
	}

	// Insert workers for session-001.
	if _, err := d.Execute(ctx,
		`INSERT INTO autorun_workers (id, session_id, worker_num, task_id, status, tmux_session, pr_number)
		 VALUES ('arw-001', 'ars-session-001', 1, 'task-001', 'running', 'autorun-001', NULL)`,
	); err != nil {
		t.Fatalf("inserting worker-001: %v", err)
	}
	if _, err := d.Execute(ctx,
		`INSERT INTO autorun_workers (id, session_id, worker_num, task_id, status, tmux_session, pr_number)
		 VALUES ('arw-002', 'ars-session-001', 2, 'task-002', 'completed', NULL, 42)`,
	); err != nil {
		t.Fatalf("inserting worker-002: %v", err)
	}

	return dbPath
}
