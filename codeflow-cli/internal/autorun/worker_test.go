package autorun

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

// mockTmuxRunner implements TmuxRunner for testing.
type mockTmuxRunner struct {
	sessions      map[string]bool
	createErr     error
	killErr       error
	commands      []tmuxCommand
	createCalled  int
	killCalled    int
}

type tmuxCommand struct {
	session string
	command string
}

func newMockTmuxRunner() *mockTmuxRunner {
	return &mockTmuxRunner{
		sessions: make(map[string]bool),
	}
}

func (m *mockTmuxRunner) CreateSession(_ context.Context, name string) error {
	m.createCalled++
	if m.createErr != nil {
		return m.createErr
	}
	m.sessions[name] = true
	return nil
}

func (m *mockTmuxRunner) SendCommand(_ context.Context, session string, command string) error {
	m.commands = append(m.commands, tmuxCommand{session: session, command: command})
	return nil
}

func (m *mockTmuxRunner) KillSession(_ context.Context, name string) error {
	m.killCalled++
	if m.killErr != nil {
		return m.killErr
	}
	delete(m.sessions, name)
	return nil
}

func (m *mockTmuxRunner) HasSession(_ context.Context, name string) (bool, error) {
	return m.sessions[name], nil
}

// mockClaudeInvoker implements ClaudeInvoker for testing.
type mockClaudeInvoker struct {
	result    *InvokeResult
	err       error
	delay     time.Duration
	callCount int
	lastCfg   InvokeConfig
}

func (m *mockClaudeInvoker) Invoke(ctx context.Context, cfg InvokeConfig) (*InvokeResult, error) {
	m.callCount++
	m.lastCfg = cfg

	if m.delay > 0 {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-time.After(m.delay):
		}
	}

	if m.err != nil {
		return nil, m.err
	}
	if m.result != nil {
		return m.result, nil
	}
	return &InvokeResult{ExitCode: 0}, nil
}

func TestTmuxWorker_SuccessfulRun(t *testing.T) {
	t.Parallel()

	tmux := newMockTmuxRunner()
	claude := &mockClaudeInvoker{
		result: &InvokeResult{
			ExitCode:   0,
			PRNumber:   42,
			PRURL:      "https://github.com/test/pr/42",
			BranchName: "feat/test",
		},
	}

	worker := NewTmuxWorker(tmux, claude)

	ctx := t.Context()
	result, err := worker.Run(ctx, WorkerConfig{
		SessionID:  "ars-test",
		WorkerID:   "arw-001",
		WorkerNum:  1,
		TaskID:     "task-001",
		BatchName:  "test",
		TmuxPrefix: "codeflow-worker",
	})

	if err != nil {
		t.Fatalf("Run: %v", err)
	}

	if result.Status != "completed" {
		t.Errorf("Status = %q, want completed", result.Status)
	}
	if result.PRNumber != 42 {
		t.Errorf("PRNumber = %d, want 42", result.PRNumber)
	}
	if result.BranchName != "feat/test" {
		t.Errorf("BranchName = %q, want feat/test", result.BranchName)
	}

	// Verify tmux session was created and cleaned up.
	if tmux.createCalled != 1 {
		t.Errorf("tmux CreateSession called %d times, want 1", tmux.createCalled)
	}
	if tmux.killCalled != 1 {
		t.Errorf("tmux KillSession called %d times, want 1", tmux.killCalled)
	}

	// Verify Claude was invoked.
	if claude.callCount != 1 {
		t.Errorf("Claude invoked %d times, want 1", claude.callCount)
	}
	if claude.lastCfg.TaskID != "task-001" {
		t.Errorf("Claude TaskID = %q, want task-001", claude.lastCfg.TaskID)
	}
}

func TestTmuxWorker_ClaudeInvocationFailure(t *testing.T) {
	t.Parallel()

	tmux := newMockTmuxRunner()
	claude := &mockClaudeInvoker{
		result: &InvokeResult{
			ExitCode: 1,
		},
	}

	worker := NewTmuxWorker(tmux, claude)

	ctx := t.Context()
	result, err := worker.Run(ctx, WorkerConfig{
		SessionID:  "ars-test",
		WorkerID:   "arw-001",
		WorkerNum:  1,
		TaskID:     "task-001",
		TmuxPrefix: "codeflow-worker",
	})

	if err != nil {
		t.Fatalf("Run: %v", err)
	}

	if result.Status != "failed" {
		t.Errorf("Status = %q, want failed", result.Status)
	}
	if result.ExitCode != 1 {
		t.Errorf("ExitCode = %d, want 1", result.ExitCode)
	}

	// Cleanup should still happen.
	if tmux.killCalled != 1 {
		t.Errorf("tmux KillSession called %d times, want 1", tmux.killCalled)
	}
}

func TestTmuxWorker_ResultCollection(t *testing.T) {
	t.Parallel()

	tmux := newMockTmuxRunner()
	claude := &mockClaudeInvoker{
		result: &InvokeResult{
			ExitCode:   0,
			PRNumber:   123,
			PRURL:      "https://github.com/org/repo/pull/123",
			BranchName: "feat/my-feature",
			Output:     "Task completed successfully",
		},
	}

	worker := NewTmuxWorker(tmux, claude)

	ctx := t.Context()
	result, err := worker.Run(ctx, WorkerConfig{
		SessionID:  "ars-collect",
		WorkerID:   "arw-collect",
		WorkerNum:  3,
		TaskID:     "task-collect",
		TmuxPrefix: "codeflow-worker",
	})

	if err != nil {
		t.Fatalf("Run: %v", err)
	}

	if result.WorkerID != "arw-collect" {
		t.Errorf("WorkerID = %q, want arw-collect", result.WorkerID)
	}
	if result.TaskID != "task-collect" {
		t.Errorf("TaskID = %q, want task-collect", result.TaskID)
	}
	if result.PRNumber != 123 {
		t.Errorf("PRNumber = %d, want 123", result.PRNumber)
	}
	if result.PRURL != "https://github.com/org/repo/pull/123" {
		t.Errorf("PRURL = %q, want https://github.com/org/repo/pull/123", result.PRURL)
	}
}

func TestTmuxWorker_TimeoutHandling(t *testing.T) {
	t.Parallel()

	tmux := newMockTmuxRunner()
	claude := &mockClaudeInvoker{
		delay: 5 * time.Second, // Long delay that should trigger timeout.
	}

	worker := &TmuxWorker{
		Tmux:    tmux,
		Claude:  claude,
		Timeout: 100 * time.Millisecond, // Very short timeout.
	}

	ctx := t.Context()
	result, err := worker.Run(ctx, WorkerConfig{
		SessionID:  "ars-timeout",
		WorkerID:   "arw-timeout",
		WorkerNum:  1,
		TaskID:     "task-timeout",
		TmuxPrefix: "codeflow-worker",
	})

	if err != nil {
		t.Fatalf("Run should not return error for timeout, got: %v", err)
	}

	if result.Status != "timeout" {
		t.Errorf("Status = %q, want timeout", result.Status)
	}
	if result.ExitCode != 124 {
		t.Errorf("ExitCode = %d, want 124 (timeout)", result.ExitCode)
	}

	// Cleanup should still happen.
	if tmux.killCalled != 1 {
		t.Errorf("tmux KillSession called %d times, want 1", tmux.killCalled)
	}
}

func TestTmuxWorker_TmuxSessionCleanup(t *testing.T) {
	t.Parallel()

	tmux := newMockTmuxRunner()
	claude := &mockClaudeInvoker{}

	worker := NewTmuxWorker(tmux, claude)

	ctx := t.Context()
	_, err := worker.Run(ctx, WorkerConfig{
		SessionID:  "ars-cleanup",
		WorkerID:   "arw-cleanup",
		WorkerNum:  2,
		TaskID:     "task-cleanup",
		TmuxPrefix: "codeflow-worker",
	})

	if err != nil {
		t.Fatalf("Run: %v", err)
	}

	// The tmux session should have been created with the correct name.
	if tmux.createCalled != 1 {
		t.Errorf("CreateSession called %d times, want 1", tmux.createCalled)
	}

	// The session should be cleaned up after completion.
	if tmux.killCalled != 1 {
		t.Errorf("KillSession called %d times, want 1", tmux.killCalled)
	}

	// Verify no sessions remain.
	if len(tmux.sessions) != 0 {
		t.Errorf("sessions remaining = %d, want 0", len(tmux.sessions))
	}
}

func TestTmuxWorker_TmuxCreateError(t *testing.T) {
	t.Parallel()

	tmux := newMockTmuxRunner()
	tmux.createErr = errors.New("tmux not available")
	claude := &mockClaudeInvoker{}

	worker := NewTmuxWorker(tmux, claude)

	ctx := t.Context()
	_, err := worker.Run(ctx, WorkerConfig{
		SessionID:  "ars-err",
		WorkerID:   "arw-err",
		WorkerNum:  1,
		TaskID:     "task-err",
		TmuxPrefix: "codeflow-worker",
	})

	if err == nil {
		t.Fatal("expected error when tmux create fails, got nil")
	}
	if !strings.Contains(err.Error(), "tmux not available") {
		t.Errorf("error = %v, want 'tmux not available' in message", err)
	}
}

func TestTmuxWorker_TmuxSessionNaming(t *testing.T) {
	t.Parallel()

	tmux := newMockTmuxRunner()
	claude := &mockClaudeInvoker{}

	worker := NewTmuxWorker(tmux, claude)

	ctx := t.Context()
	_, err := worker.Run(ctx, WorkerConfig{
		SessionID:  "ars-naming",
		WorkerID:   "arw-naming",
		WorkerNum:  5,
		TaskID:     "task-naming",
		TmuxPrefix: "codeflow-worker",
	})

	if err != nil {
		t.Fatalf("Run: %v", err)
	}

	// Verify Claude was invoked with the correct tmux session name.
	expectedTmux := "codeflow-worker-5"
	if claude.lastCfg.TmuxSession != expectedTmux {
		t.Errorf("TmuxSession = %q, want %q", claude.lastCfg.TmuxSession, expectedTmux)
	}
}

func TestTmuxWorker_ClaudeError(t *testing.T) {
	t.Parallel()

	tmux := newMockTmuxRunner()
	claude := &mockClaudeInvoker{
		err: errors.New("connection refused"),
	}

	worker := NewTmuxWorker(tmux, claude)

	ctx := t.Context()
	_, err := worker.Run(ctx, WorkerConfig{
		SessionID:  "ars-err",
		WorkerID:   "arw-err",
		WorkerNum:  1,
		TaskID:     "task-err",
		TmuxPrefix: "codeflow-worker",
	})

	if err == nil {
		t.Fatal("expected error when Claude returns non-timeout error, got nil")
	}
	if !strings.Contains(err.Error(), "connection refused") {
		t.Errorf("error = %v, want 'connection refused' in message", err)
	}

	// Cleanup should still happen.
	if tmux.killCalled != 1 {
		t.Errorf("tmux KillSession called %d times, want 1", tmux.killCalled)
	}
}

func TestNewTmuxWorker_Defaults(t *testing.T) {
	t.Parallel()

	tmux := newMockTmuxRunner()
	claude := &mockClaudeInvoker{}

	worker := NewTmuxWorker(tmux, claude)

	if worker.Timeout != DefaultWorkerTimeout {
		t.Errorf("Timeout = %v, want %v", worker.Timeout, DefaultWorkerTimeout)
	}
}
