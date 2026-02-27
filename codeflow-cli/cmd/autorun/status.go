package autorun

import (
	"context"
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/autorun"
	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/spf13/cobra"
)

// newStatusCmd creates the "autorun status" subcommand.
func newStatusCmd() *cobra.Command {
	var dbPath string

	cmd := &cobra.Command{
		Use:   "status [session-id]",
		Short: "Show current autorun session status",
		Long:  "Displays the status of an autorun session including worker states and task progress. If no session ID is given, shows the most recent session.",
		Args:  cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			var sessionID string
			if len(args) > 0 {
				sessionID = args[0]
			}
			return runStatus(cmd.OutOrStdout(), dbPath, sessionID)
		},
	}

	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")

	return cmd
}

// runStatus implements the autorun status logic.
func runStatus(w io.Writer, dbPath, sessionID string) error {
	ctx := context.Background()

	d, err := db.NewDB(dbPath)
	if err != nil {
		return fmt.Errorf("opening database: %w", err)
	}
	defer d.Close()

	var session *db.AutorunSession
	if sessionID != "" {
		session, err = autorun.GetSessionStatus(ctx, d, sessionID)
	} else {
		session, err = autorun.GetLatestSession(ctx, d)
	}
	if err != nil {
		return fmt.Errorf("querying session: %w", err)
	}

	// Print session summary.
	fmt.Fprintf(w, "Session: %s\n", session.ID)
	fmt.Fprintf(w, "  Batch: %s", session.BatchFile)
	if session.BatchName.Valid {
		fmt.Fprintf(w, " (%s)", session.BatchName.String)
	}
	fmt.Fprintln(w)
	fmt.Fprintf(w, "  Status: %s\n", session.Status)
	fmt.Fprintf(w, "  Workers: %d max\n", session.MaxSessionWorkers)
	fmt.Fprintf(w, "  Progress: %d/%d completed, %d failed\n",
		session.CompletedTasks, session.TotalTasks, session.FailedTasks)
	fmt.Fprintf(w, "  Created: %s\n", session.CreatedAt)
	if session.CompletedAt.Valid {
		fmt.Fprintf(w, "  Completed: %s\n", session.CompletedAt.String)
	}

	// Print worker details.
	workers, err := autorun.GetSessionWorkers(ctx, d, session.ID)
	if err != nil {
		return fmt.Errorf("querying workers: %w", err)
	}

	if len(workers) > 0 {
		fmt.Fprintf(w, "\nWorkers:\n")
		for _, worker := range workers {
			fmt.Fprintf(w, "  #%d [%s] task=%s", worker.WorkerNum, worker.Status, worker.TaskID)
			if worker.TmuxSession.Valid {
				fmt.Fprintf(w, " tmux=%s", worker.TmuxSession.String)
			}
			if worker.PRNumber.Valid {
				fmt.Fprintf(w, " PR=#%d", worker.PRNumber.Int64)
			}
			fmt.Fprintln(w)
		}
	}

	return nil
}
