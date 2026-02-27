package autorun

import (
	"context"
	"errors"
	"fmt"
	"os/exec"
	"strings"
)

// RealTmuxRunner implements TmuxRunner using actual tmux commands.
type RealTmuxRunner struct{}

// CreateSession creates a new detached tmux session.
func (r *RealTmuxRunner) CreateSession(ctx context.Context, name string) error {
	if _, err := exec.LookPath("tmux"); err != nil {
		return ErrTmuxNotFound
	}
	cmd := exec.CommandContext(ctx, "tmux", "new-session", "-d", "-s", name)
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("tmux new-session: %w", err)
	}
	return nil
}

// SendCommand sends a command to an existing tmux session.
func (r *RealTmuxRunner) SendCommand(ctx context.Context, session string, command string) error {
	cmd := exec.CommandContext(ctx, "tmux", "send-keys", "-t", session, command, "Enter")
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("tmux send-keys: %w", err)
	}
	return nil
}

// KillSession terminates a tmux session.
func (r *RealTmuxRunner) KillSession(ctx context.Context, name string) error {
	cmd := exec.CommandContext(ctx, "tmux", "kill-session", "-t", name)
	if err := cmd.Run(); err != nil {
		// Not an error if the session doesn't exist.
		if strings.Contains(err.Error(), "can't find session") {
			return nil
		}
		return fmt.Errorf("tmux kill-session: %w", err)
	}
	return nil
}

// HasSession checks if a named tmux session exists.
func (r *RealTmuxRunner) HasSession(ctx context.Context, name string) (bool, error) {
	cmd := exec.CommandContext(ctx, "tmux", "has-session", "-t", name)
	err := cmd.Run()
	if err == nil {
		return true, nil
	}
	// Exit code 1 means session doesn't exist.
	var exitErr *exec.ExitError
	if errors.As(err, &exitErr) && exitErr.ExitCode() == 1 {
		return false, nil
	}
	return false, fmt.Errorf("tmux has-session: %w", err)
}

// RealClaudeInvoker implements ClaudeInvoker using the actual Claude CLI.
type RealClaudeInvoker struct{}

// Invoke runs Claude Code as a subprocess.
func (r *RealClaudeInvoker) Invoke(ctx context.Context, cfg InvokeConfig) (*InvokeResult, error) {
	claudePath, err := exec.LookPath("claude")
	if err != nil {
		return nil, ErrClaudeNotFound
	}

	args := []string{
		"--print",
		"--dangerously-skip-permissions",
		"-p", cfg.Prompt,
	}

	cmd := exec.CommandContext(ctx, claudePath, args...)
	cmd.Dir = cfg.WorkDir

	output, err := cmd.CombinedOutput()

	exitCode := 0
	if err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) {
			exitCode = exitErr.ExitCode()
		} else {
			return nil, fmt.Errorf("running claude: %w", err)
		}
	}

	return &InvokeResult{
		ExitCode: exitCode,
		Output:   string(output),
	}, nil
}
