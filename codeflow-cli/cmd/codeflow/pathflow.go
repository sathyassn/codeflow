package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"path/filepath"

	"github.com/codeflow/codeflow-cli/internal/pathflow"
	"github.com/spf13/cobra"
)

// newPathflowCmd creates the top-level "pathflow" command with checkpoint subcommands.
func newPathflowCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "pathflow",
		Short: "PathFlow checkpoint and sentinel operations",
		Long:  "Manage PathFlow phase checkpoints: initialize, register, complete, skip, and query task status.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newCheckpointCmd())

	return cmd
}

// newCheckpointCmd creates the "pathflow checkpoint" command group.
func newCheckpointCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "checkpoint",
		Short: "Checkpoint CRUD operations",
		Long:  "Manage phase task checkpoints: initialization, registration, completion, skipping, and status queries.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newCheckpointInitCmd())
	cmd.AddCommand(newCheckpointRegisterCmd())
	cmd.AddCommand(newCheckpointCompleteCmd())
	cmd.AddCommand(newCheckpointSkipCmd())
	cmd.AddCommand(newCheckpointStatusCmd())

	return cmd
}

// newCheckpointInitCmd creates "pathflow checkpoint init".
func newCheckpointInitCmd() *cobra.Command {
	var (
		sessionDir string
		configPath string
	)

	cmd := &cobra.Command{
		Use:   "init",
		Short: "Initialize checkpoint for all phases",
		Long:  "Reads pathflow-config.json and initializes pathflow-phase-tasks.json with all 7 phases. Idempotent.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runCheckpointInit(cmd.OutOrStdout(), sessionDir, configPath)
		},
	}

	cmd.Flags().StringVar(&sessionDir, "session-dir", "", "path to session directory containing pathflow-phase-tasks.json")
	cmd.Flags().StringVar(&configPath, "config", "", "path to pathflow-config.json")
	_ = cmd.MarkFlagRequired("session-dir")
	_ = cmd.MarkFlagRequired("config")

	return cmd
}

// runCheckpointInit implements the checkpoint init logic.
func runCheckpointInit(w io.Writer, sessionDir, configPath string) error {
	cpPath := checkpointPath(sessionDir)

	cp := pathflow.NewCheckpoint()
	if err := cp.InitAllPhases(cpPath, configPath); err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("initializing checkpoint: %w", err)}
	}

	fmt.Fprintln(w, "Checkpoint initialized for all phases")
	return nil
}

// newCheckpointRegisterCmd creates "pathflow checkpoint register".
func newCheckpointRegisterCmd() *cobra.Command {
	var (
		sessionDir  string
		sentinelDir string
		phase       string
		task        string
	)

	cmd := &cobra.Command{
		Use:   "register",
		Short: "Register a task in the checkpoint",
		Long:  "Registers a phase task. Blocks cross-phase registration if prior sentinel missing (exit code 2).",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runCheckpointRegister(cmd.OutOrStdout(), sessionDir, sentinelDir, phase, task)
		},
	}

	cmd.Flags().StringVar(&sessionDir, "session-dir", "", "path to session directory")
	cmd.Flags().StringVar(&sentinelDir, "sentinel-dir", "", "path to sentinel directory")
	cmd.Flags().StringVar(&phase, "phase", "", "phase ID (e.g., PF1)")
	cmd.Flags().StringVar(&task, "task", "", "task ID (e.g., PF1-TSK-01)")
	_ = cmd.MarkFlagRequired("session-dir")
	_ = cmd.MarkFlagRequired("sentinel-dir")
	_ = cmd.MarkFlagRequired("task")

	return cmd
}

// runCheckpointRegister implements the checkpoint register logic.
func runCheckpointRegister(w io.Writer, sessionDir, sentinelDir, _, task string) error {
	cpPath := checkpointPath(sessionDir)

	cp := pathflow.NewCheckpoint()
	if err := cp.RegisterTask(cpPath, sentinelDir, task); err != nil {
		// Cross-phase block returns exit code 2 (matches shell convention).
		if isCrossPhaseBlock(err) {
			return &exitError{code: ExitConfigError, err: err}
		}
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("registering task: %w", err)}
	}

	fmt.Fprintf(w, "Task %s registered\n", task)
	return nil
}

// newCheckpointCompleteCmd creates "pathflow checkpoint complete".
func newCheckpointCompleteCmd() *cobra.Command {
	var (
		sessionDir  string
		sentinelDir string
		phase       string
		task        string
	)

	cmd := &cobra.Command{
		Use:   "complete",
		Short: "Mark a task as completed",
		Long:  "Marks a task complete. Creates phase sentinel automatically when all tasks in the phase are done.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runCheckpointComplete(cmd.OutOrStdout(), sessionDir, sentinelDir, phase, task)
		},
	}

	cmd.Flags().StringVar(&sessionDir, "session-dir", "", "path to session directory")
	cmd.Flags().StringVar(&sentinelDir, "sentinel-dir", "", "path to sentinel directory")
	cmd.Flags().StringVar(&phase, "phase", "", "phase ID (e.g., PF1)")
	cmd.Flags().StringVar(&task, "task", "", "task ID (e.g., PF1-TSK-01)")
	_ = cmd.MarkFlagRequired("session-dir")
	_ = cmd.MarkFlagRequired("sentinel-dir")
	_ = cmd.MarkFlagRequired("task")

	return cmd
}

// runCheckpointComplete implements the checkpoint complete logic.
func runCheckpointComplete(w io.Writer, sessionDir, sentinelDir, _, task string) error {
	cpPath := checkpointPath(sessionDir)

	cp := pathflow.NewCheckpoint()
	if err := cp.CompleteTask(cpPath, sentinelDir, task); err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("completing task: %w", err)}
	}

	fmt.Fprintf(w, "Task %s completed\n", task)
	return nil
}

// newCheckpointSkipCmd creates "pathflow checkpoint skip".
func newCheckpointSkipCmd() *cobra.Command {
	var (
		sessionDir  string
		sentinelDir string
		phase       string
		task        string
	)

	cmd := &cobra.Command{
		Use:   "skip",
		Short: "Mark a task as skipped",
		Long:  "Marks a conditional task as skipped. Counts as done for phase completion logic.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runCheckpointSkip(cmd.OutOrStdout(), sessionDir, sentinelDir, phase, task)
		},
	}

	cmd.Flags().StringVar(&sessionDir, "session-dir", "", "path to session directory")
	cmd.Flags().StringVar(&sentinelDir, "sentinel-dir", "", "path to sentinel directory")
	cmd.Flags().StringVar(&phase, "phase", "", "phase ID (e.g., PF1)")
	cmd.Flags().StringVar(&task, "task", "", "task ID (e.g., PF1-TSK-01)")
	_ = cmd.MarkFlagRequired("session-dir")
	_ = cmd.MarkFlagRequired("sentinel-dir")
	_ = cmd.MarkFlagRequired("task")

	return cmd
}

// runCheckpointSkip implements the checkpoint skip logic.
func runCheckpointSkip(w io.Writer, sessionDir, sentinelDir, _, task string) error {
	cpPath := checkpointPath(sessionDir)

	cp := pathflow.NewCheckpoint()
	if err := cp.SkipTask(cpPath, sentinelDir, task); err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("skipping task: %w", err)}
	}

	fmt.Fprintf(w, "Task %s skipped\n", task)
	return nil
}

// newCheckpointStatusCmd creates "pathflow checkpoint status".
func newCheckpointStatusCmd() *cobra.Command {
	var (
		sessionDir string
		phase      string
	)

	cmd := &cobra.Command{
		Use:   "status",
		Short: "Show checkpoint status for a phase",
		Long:  "Outputs JSON status of all tasks in a phase, including expected, registered, completed, and skipped.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runCheckpointStatus(cmd.OutOrStdout(), sessionDir, phase)
		},
	}

	cmd.Flags().StringVar(&sessionDir, "session-dir", "", "path to session directory")
	cmd.Flags().StringVar(&phase, "phase", "", "phase ID (e.g., PF1)")
	_ = cmd.MarkFlagRequired("session-dir")
	_ = cmd.MarkFlagRequired("phase")

	return cmd
}

// runCheckpointStatus implements the checkpoint status logic.
func runCheckpointStatus(w io.Writer, sessionDir, phase string) error {
	cpPath := checkpointPath(sessionDir)

	cp := pathflow.NewCheckpoint()
	pc, err := cp.GetStatus(cpPath, phase)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("getting status: %w", err)}
	}

	data, err := json.MarshalIndent(pc, "", "  ")
	if err != nil {
		return &exitError{code: ExitInternalError, err: fmt.Errorf("marshaling status: %w", err)}
	}

	fmt.Fprintln(w, string(data))
	return nil
}

// checkpointPath returns the standard checkpoint file path within a session directory.
func checkpointPath(sessionDir string) string {
	return filepath.Join(sessionDir, "pathflow-phase-tasks.json")
}

// isCrossPhaseBlock checks if an error is a cross-phase registration block.
func isCrossPhaseBlock(err error) bool {
	return errors.Is(err, pathflow.ErrCrossPhaseBlock)
}
