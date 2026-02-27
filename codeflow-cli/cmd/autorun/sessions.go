package autorun

import (
	"context"
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/autorun"
	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/spf13/cobra"
)

// newSessionsCmd creates the "autorun sessions" subcommand.
func newSessionsCmd() *cobra.Command {
	var (
		dbPath string
		limit  int
	)

	cmd := &cobra.Command{
		Use:   "sessions",
		Short: "Show history of autorun sessions",
		Long:  "Lists all autorun sessions with their status, task counts, and timestamps. Newest sessions first.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSessions(cmd.OutOrStdout(), dbPath, limit)
		},
	}

	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().IntVar(&limit, "limit", 20, "maximum number of sessions to display")

	return cmd
}

// runSessions implements the autorun sessions logic.
func runSessions(w io.Writer, dbPath string, limit int) error {
	ctx := context.Background()

	d, err := db.NewDB(dbPath)
	if err != nil {
		return fmt.Errorf("opening database: %w", err)
	}
	defer d.Close()

	sessions, err := autorun.ListSessions(ctx, d, limit)
	if err != nil {
		return fmt.Errorf("listing sessions: %w", err)
	}

	if len(sessions) == 0 {
		fmt.Fprintln(w, "No autorun sessions found.")
		return nil
	}

	fmt.Fprintf(w, "%-30s %-12s %-20s %s/%s/%s\n", "SESSION", "STATUS", "BATCH", "DONE", "FAIL", "TOTAL")
	fmt.Fprintf(w, "%-30s %-12s %-20s %s\n", "-------", "------", "-----", "----/----/-----")

	for _, s := range sessions {
		batchName := s.BatchFile
		if s.BatchName.Valid && s.BatchName.String != "" {
			batchName = s.BatchName.String
		}
		// Truncate long batch names.
		if len(batchName) > 20 {
			batchName = batchName[:17] + "..."
		}

		fmt.Fprintf(w, "%-30s %-12s %-20s %4d/%4d/%5d\n",
			s.ID, s.Status, batchName,
			s.CompletedTasks, s.FailedTasks, s.TotalTasks)
	}

	return nil
}
