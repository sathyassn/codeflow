package autorun

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"sync"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/oklog/ulid/v2"
)

// DefaultMaxWorkers is the default maximum concurrent workers.
const DefaultMaxWorkers = 3

// Sentinel errors for orchestrator operations.
var (
	// ErrSessionNotFound indicates the autorun session was not found.
	ErrSessionNotFound = errors.New("autorun: session not found")

	// ErrNoReadyTasks indicates no tasks are ready for execution.
	ErrNoReadyTasks = errors.New("autorun: no tasks ready for execution")
)

// WorkerRunner defines the interface for executing a task in a worker.
// This interface enables testing with mock implementations.
type WorkerRunner interface {
	// Run starts a worker for the given task and blocks until completion.
	Run(ctx context.Context, cfg WorkerConfig) (*WorkerResult, error)
}

// WorkerConfig contains the configuration for a single worker execution.
type WorkerConfig struct {
	SessionID  string
	WorkerID   string
	WorkerNum  int
	TaskID     string
	BatchName  string
	AutoMerge  bool
	Target     string
	TmuxPrefix string
}

// WorkerResult contains the outcome of a worker execution.
type WorkerResult struct {
	WorkerID    string
	TaskID      string
	Status      string
	ExitCode    int
	PRNumber    int64
	PRURL       string
	Error       string
	BranchName  string
	DurationSec int64
}

// Orchestrator manages task sequencing and worker lifecycle.
type Orchestrator struct {
	db     *db.DB
	runner WorkerRunner
	mu     sync.Mutex
}

// NewOrchestrator creates a new orchestrator with the given database and worker runner.
func NewOrchestrator(d *db.DB, runner WorkerRunner) *Orchestrator {
	return &Orchestrator{
		db:     d,
		runner: runner,
	}
}

// generateID creates a new ULID-based ID with the given prefix.
func generateID(prefix string) string {
	return prefix + strings.ToLower(ulid.Make().String())
}

// Start begins execution of a parsed batch file. It creates the autorun session
// record, queues all tasks, and launches workers up to maxWorkers concurrently.
func (o *Orchestrator) Start(ctx context.Context, batch *ParsedBatch) (string, error) {
	sessionID := generateID("ars-")
	now := time.Now().UTC().Format(time.RFC3339)

	// Create the autorun session record.
	err := o.db.Transaction(ctx, func(tx *sql.Tx) error {
		if _, txErr := tx.ExecContext(ctx,
			`INSERT INTO autorun_sessions (id, batch_file, batch_name, status, max_session_workers, total_tasks, created_at)
			 VALUES (?, ?, ?, ?, ?, ?, ?)`,
			sessionID, batch.FilePath, batch.Name, "running", batch.MaxWorkers, len(batch.Tasks), now,
		); txErr != nil {
			return fmt.Errorf("inserting autorun session: %w", txErr)
		}

		// Create worker and task_run records for each task.
		for i, taskID := range batch.Order {
			workerID := generateID("arw-")
			taskRunID := generateID("atr-")

			if _, txErr := tx.ExecContext(ctx,
				`INSERT INTO autorun_workers (id, session_id, worker_num, task_id, status, started_at)
				 VALUES (?, ?, ?, ?, ?, NULL)`,
				workerID, sessionID, i+1, taskID, "queued",
			); txErr != nil {
				return fmt.Errorf("inserting worker for task %s: %w", taskID, txErr)
			}

			if _, txErr := tx.ExecContext(ctx,
				`INSERT INTO autorun_task_runs (id, worker_id, task_id, session_id, status)
				 VALUES (?, ?, ?, ?, ?)`,
				taskRunID, workerID, taskID, sessionID, "pending",
			); txErr != nil {
				return fmt.Errorf("inserting task run for task %s: %w", taskID, txErr)
			}
		}

		return nil
	})
	if err != nil {
		return "", fmt.Errorf("autorun: creating session: %w", err)
	}

	// Launch execution in a goroutine so Start returns immediately.
	go o.execute(ctx, sessionID, batch)

	return sessionID, nil
}

// execute runs the task execution loop.
func (o *Orchestrator) execute(ctx context.Context, sessionID string, batch *ParsedBatch) {
	// Build dependency map for quick lookup.
	depMap := make(map[string][]string, len(batch.Tasks))
	for _, t := range batch.Tasks {
		depMap[t.ID] = t.DependsOn
	}

	completed := make(map[string]bool)
	failed := make(map[string]bool)
	running := make(map[string]bool)

	var wg sync.WaitGroup
	results := make(chan *WorkerResult, len(batch.Order))
	sem := make(chan struct{}, batch.MaxWorkers)

	totalTasks := len(batch.Order)

	for len(completed)+len(failed) < totalTasks {
		// Check for cancellation.
		select {
		case <-ctx.Done():
			o.updateSessionStatus(context.Background(), sessionID, "cancelled")
			wg.Wait()
			return
		default:
		}

		// Drain all available results first.
		o.collectResults(ctx, sessionID, results, completed, failed, running)

		// Find tasks ready to launch.
		launched := false
	launchLoop:
		for _, taskID := range batch.Order {
			if completed[taskID] || failed[taskID] || running[taskID] {
				continue
			}

			// Check if all dependencies are met.
			allDepsDone := true
			anyDepFailed := false
			for _, dep := range depMap[taskID] {
				if failed[dep] {
					anyDepFailed = true
				}
				if !completed[dep] {
					allDepsDone = false
				}
			}

			if anyDepFailed {
				failed[taskID] = true
				o.updateWorkerStatus(ctx, sessionID, taskID, "skipped")
				o.updateTaskRunStatus(ctx, sessionID, taskID, "skipped")
				o.incrementFailed(ctx, sessionID)
				continue
			}

			if !allDepsDone {
				continue
			}

			// Try to acquire a worker slot (non-blocking).
			select {
			case sem <- struct{}{}:
				running[taskID] = true
				launched = true
				wg.Add(1)
				go func(tid string) {
					defer wg.Done()
					defer func() { <-sem }()

					workerID, workerNum := o.getWorkerInfo(ctx, sessionID, tid)
					o.updateWorkerStatus(ctx, sessionID, tid, "starting")
					o.updateTaskRunStatus(ctx, sessionID, tid, "running")

					cfg := WorkerConfig{
						SessionID:  sessionID,
						WorkerID:   workerID,
						WorkerNum:  workerNum,
						TaskID:     tid,
						BatchName:  batch.Name,
						AutoMerge:  batch.AutoMerge,
						Target:     batch.Target,
						TmuxPrefix: "codeflow-worker",
					}

					startTime := time.Now()
					result, err := o.runner.Run(ctx, cfg)
					if err != nil {
						result = &WorkerResult{
							WorkerID: workerID,
							TaskID:   tid,
							Status:   "failed",
							ExitCode: 1,
							Error:    err.Error(),
						}
					}
					result.DurationSec = int64(time.Since(startTime).Seconds())
					results <- result
				}(taskID)
			default:
				// All worker slots full; no point checking remaining tasks.
				break launchLoop
			}
		}

		// If nothing was launched and workers are running, wait for a result.
		if !launched && len(running) > 0 {
			select {
			case <-ctx.Done():
				o.updateSessionStatus(context.Background(), sessionID, "cancelled")
				wg.Wait()
				return
			case r := <-results:
				o.processResult(ctx, sessionID, r, completed, failed, running)
			}
		} else if !launched && len(running) == 0 {
			// Nothing to launch and nothing running -- should not happen
			// unless all tasks are done or failed.
			break
		}
	}

	// Wait for all running workers to finish.
	wg.Wait()

	// Drain remaining results.
	close(results)
	for r := range results {
		o.processResult(ctx, sessionID, r, completed, failed, running)
	}

	// Update session final status.
	finalStatus := "completed"
	if len(failed) > 0 {
		finalStatus = "failed"
	}
	o.updateSessionStatus(ctx, sessionID, finalStatus)
}

// collectResults drains available results from the channel without blocking.
func (o *Orchestrator) collectResults(ctx context.Context, sessionID string, results chan *WorkerResult, completed, failed, running map[string]bool) {
	for {
		select {
		case r := <-results:
			if r == nil {
				return
			}
			o.processResult(ctx, sessionID, r, completed, failed, running)
		default:
			return
		}
	}
}

// processResult handles a worker result, updating maps and DB records.
func (o *Orchestrator) processResult(ctx context.Context, sessionID string, r *WorkerResult, completed, failed, running map[string]bool) {
	delete(running, r.TaskID)

	if r.Status == "completed" {
		completed[r.TaskID] = true
		o.incrementCompleted(ctx, sessionID)
	} else {
		failed[r.TaskID] = true
		o.incrementFailed(ctx, sessionID)
	}

	// Update worker record.
	now := time.Now().UTC().Format(time.RFC3339)
	if _, err := o.db.Execute(ctx,
		`UPDATE autorun_workers SET status = ?, completed_at = ?, pr_number = ?
		 WHERE session_id = ? AND task_id = ?`,
		r.Status, now, nullInt64(r.PRNumber), sessionID, r.TaskID,
	); err != nil {
		slog.Error("updating worker result", "error", err, "sessionID", sessionID, "taskID", r.TaskID)
	}

	// Update task run record.
	if _, err := o.db.Execute(ctx,
		`UPDATE autorun_task_runs SET status = ?, completed_at = ?, duration_seconds = ?,
		 exit_code = ?, error_message = ?, pr_number = ?, pr_url = ?, branch_name = ?
		 WHERE session_id = ? AND task_id = ?`,
		r.Status, now, r.DurationSec,
		nullInt64(int64(r.ExitCode)), nullString(r.Error),
		nullInt64(r.PRNumber), nullString(r.PRURL), nullString(r.BranchName),
		sessionID, r.TaskID,
	); err != nil {
		slog.Error("updating task run result", "error", err, "sessionID", sessionID, "taskID", r.TaskID)
	}
}

// getWorkerInfo retrieves the worker ID and number for a task.
func (o *Orchestrator) getWorkerInfo(ctx context.Context, sessionID, taskID string) (string, int) {
	var workerID string
	var workerNum int
	if err := o.db.QueryRow(ctx,
		`SELECT id, worker_num FROM autorun_workers WHERE session_id = ? AND task_id = ?`,
		sessionID, taskID,
	).Scan(&workerID, &workerNum); err != nil {
		slog.Error("querying worker info", "error", err, "sessionID", sessionID, "taskID", taskID)
	}
	return workerID, workerNum
}

// updateWorkerStatus updates the status of a worker for a given task.
func (o *Orchestrator) updateWorkerStatus(ctx context.Context, sessionID, taskID, status string) {
	now := time.Now().UTC().Format(time.RFC3339)
	cols := "status = ?"
	args := []any{status}

	if status == "starting" || status == "running" {
		cols += ", started_at = ?"
		args = append(args, now)
	}

	args = append(args, sessionID, taskID)
	if _, err := o.db.Execute(ctx,
		fmt.Sprintf("UPDATE autorun_workers SET %s WHERE session_id = ? AND task_id = ?", cols),
		args...,
	); err != nil {
		slog.Error("updating worker status", "error", err, "sessionID", sessionID, "taskID", taskID, "status", status)
	}
}

// updateTaskRunStatus updates the status of a task run.
func (o *Orchestrator) updateTaskRunStatus(ctx context.Context, sessionID, taskID, status string) {
	now := time.Now().UTC().Format(time.RFC3339)
	cols := "status = ?"
	args := []any{status}

	if status == "running" {
		cols += ", started_at = ?"
		args = append(args, now)
	}

	args = append(args, sessionID, taskID)
	if _, err := o.db.Execute(ctx,
		fmt.Sprintf("UPDATE autorun_task_runs SET %s WHERE session_id = ? AND task_id = ?", cols),
		args...,
	); err != nil {
		slog.Error("updating task run status", "error", err, "sessionID", sessionID, "taskID", taskID, "status", status)
	}
}

// updateSessionStatus sets the final status of an autorun session.
func (o *Orchestrator) updateSessionStatus(ctx context.Context, sessionID, status string) {
	now := time.Now().UTC().Format(time.RFC3339)
	if _, err := o.db.Execute(ctx,
		`UPDATE autorun_sessions SET status = ?, completed_at = ? WHERE id = ?`,
		status, now, sessionID,
	); err != nil {
		slog.Error("updating session status", "error", err, "sessionID", sessionID, "status", status)
	}
}

// incrementCompleted atomically increments the completed_tasks counter.
func (o *Orchestrator) incrementCompleted(ctx context.Context, sessionID string) {
	o.mu.Lock()
	defer o.mu.Unlock()
	if _, err := o.db.Execute(ctx,
		`UPDATE autorun_sessions SET completed_tasks = completed_tasks + 1 WHERE id = ?`,
		sessionID,
	); err != nil {
		slog.Error("incrementing completed tasks", "error", err, "sessionID", sessionID)
	}
}

// incrementFailed atomically increments the failed_tasks counter.
func (o *Orchestrator) incrementFailed(ctx context.Context, sessionID string) {
	o.mu.Lock()
	defer o.mu.Unlock()
	if _, err := o.db.Execute(ctx,
		`UPDATE autorun_sessions SET failed_tasks = failed_tasks + 1 WHERE id = ?`,
		sessionID,
	); err != nil {
		slog.Error("incrementing failed tasks", "error", err, "sessionID", sessionID)
	}
}

// GetSessionStatus retrieves the current status of an autorun session.
func GetSessionStatus(ctx context.Context, d *db.DB, sessionID string) (*db.AutorunSession, error) {
	var s db.AutorunSession
	err := d.QueryRow(ctx,
		`SELECT id, batch_file, batch_name, status, max_session_workers,
		        total_tasks, completed_tasks, failed_tasks, created_at, completed_at
		 FROM autorun_sessions WHERE id = ?`, sessionID,
	).Scan(&s.ID, &s.BatchFile, &s.BatchName, &s.Status, &s.MaxSessionWorkers,
		&s.TotalTasks, &s.CompletedTasks, &s.FailedTasks, &s.CreatedAt, &s.CompletedAt)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return nil, ErrSessionNotFound
		}
		return nil, fmt.Errorf("autorun: querying session: %w", err)
	}
	return &s, nil
}

// GetLatestSession retrieves the most recent autorun session.
func GetLatestSession(ctx context.Context, d *db.DB) (*db.AutorunSession, error) {
	var s db.AutorunSession
	err := d.QueryRow(ctx,
		`SELECT id, batch_file, batch_name, status, max_session_workers,
		        total_tasks, completed_tasks, failed_tasks, created_at, completed_at
		 FROM autorun_sessions ORDER BY created_at DESC LIMIT 1`,
	).Scan(&s.ID, &s.BatchFile, &s.BatchName, &s.Status, &s.MaxSessionWorkers,
		&s.TotalTasks, &s.CompletedTasks, &s.FailedTasks, &s.CreatedAt, &s.CompletedAt)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return nil, ErrSessionNotFound
		}
		return nil, fmt.Errorf("autorun: querying latest session: %w", err)
	}
	return &s, nil
}

// ListSessions retrieves all autorun sessions ordered by creation time (newest first).
func ListSessions(ctx context.Context, d *db.DB, limit int) ([]db.AutorunSession, error) {
	if limit <= 0 {
		limit = 20
	}

	rows, err := d.Query(ctx,
		`SELECT id, batch_file, batch_name, status, max_session_workers,
		        total_tasks, completed_tasks, failed_tasks, created_at, completed_at
		 FROM autorun_sessions ORDER BY created_at DESC LIMIT ?`, limit,
	)
	if err != nil {
		return nil, fmt.Errorf("autorun: listing sessions: %w", err)
	}
	defer rows.Close()

	var sessions []db.AutorunSession
	for rows.Next() {
		var s db.AutorunSession
		if err := rows.Scan(&s.ID, &s.BatchFile, &s.BatchName, &s.Status, &s.MaxSessionWorkers,
			&s.TotalTasks, &s.CompletedTasks, &s.FailedTasks, &s.CreatedAt, &s.CompletedAt); err != nil {
			return nil, fmt.Errorf("autorun: scanning session row: %w", err)
		}
		sessions = append(sessions, s)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("autorun: iterating sessions: %w", err)
	}

	return sessions, nil
}

// GetSessionWorkers retrieves all workers for a given autorun session.
func GetSessionWorkers(ctx context.Context, d *db.DB, sessionID string) ([]db.AutorunWorker, error) {
	rows, err := d.Query(ctx,
		`SELECT id, session_id, worker_num, task_id, status, tmux_session,
		        worktree_path, pr_number, started_at, completed_at
		 FROM autorun_workers WHERE session_id = ? ORDER BY worker_num`, sessionID,
	)
	if err != nil {
		return nil, fmt.Errorf("autorun: listing workers: %w", err)
	}
	defer rows.Close()

	var workers []db.AutorunWorker
	for rows.Next() {
		var w db.AutorunWorker
		if err := rows.Scan(&w.ID, &w.SessionID, &w.WorkerNum, &w.TaskID, &w.Status,
			&w.TmuxSession, &w.WorktreePath, &w.PRNumber, &w.StartedAt, &w.CompletedAt); err != nil {
			return nil, fmt.Errorf("autorun: scanning worker row: %w", err)
		}
		workers = append(workers, w)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("autorun: iterating workers: %w", err)
	}

	return workers, nil
}

// nullString converts a string to sql.NullString, treating empty as null.
func nullString(s string) sql.NullString {
	if s == "" {
		return sql.NullString{}
	}
	return sql.NullString{String: s, Valid: true}
}

// nullInt64 converts an int64 to sql.NullInt64, treating 0 as null.
func nullInt64(n int64) sql.NullInt64 {
	if n == 0 {
		return sql.NullInt64{}
	}
	return sql.NullInt64{Int64: n, Valid: true}
}
