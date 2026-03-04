// Package autorun provides the autorun CLI subcommands for batch execution.
package autorun

import (
	"context"
	"fmt"
	"io"
	"path/filepath"

	"github.com/codeflow/codeflow-cli/internal/autorun"
	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/spf13/cobra"
)

// defaultDBPath returns the default database path relative to the repo root.
func defaultDBPath() string {
	return filepath.Join(".state", "db", "codeflow.db")
}

// NewCmd creates the top-level "autorun" command with start, status, and sessions subcommands.
func NewCmd() *cobra.Command {
	autorunCmd := &cobra.Command{
		Use:   "autorun",
		Short: "Automated batch session execution",
		Long:  "Run, monitor, and manage automated batch sessions that execute multiple tasks in parallel using Claude Code workers.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	autorunCmd.AddCommand(newStartCmd())
	autorunCmd.AddCommand(newStatusCmd())
	autorunCmd.AddCommand(newSessionsCmd())

	return autorunCmd
}

// newStartCmd creates the "autorun start" subcommand.
func newStartCmd() *cobra.Command {
	var (
		dbPath     string
		maxWorkers int
	)

	cmd := &cobra.Command{
		Use:   "start <batch-file>",
		Short: "Start autorun execution from a batch file",
		Long:  "Parses a YAML batch file and starts parallel execution of tasks using Claude Code workers. Returns the autorun session ID.",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return runStart(cmd.OutOrStdout(), dbPath, args[0], maxWorkers)
		},
	}

	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().IntVar(&maxWorkers, "max-workers", 0, "maximum concurrent workers (overrides batch file; 0 = use batch file value)")

	return cmd
}

// runStart implements the autorun start logic.
func runStart(w io.Writer, dbPath, batchPath string, maxWorkersOverride int) error {
	batch, err := autorun.ParseBatchFile(batchPath)
	if err != nil {
		return fmt.Errorf("parsing batch file: %w", err)
	}

	// Apply max-workers override if specified.
	if maxWorkersOverride > 0 {
		batch.MaxWorkers = maxWorkersOverride
	}

	d, err := db.NewDB(dbPath)
	if err != nil {
		return fmt.Errorf("opening database: %w", err)
	}
	// Note: d.Close() is intentionally not deferred here. Start() launches a
	// background goroutine that continues using the DB after this function
	// returns. The DB connection is owned by the process lifetime.

	ctx := context.Background()

	// Ensure schema exists and migrations are applied.
	if err := d.InitFromSchema(ctx); err != nil {
		return fmt.Errorf("initializing schema: %w", err)
	}
	if _, err := d.Migrate(ctx); err != nil {
		return fmt.Errorf("applying migrations: %w", err)
	}

	// Create the runner with real tmux and Claude implementations.
	runner := autorun.NewTmuxWorker(&autorun.RealTmuxRunner{}, &autorun.RealClaudeInvoker{})
	orch := autorun.NewOrchestrator(d, runner)

	sessionID, err := orch.Start(ctx, batch)
	if err != nil {
		return fmt.Errorf("starting autorun: %w", err)
	}

	fmt.Fprintf(w, "Autorun session started: %s\n", sessionID)
	fmt.Fprintf(w, "  Batch: %s (%s)\n", batch.Name, batchPath)
	fmt.Fprintf(w, "  Tasks: %d\n", len(batch.Tasks))
	fmt.Fprintf(w, "  Max workers: %d\n", batch.MaxWorkers)

	return nil
}
