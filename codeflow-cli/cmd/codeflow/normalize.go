package main

import (
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/ledger"
	"github.com/spf13/cobra"
)

// newNormalizeCmd creates the top-level "normalize" command group.
func newNormalizeCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "normalize",
		Short: "Normalize CodeFlow data files",
		Long:  "Normalize CodeFlow data files to canonical schemas. Subcommands target specific data stores.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newNormalizeLedgerCmd())

	return cmd
}

// newNormalizeLedgerCmd creates the "normalize ledger" subcommand.
func newNormalizeLedgerCmd() *cobra.Command {
	var (
		dryRun    bool
		ledgerDir string
	)

	cmd := &cobra.Command{
		Use:   "ledger",
		Short: "Normalize JSONL ledger files to canonical schema",
		Long: `Normalizes all 4 canonical JSONL ledger files to consistent field names.

Normalization rules:
  - Rename "type"/"e" -> "event"
  - Rename "ts" -> "timestamp"
  - Rename "sid" -> "session_id", "wid" -> "work_id"
  - Rename "from_status" -> "old_status", "to_status" -> "new_status"
  - Map "op" + "table" -> canonical event name
  - Drop records with no resolvable event type

Idempotent: running twice produces identical output.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runNormalizeLedger(cmd.OutOrStdout(), ledgerDir, dryRun)
		},
	}

	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "report changes without modifying files")
	cmd.Flags().StringVar(&ledgerDir, "ledger-dir", defaultLedgerDir(), "path to JSONL ledger directory")

	return cmd
}

// runNormalizeLedger implements the normalize ledger logic.
func runNormalizeLedger(w io.Writer, ledgerDir string, dryRun bool) error {
	config := ledger.NormalizeConfig{
		DryRun:    dryRun,
		LedgerDir: ledgerDir,
	}

	results, err := ledger.NormalizeAll(ledgerDir, config)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("normalization failed: %w", err)}
	}

	if len(results) == 0 {
		fmt.Fprintln(w, "No canonical JSONL files found in", ledgerDir)
		return nil
	}

	ledger.PrintResults(w, results, dryRun)
	return nil
}
