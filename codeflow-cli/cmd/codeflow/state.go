package main

import (
	"encoding/json"
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/ledger"
	"github.com/codeflow/codeflow-cli/internal/workstate"
	"github.com/spf13/cobra"
)

// newStateCmd creates the top-level "state" command with active-task and memory subgroups.
func newStateCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "state",
		Short: "Active task state and memory operations",
		Long:  "Manage the active task bridge file and record memory events to the JSONL ledger.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newStateActiveTaskCmd())
	cmd.AddCommand(newStateMemoryCmd())

	return cmd
}

// newStateActiveTaskCmd creates the "state active-task" subgroup.
func newStateActiveTaskCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "active-task",
		Short: "Active task bridge file operations",
		Long:  "Set, get, or clear the active task bridge file used by hooks to identify the current task context.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newStateActiveTaskSetCmd())
	cmd.AddCommand(newStateActiveTaskGetCmd())
	cmd.AddCommand(newStateActiveTaskClearCmd())

	return cmd
}

// newStateActiveTaskSetCmd creates "state active-task set".
func newStateActiveTaskSetCmd() *cobra.Command {
	var (
		runtimeDir   string
		taskID       string
		formatID     string
		epicID       string
		epicFormatID string
		title        string
		branch       string
		sessionID    string
		status       string
		stage        string
		teamName     string
	)

	cmd := &cobra.Command{
		Use:   "set",
		Short: "Write the active task bridge file",
		Long:  "Writes active-task.json with the specified task fields. Atomic write via temp file + rename.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			task := workstate.ActiveTask{
				TaskID:       taskID,
				FormatID:     formatID,
				EpicID:       epicID,
				EpicFormatID: epicFormatID,
				Title:        title,
				Status:       status,
				Branch:       branch,
				SessionID:    sessionID,
				CurrentStage: stage,
				TeamName:     teamName,
			}
			return runStateActiveTaskSet(cmd.OutOrStdout(), runtimeDir, task)
		},
	}

	cmd.Flags().StringVar(&runtimeDir, "runtime", defaultRuntimeDir(), "path to runtime state directory")
	cmd.Flags().StringVar(&taskID, "task-id", "", "task ID (required)")
	cmd.Flags().StringVar(&formatID, "format-id", "", "task format ID (required)")
	cmd.Flags().StringVar(&epicID, "epic-id", "", "epic ID (ULID)")
	cmd.Flags().StringVar(&epicFormatID, "epic", "", "epic format ID")
	cmd.Flags().StringVar(&title, "title", "", "task title")
	cmd.Flags().StringVar(&branch, "branch", "", "git branch")
	cmd.Flags().StringVar(&sessionID, "session", "", "session ID")
	cmd.Flags().StringVar(&status, "status", "", "task status")
	cmd.Flags().StringVar(&stage, "stage", "", "current work stage")
	cmd.Flags().StringVar(&teamName, "team", "", "team name")
	_ = cmd.MarkFlagRequired("task-id")
	_ = cmd.MarkFlagRequired("format-id")

	return cmd
}

// runStateActiveTaskSet implements the active-task set logic.
func runStateActiveTaskSet(w io.Writer, runtimeDir string, task workstate.ActiveTask) error {
	if err := workstate.SetActiveTask(runtimeDir, task); err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("setting active task: %w", err)}
	}

	fmt.Fprintln(w, "Active task set")
	return nil
}

// newStateActiveTaskGetCmd creates "state active-task get".
func newStateActiveTaskGetCmd() *cobra.Command {
	var runtimeDir string

	cmd := &cobra.Command{
		Use:   "get",
		Short: "Output the current active task JSON",
		Long:  "Reads active-task.json and outputs its contents to stdout.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runStateActiveTaskGet(cmd.OutOrStdout(), runtimeDir)
		},
	}

	cmd.Flags().StringVar(&runtimeDir, "runtime", defaultRuntimeDir(), "path to runtime state directory")

	return cmd
}

// runStateActiveTaskGet implements the active-task get logic.
func runStateActiveTaskGet(w io.Writer, runtimeDir string) error {
	task, err := workstate.GetActiveTask(runtimeDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("getting active task: %w", err)}
	}

	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	if err := enc.Encode(task); err != nil {
		return &exitError{code: ExitInternalError, err: fmt.Errorf("encoding active task: %w", err)}
	}

	return nil
}

// newStateActiveTaskClearCmd creates "state active-task clear".
func newStateActiveTaskClearCmd() *cobra.Command {
	var runtimeDir string

	cmd := &cobra.Command{
		Use:   "clear",
		Short: "Remove the active task bridge file",
		Long:  "Removes active-task.json. Clearing a non-existent file is a no-op.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runStateActiveTaskClear(cmd.OutOrStdout(), runtimeDir)
		},
	}

	cmd.Flags().StringVar(&runtimeDir, "runtime", defaultRuntimeDir(), "path to runtime state directory")

	return cmd
}

// runStateActiveTaskClear implements the active-task clear logic.
func runStateActiveTaskClear(w io.Writer, runtimeDir string) error {
	if err := workstate.ClearActiveTask(runtimeDir); err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("clearing active task: %w", err)}
	}

	fmt.Fprintln(w, "Active task cleared")
	return nil
}

// newStateMemoryCmd creates the "state memory" subgroup.
func newStateMemoryCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "memory",
		Short: "Memory event operations",
		Long:  "Record memory events to the JSONL ledger for cross-session context persistence.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newStateMemoryRecordCmd())

	return cmd
}

// newStateMemoryRecordCmd creates "state memory record".
func newStateMemoryRecordCmd() *cobra.Command {
	var (
		ledgerDir string
		eventJSON string
	)

	cmd := &cobra.Command{
		Use:   "record",
		Short: "Record a memory event to the ledger",
		Long:  "Parses event JSON and appends it to memory-events.jsonl via the ledger writer.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runStateMemoryRecord(cmd.OutOrStdout(), ledgerDir, eventJSON)
		},
	}

	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "path to JSONL ledger directory")
	cmd.Flags().StringVar(&eventJSON, "event", "", "event as JSON string (required)")
	_ = cmd.MarkFlagRequired("event")

	return cmd
}

// runStateMemoryRecord implements the memory record logic.
func runStateMemoryRecord(w io.Writer, ledgerDir, eventJSON string) error {
	var event ledger.Event
	if err := json.Unmarshal([]byte(eventJSON), &event); err != nil {
		return &exitError{code: ExitConfigError, err: fmt.Errorf("parsing event JSON: %w", err)}
	}

	writer, err := ledger.NewWriter(ledgerDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("creating ledger writer: %w", err)}
	}

	if err := workstate.RecordMemoryEvent(writer, event); err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("recording memory event: %w", err)}
	}

	fmt.Fprintln(w, "Memory event recorded")
	return nil
}
