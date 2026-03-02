package main

import (
	"context"
	"errors"
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/ledger"
	"github.com/codeflow/codeflow-cli/internal/workgraph"
	"github.com/spf13/cobra"
)

// newWorkgraphCmd creates the top-level "workgraph" command group with all subcommands.
func newWorkgraphCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "workgraph",
		Short: "Epic and task lifecycle commands",
		Long:  "Create, update, and query epics and tasks in the CodeFlow work graph.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newWorkgraphCreateEpicCmd())
	cmd.AddCommand(newWorkgraphCreateTaskCmd())
	cmd.AddCommand(newWorkgraphUpdateEpicCmd())
	cmd.AddCommand(newWorkgraphUpdateTaskCmd())
	cmd.AddCommand(newWorkgraphQueryCmd())

	return cmd
}

// newWorkgraphCreateEpicCmd creates the "workgraph create-epic" subcommand.
func newWorkgraphCreateEpicCmd() *cobra.Command {
	var (
		dbPath    string
		ledgerDir string
		data      string
	)

	cmd := &cobra.Command{
		Use:   "create-epic",
		Short: "Create a new epic with dual IDs",
		Long:  "Generates ULID + format_id, inserts into the epics table, and appends an epic_created JSONL event.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runWorkgraphCreateEpic(cmd.Context(), cmd.OutOrStdout(), dbPath, ledgerDir, data)
		},
	}

	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "path to JSONL ledger directory")
	cmd.Flags().StringVar(&data, "data", "", "JSON input (reads stdin if empty)")

	return cmd
}

func runWorkgraphCreateEpic(ctx context.Context, w io.Writer, dbPath, ledgerDir, data string) error {
	svc, cleanup, err := newWorkgraphService(dbPath, ledgerDir)
	if err != nil {
		return err
	}
	defer cleanup()

	if err := svc.CreateEpic(ctx, data, w); err != nil {
		return classifyWorkgraphError(err)
	}
	return nil
}

// newWorkgraphCreateTaskCmd creates the "workgraph create-task" subcommand.
func newWorkgraphCreateTaskCmd() *cobra.Command {
	var (
		dbPath    string
		ledgerDir string
		data      string
	)

	cmd := &cobra.Command{
		Use:   "create-task",
		Short: "Create a new task with dual IDs under an epic",
		Long:  "Generates ULID + format_id, validates the parent epic, inserts into the tasks table, and appends a task_created JSONL event.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runWorkgraphCreateTask(cmd.Context(), cmd.OutOrStdout(), dbPath, ledgerDir, data)
		},
	}

	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "path to JSONL ledger directory")
	cmd.Flags().StringVar(&data, "data", "", "JSON input (reads stdin if empty)")

	return cmd
}

func runWorkgraphCreateTask(ctx context.Context, w io.Writer, dbPath, ledgerDir, data string) error {
	svc, cleanup, err := newWorkgraphService(dbPath, ledgerDir)
	if err != nil {
		return err
	}
	defer cleanup()

	if err := svc.CreateTask(ctx, data, w); err != nil {
		return classifyWorkgraphError(err)
	}
	return nil
}

// newWorkgraphUpdateEpicCmd creates the "workgraph update-epic" subcommand.
func newWorkgraphUpdateEpicCmd() *cobra.Command {
	var (
		dbPath    string
		ledgerDir string
		data      string
	)

	cmd := &cobra.Command{
		Use:   "update-epic",
		Short: "Update an existing epic",
		Long:  "Resolves by id or format_id, updates specified fields, and appends epic_status_changed JSONL event if status changed.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runWorkgraphUpdateEpic(cmd.Context(), cmd.OutOrStdout(), dbPath, ledgerDir, data)
		},
	}

	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "path to JSONL ledger directory")
	cmd.Flags().StringVar(&data, "data", "", "JSON input (reads stdin if empty)")

	return cmd
}

func runWorkgraphUpdateEpic(ctx context.Context, w io.Writer, dbPath, ledgerDir, data string) error {
	svc, cleanup, err := newWorkgraphService(dbPath, ledgerDir)
	if err != nil {
		return err
	}
	defer cleanup()

	if err := svc.UpdateEpic(ctx, data, w); err != nil {
		return classifyWorkgraphError(err)
	}
	return nil
}

// newWorkgraphUpdateTaskCmd creates the "workgraph update-task" subcommand.
func newWorkgraphUpdateTaskCmd() *cobra.Command {
	var (
		dbPath    string
		ledgerDir string
		data      string
	)

	cmd := &cobra.Command{
		Use:   "update-task",
		Short: "Update an existing task",
		Long:  "Resolves by id or format_id, updates specified fields, sets started_at/completed_at on status transitions, and appends task_status_changed JSONL event if status changed.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runWorkgraphUpdateTask(cmd.Context(), cmd.OutOrStdout(), dbPath, ledgerDir, data)
		},
	}

	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "path to JSONL ledger directory")
	cmd.Flags().StringVar(&data, "data", "", "JSON input (reads stdin if empty)")

	return cmd
}

func runWorkgraphUpdateTask(ctx context.Context, w io.Writer, dbPath, ledgerDir, data string) error {
	svc, cleanup, err := newWorkgraphService(dbPath, ledgerDir)
	if err != nil {
		return err
	}
	defer cleanup()

	if err := svc.UpdateTask(ctx, data, w); err != nil {
		return classifyWorkgraphError(err)
	}
	return nil
}

// newWorkgraphQueryCmd creates the "workgraph query" subcommand.
func newWorkgraphQueryCmd() *cobra.Command {
	var (
		dbPath   string
		mode     string
		id       string
		formatID string
		epicID   string
		area     string
		status   string
		domain   string
		workType string
		limit    int
	)

	cmd := &cobra.Command{
		Use:   "query",
		Short: "Query epics and tasks",
		Long:  "Query the work graph with modes: get-epic, get-task, list-epics, list-tasks.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			params := workgraph.QueryParams{
				Mode:     mode,
				ID:       id,
				FormatID: formatID,
				EpicID:   epicID,
				Area:     area,
				Status:   status,
				Domain:   domain,
				WorkType: workType,
				Limit:    limit,
			}
			return runWorkgraphQuery(cmd.Context(), cmd.OutOrStdout(), dbPath, params)
		},
	}

	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&mode, "mode", "", "query mode: get-epic, get-task, list-epics, list-tasks")
	cmd.Flags().StringVar(&id, "id", "", "ULID primary key")
	cmd.Flags().StringVar(&formatID, "format-id", "", "human-readable format ID")
	cmd.Flags().StringVar(&epicID, "epic-id", "", "filter tasks by epic ULID")
	cmd.Flags().StringVar(&area, "area", "", "filter by area_type")
	cmd.Flags().StringVar(&status, "status", "", "filter by status")
	cmd.Flags().StringVar(&domain, "domain", "", "filter by domain")
	cmd.Flags().StringVar(&workType, "work-type", "", "filter by work_type")
	cmd.Flags().IntVar(&limit, "limit", 50, "max results for list modes")

	_ = cmd.MarkFlagRequired("mode")

	return cmd
}

func runWorkgraphQuery(ctx context.Context, w io.Writer, dbPath string, params workgraph.QueryParams) error {
	d, err := openDB(dbPath)
	if err != nil {
		return &exitError{code: ExitConfigError, err: err}
	}
	defer closeDB(d)

	svc := &workgraph.Service{DB: d}

	if err := svc.Query(ctx, params, w); err != nil {
		return classifyWorkgraphError(err)
	}
	return nil
}

// newWorkgraphService creates a Service with DB and ledger writer, returning
// a cleanup function that closes the DB.
func newWorkgraphService(dbPath, ledgerDir string) (*workgraph.Service, func(), error) {
	d, err := openDB(dbPath)
	if err != nil {
		return nil, nil, &exitError{code: ExitConfigError, err: err}
	}

	writer, err := ledger.NewWriter(ledgerDir)
	if err != nil {
		closeDB(d)
		return nil, nil, &exitError{code: ExitRuntimeError, err: fmt.Errorf("creating ledger writer: %w", err)}
	}

	svc := workgraph.NewService(d, writer)
	cleanup := func() { closeDB(d) }
	return svc, cleanup, nil
}

// classifyWorkgraphError maps workgraph sentinel errors to exit codes.
func classifyWorkgraphError(err error) error {
	switch {
	case errors.Is(err, workgraph.ErrInvalidInput):
		return &exitError{code: ExitConfigError, err: err}
	case errors.Is(err, workgraph.ErrNotFound):
		return &exitError{code: ExitGeneralError, err: err}
	case errors.Is(err, workgraph.ErrConstraint):
		return &exitError{code: ExitConfigError, err: err}
	default:
		return &exitError{code: ExitRuntimeError, err: err}
	}
}
