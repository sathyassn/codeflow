package main

import (
	"context"
	"fmt"
	"io"
	"path/filepath"

	"github.com/codeflow/codeflow-cli/internal/session"
	"github.com/spf13/cobra"
)

// newSessionCmd creates the top-level "session" command with start/end subcommands.
func newSessionCmd() *cobra.Command {
	sessionCmd := &cobra.Command{
		Use:   "session",
		Short: "Session lifecycle management",
		Long:  "Manage CodeFlow sessions: start a new session with ULID-based ID, end the current session, or query the active session.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	sessionCmd.AddCommand(newSessionStartCmd())
	sessionCmd.AddCommand(newSessionEndCmd())
	sessionCmd.AddCommand(newSessionCurrentCmd())

	return sessionCmd
}

// newSessionStartCmd creates the "session start" subcommand.
func newSessionStartCmd() *cobra.Command {
	var (
		claudeID   string
		dbPath     string
		ledgerDir  string
		runtimeDir string
	)

	cmd := &cobra.Command{
		Use:   "start",
		Short: "Start a new session",
		Long:  "Creates a new session with a ULID-based ID, inserts a DB record, writes a JSONL event, and sets the current session ID.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSessionStart(cmd.OutOrStdout(), dbPath, claudeID, ledgerDir, runtimeDir)
		},
	}

	cmd.Flags().StringVar(&claudeID, "claude-id", "", "Claude agent ID for the session (required)")
	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "path to JSONL ledger directory")
	cmd.Flags().StringVar(&runtimeDir, "runtime", defaultRuntimeDir(), "path to runtime state directory")
	_ = cmd.MarkFlagRequired("claude-id")

	return cmd
}

// runSessionStart implements the session start logic.
func runSessionStart(w io.Writer, dbPath, claudeID, ledgerDir, runtimeDir string) error {
	ctx := contextFromWriter(w)
	d, err := openDB(dbPath)
	if err != nil {
		return err
	}
	defer closeDB(d)

	// Ensure schema is initialized and migrations are applied.
	if err := d.InitFromSchema(ctx); err != nil {
		return fmt.Errorf("initializing schema: %w", err)
	}
	if _, err := d.Migrate(ctx); err != nil {
		return fmt.Errorf("applying migrations: %w", err)
	}

	sessionID, err := session.Start(ctx, d, claudeID, ledgerDir, runtimeDir)
	if err != nil {
		return fmt.Errorf("starting session: %w", err)
	}

	// Write codeflow-env.sh for the CLI path. The hook path (start.go)
	// handles this separately via writeEnvFile().
	// Derive projectDir from the runtimeDir (runtimeDir is typically .state/runtime).
	projectDir := filepath.Dir(filepath.Dir(runtimeDir))
	if err := session.WriteEnvFile(runtimeDir, sessionID, projectDir); err != nil {
		return fmt.Errorf("writing codeflow-env.sh: %w", err)
	}

	fmt.Fprintf(w, "%s\n", sessionID)
	return nil
}

// newSessionEndCmd creates the "session end" subcommand.
func newSessionEndCmd() *cobra.Command {
	var (
		dbPath     string
		ledgerDir  string
		runtimeDir string
	)

	cmd := &cobra.Command{
		Use:   "end",
		Short: "End the current session",
		Long:  "Ends the active session, updates the DB record with duration, writes a JSONL event, and cleans up the session ID file.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSessionEnd(cmd.OutOrStdout(), dbPath, ledgerDir, runtimeDir)
		},
	}

	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "path to JSONL ledger directory")
	cmd.Flags().StringVar(&runtimeDir, "runtime", defaultRuntimeDir(), "path to runtime state directory")

	return cmd
}

// runSessionEnd implements the session end logic.
func runSessionEnd(w io.Writer, dbPath, ledgerDir, runtimeDir string) error {
	ctx := contextFromWriter(w)
	d, err := openDB(dbPath)
	if err != nil {
		return err
	}
	defer closeDB(d)

	if err := session.End(ctx, d, ledgerDir, runtimeDir); err != nil {
		return fmt.Errorf("ending session: %w", err)
	}

	fmt.Fprintln(w, "Session ended")
	return nil
}

// newSessionCurrentCmd creates the "session current" subcommand.
func newSessionCurrentCmd() *cobra.Command {
	var runtimeDir string

	cmd := &cobra.Command{
		Use:   "current",
		Short: "Show the current session ID",
		Long:  "Reads and displays the current active session ID from the runtime state file.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSessionCurrent(cmd.OutOrStdout(), runtimeDir)
		},
	}

	cmd.Flags().StringVar(&runtimeDir, "runtime", defaultRuntimeDir(), "path to runtime state directory")

	return cmd
}

// runSessionCurrent implements the session current logic.
func runSessionCurrent(w io.Writer, runtimeDir string) error {
	sessionID, err := session.Current(runtimeDir)
	if err != nil {
		return fmt.Errorf("reading current session: %w", err)
	}

	fmt.Fprintln(w, sessionID)
	return nil
}

// defaultRuntimeDir returns the default runtime state directory.
func defaultRuntimeDir() string {
	return session.DefaultRuntimeDir
}

// contextFromWriter returns a background context. The writer parameter is
// accepted but unused to maintain a consistent function signature.
func contextFromWriter(_ io.Writer) context.Context {
	return context.Background()
}
