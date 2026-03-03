package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"path/filepath"

	"github.com/codeflow/codeflow-cli/internal/pathflow"
	"github.com/spf13/cobra"
)

// defaultPathflowLogsDir returns the default directory for pathflow event logs.
func defaultPathflowLogsDir() string {
	return filepath.Join(".state", "logs")
}

// newPathflowCmd creates the top-level "pathflow" command with checkpoint and transition subcommands.
func newPathflowCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "pathflow",
		Short: "PathFlow checkpoint, sentinel, and transition operations",
		Long:  "Manage PathFlow phase checkpoints, sentinels, and lifecycle transition events.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newCheckpointCmd())
	cmd.AddCommand(newPhaseTransitionCmd())
	cmd.AddCommand(newStageTransitionCmd())
	cmd.AddCommand(newSessionRegisterCmd())
	cmd.AddCommand(newTaskUpdateCmd())
	cmd.AddCommand(newSessionMetadataCmd())

	return cmd
}

// newPhaseTransitionCmd creates "pathflow phase-transition".
func newPhaseTransitionCmd() *cobra.Command {
	var (
		sessionID string
		phase     string
		status    string
		logsDir   string
	)

	cmd := &cobra.Command{
		Use:   "phase-transition",
		Short: "Record a phase transition event",
		Long:  "Writes a phase_transition event to pathflow-events.jsonl.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runPhaseTransition(cmd.Context(), cmd.OutOrStdout(), cmd.ErrOrStderr(), logsDir, sessionID, phase, status)
		},
	}

	cmd.Flags().StringVarP(&sessionID, "session-id", "s", "", "session ID (required)")
	cmd.Flags().StringVarP(&phase, "phase", "p", "", "phase: PF1-INIT through PF7-END (required)")
	cmd.Flags().StringVarP(&status, "status", "t", "", "status: entered, completed, or skipped (required)")
	cmd.Flags().StringVar(&logsDir, "logs-dir", defaultPathflowLogsDir(), "path to pathflow logs directory")
	_ = cmd.MarkFlagRequired("session-id")
	_ = cmd.MarkFlagRequired("phase")
	_ = cmd.MarkFlagRequired("status")

	return cmd
}

func runPhaseTransition(ctx context.Context, w io.Writer, errW io.Writer, logsDir, sessionID, phase, status string) error {
	tw, err := pathflow.NewTransitionWriter(ctx, logsDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: err}
	}

	params := pathflow.PhaseTransitionParams{
		SessionID: sessionID,
		Phase:     phase,
		Status:    status,
	}
	if err := tw.RecordPhaseTransition(params); err != nil {
		fmt.Fprintf(errW, `{"error":%q}`+"\n", err.Error())
		return &exitError{code: ExitConfigError, err: err}
	}

	fmt.Fprintf(w, `{"status":"recorded","phase":%q,"transition":%q}`+"\n", phase, status)
	return nil
}

// newStageTransitionCmd creates "pathflow stage-transition".
func newStageTransitionCmd() *cobra.Command {
	var (
		sessionID string
		stage     string
		status    string
		iteration int
		verdict   string
		logsDir   string
	)

	cmd := &cobra.Command{
		Use:   "stage-transition",
		Short: "Record a stage transition event",
		Long:  "Writes a stage_transition event to pathflow-events.jsonl.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runStageTransition(cmd.Context(), cmd.OutOrStdout(), logsDir, sessionID, stage, status, iteration, verdict)
		},
	}

	cmd.Flags().StringVarP(&sessionID, "session-id", "s", "", "session ID (required)")
	cmd.Flags().StringVarP(&stage, "stage", "g", "", "stage: WS-DEV, WS-PLAN, WS-DOCS, WS-TEST, WS-REV, WS-QA (required)")
	cmd.Flags().StringVarP(&status, "status", "t", "", "status: pending, in_progress, complete, or failed (required)")
	cmd.Flags().IntVarP(&iteration, "iteration", "i", 1, "iteration number (default: 1)")
	cmd.Flags().StringVarP(&verdict, "verdict", "v", "", "verdict: pass, fail, approved, or changes_requested (optional)")
	cmd.Flags().StringVar(&logsDir, "logs-dir", defaultPathflowLogsDir(), "path to pathflow logs directory")
	_ = cmd.MarkFlagRequired("session-id")
	_ = cmd.MarkFlagRequired("stage")
	_ = cmd.MarkFlagRequired("status")

	return cmd
}

func runStageTransition(ctx context.Context, w io.Writer, logsDir, sessionID, stage, status string, iteration int, verdict string) error {
	tw, err := pathflow.NewTransitionWriter(ctx, logsDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: err}
	}

	params := pathflow.StageTransitionParams{
		SessionID: sessionID,
		Stage:     stage,
		Status:    status,
		Iteration: iteration,
		Verdict:   verdict,
	}
	if err := tw.RecordStageTransition(params); err != nil {
		return &exitError{code: ExitConfigError, err: err}
	}

	fmt.Fprintf(w, `{"status":"recorded","stage":%q,"transition":%q,"iteration":%d}`+"\n", stage, status, iteration)
	return nil
}

// newSessionRegisterCmd creates "pathflow session-register".
func newSessionRegisterCmd() *cobra.Command {
	var (
		sessionID string
		mode      string
		logsDir   string
	)

	cmd := &cobra.Command{
		Use:   "session-register",
		Short: "Register a new PathFlow session",
		Long:  "Writes two session_register events to pathflow-events.jsonl: tracking_level and interaction_mode.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSessionRegister(cmd.Context(), cmd.OutOrStdout(), cmd.ErrOrStderr(), logsDir, sessionID, mode)
		},
	}

	cmd.Flags().StringVarP(&sessionID, "session-id", "s", "", "session ID (required)")
	cmd.Flags().StringVarP(&mode, "mode", "m", "interactive", "interaction mode: interactive or autorun")
	cmd.Flags().StringVar(&logsDir, "logs-dir", defaultPathflowLogsDir(), "path to pathflow logs directory")
	_ = cmd.MarkFlagRequired("session-id")

	return cmd
}

func runSessionRegister(ctx context.Context, w io.Writer, errW io.Writer, logsDir, sessionID, mode string) error {
	tw, err := pathflow.NewTransitionWriter(ctx, logsDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: err}
	}

	params := pathflow.SessionRegisterParams{
		SessionID: sessionID,
		Mode:      mode,
	}
	if err := tw.RegisterSession(params); err != nil {
		fmt.Fprintf(errW, `{"error":%q}`+"\n", err.Error())
		return &exitError{code: ExitConfigError, err: err}
	}

	fmt.Fprintf(w, `{"status":"registered","session_id":%q,"tracking_level":"pending","interaction_mode":%q}`+"\n", sessionID, mode)
	return nil
}

// newTaskUpdateCmd creates "pathflow task-update".
func newTaskUpdateCmd() *cobra.Command {
	var (
		sessionID  string
		taskID     string
		taskStatus string
		logsDir    string
	)

	cmd := &cobra.Command{
		Use:   "task-update",
		Short: "Record a PathFlow task status update",
		Long:  "Writes a pathflow_task_update event to pathflow-events.jsonl.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runTaskUpdate(cmd.Context(), cmd.OutOrStdout(), logsDir, sessionID, taskID, taskStatus)
		},
	}

	cmd.Flags().StringVarP(&sessionID, "session-id", "s", "", "session ID (required)")
	cmd.Flags().StringVarP(&taskID, "task-id", "k", "", "PathFlow task ID, e.g., PF3-TSK-01 (required)")
	cmd.Flags().StringVarP(&taskStatus, "task-status", "t", "", "status: pending, in_progress, completed, skipped, or blocked (required)")
	cmd.Flags().StringVar(&logsDir, "logs-dir", defaultPathflowLogsDir(), "path to pathflow logs directory")
	_ = cmd.MarkFlagRequired("session-id")
	_ = cmd.MarkFlagRequired("task-id")
	_ = cmd.MarkFlagRequired("task-status")

	return cmd
}

func runTaskUpdate(ctx context.Context, w io.Writer, logsDir, sessionID, taskID, taskStatus string) error {
	tw, err := pathflow.NewTransitionWriter(ctx, logsDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: err}
	}

	params := pathflow.TaskUpdateParams{
		SessionID:  sessionID,
		TaskID:     taskID,
		TaskStatus: taskStatus,
	}
	if err := tw.RecordTaskUpdate(params); err != nil {
		return &exitError{code: ExitConfigError, err: err}
	}

	fmt.Fprintf(w, `{"status":"recorded","task_id":%q,"task_status":%q}`+"\n", taskID, taskStatus)
	return nil
}

// newSessionMetadataCmd creates "pathflow session-metadata".
func newSessionMetadataCmd() *cobra.Command {
	var (
		sessionID string
		key       string
		value     string
		logsDir   string
	)

	cmd := &cobra.Command{
		Use:   "session-metadata",
		Short: "Record session metadata",
		Long:  "Writes a session_metadata event to pathflow-events.jsonl.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSessionMetadata(cmd.Context(), cmd.OutOrStdout(), logsDir, sessionID, key, value)
		},
	}

	cmd.Flags().StringVarP(&sessionID, "session-id", "s", "", "session ID (required)")
	cmd.Flags().StringVarP(&key, "key", "k", "", "metadata key (required)")
	cmd.Flags().StringVarP(&value, "value", "v", "", "metadata value (required)")
	cmd.Flags().StringVar(&logsDir, "logs-dir", defaultPathflowLogsDir(), "path to pathflow logs directory")
	_ = cmd.MarkFlagRequired("session-id")
	_ = cmd.MarkFlagRequired("key")
	_ = cmd.MarkFlagRequired("value")

	return cmd
}

func runSessionMetadata(ctx context.Context, w io.Writer, logsDir, sessionID, key, value string) error {
	tw, err := pathflow.NewTransitionWriter(ctx, logsDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: err}
	}

	params := pathflow.SessionMetadataParams{
		SessionID: sessionID,
		Key:       key,
		Value:     value,
	}
	if err := tw.RecordSessionMetadata(params); err != nil {
		return &exitError{code: ExitConfigError, err: err}
	}

	fmt.Fprintf(w, `{"status":"recorded","key":%q,"value":%q}`+"\n", key, value)
	return nil
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
