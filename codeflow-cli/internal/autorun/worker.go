package autorun

import (
	"context"
	"errors"
	"fmt"
	"time"
)

// Sentinel errors for worker operations.
var (
	// ErrWorkerTimeout indicates the worker exceeded its timeout.
	ErrWorkerTimeout = errors.New("autorun: worker timeout")

	// ErrTmuxNotFound indicates tmux is not available.
	ErrTmuxNotFound = errors.New("autorun: tmux not found")

	// ErrClaudeNotFound indicates the Claude CLI is not available.
	ErrClaudeNotFound = errors.New("autorun: claude CLI not found")
)

// DefaultWorkerTimeout is the default timeout for a single worker.
const DefaultWorkerTimeout = 60 * time.Minute

// TmuxRunner executes commands via tmux. This interface allows testing
// without actual tmux sessions.
type TmuxRunner interface {
	// CreateSession creates a new tmux session with the given name.
	CreateSession(ctx context.Context, name string) error

	// SendCommand sends a command string to the named tmux session.
	SendCommand(ctx context.Context, session string, command string) error

	// KillSession terminates a tmux session.
	KillSession(ctx context.Context, name string) error

	// HasSession checks if a tmux session exists.
	HasSession(ctx context.Context, name string) (bool, error)
}

// ClaudeInvoker defines the interface for invoking Claude Code in a worker.
type ClaudeInvoker interface {
	// Invoke runs Claude Code with the given prompt and returns when complete.
	Invoke(ctx context.Context, cfg InvokeConfig) (*InvokeResult, error)
}

// InvokeConfig contains parameters for a Claude Code invocation.
type InvokeConfig struct {
	WorkDir     string
	Prompt      string
	SessionID   string
	TaskID      string
	AutoMerge   bool
	Target      string
	TmuxSession string
}

// InvokeResult contains the outcome of a Claude Code invocation.
type InvokeResult struct {
	ExitCode   int
	PRNumber   int64
	PRURL      string
	BranchName string
	Output     string
}

// TmuxWorker implements WorkerRunner using real tmux sessions and Claude Code.
type TmuxWorker struct {
	Tmux    TmuxRunner
	Claude  ClaudeInvoker
	Timeout time.Duration
}

// NewTmuxWorker creates a new TmuxWorker with sensible defaults.
func NewTmuxWorker(tmux TmuxRunner, claude ClaudeInvoker) *TmuxWorker {
	return &TmuxWorker{
		Tmux:    tmux,
		Claude:  claude,
		Timeout: DefaultWorkerTimeout,
	}
}

// Run executes a single task in an isolated tmux session.
func (w *TmuxWorker) Run(ctx context.Context, cfg WorkerConfig) (*WorkerResult, error) {
	tmuxName := fmt.Sprintf("%s-%d", cfg.TmuxPrefix, cfg.WorkerNum)

	// Apply timeout to the context.
	timeout := w.Timeout
	if timeout <= 0 {
		timeout = DefaultWorkerTimeout
	}
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	// Create the tmux session.
	if err := w.Tmux.CreateSession(ctx, tmuxName); err != nil {
		return nil, fmt.Errorf("autorun: creating tmux session %s: %w", tmuxName, err)
	}

	// Ensure cleanup on completion.
	defer func() {
		// Use background context for cleanup since the original may be cancelled.
		cleanupCtx := context.Background()
		_ = w.Tmux.KillSession(cleanupCtx, tmuxName)
	}()

	// Invoke Claude Code.
	result, err := w.Claude.Invoke(ctx, InvokeConfig{
		WorkDir:     ".",
		Prompt:      fmt.Sprintf("Execute autorun task %s", cfg.TaskID),
		SessionID:   cfg.SessionID,
		TaskID:      cfg.TaskID,
		AutoMerge:   cfg.AutoMerge,
		Target:      cfg.Target,
		TmuxSession: tmuxName,
	})

	if err != nil {
		if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			return &WorkerResult{
				WorkerID: cfg.WorkerID,
				TaskID:   cfg.TaskID,
				Status:   "timeout",
				ExitCode: 124, // standard timeout exit code
				Error:    "worker exceeded timeout",
			}, nil
		}
		return nil, fmt.Errorf("autorun: invoking Claude for task %s: %w", cfg.TaskID, err)
	}

	status := "completed"
	if result.ExitCode != 0 {
		status = "failed"
	}

	return &WorkerResult{
		WorkerID:   cfg.WorkerID,
		TaskID:     cfg.TaskID,
		Status:     status,
		ExitCode:   result.ExitCode,
		PRNumber:   result.PRNumber,
		PRURL:      result.PRURL,
		BranchName: result.BranchName,
	}, nil
}

// Real* implementations (RealTmuxRunner, RealClaudeInvoker) are in worker_real.go.
