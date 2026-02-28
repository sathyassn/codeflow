package main

import (
	"context"
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/idgen"
	"github.com/spf13/cobra"
)

// newInternalCmd creates the hidden "internal" command group for internal tooling.
// These commands are used by CodeFlow hooks and scripts, not end users.
func newInternalCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:    "internal",
		Short:  "Internal commands for CodeFlow infrastructure",
		Long:   "Internal commands used by hooks and scripts. Not intended for direct user invocation.",
		Hidden: true,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newInternalUlidCmd())

	return cmd
}

// newInternalUlidCmd creates the "internal ulid" subcommand.
func newInternalUlidCmd() *cobra.Command {
	var prefix string

	cmd := &cobra.Command{
		Use:   "ulid",
		Short: "Generate a ULID",
		Long:  "Generates a new ULID (Universally Unique Lexicographically Sortable Identifier) and prints it to stdout.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runInternalUlid(cmd.OutOrStdout(), prefix)
		},
	}

	cmd.Flags().StringVar(&prefix, "prefix", "", "prefix for the ULID (e.g., task, epic, ses)")

	return cmd
}

// runInternalUlid generates and prints a ULID.
func runInternalUlid(w io.Writer, prefix string) error {
	var id string
	if prefix != "" {
		id = idgen.NewPrefixedULID(prefix)
	} else {
		id = idgen.NewULID()
	}
	fmt.Fprintln(w, id)
	return nil
}

// newDBGenerateIdCmd creates the "db generate-id" subcommand.
func newDBGenerateIdCmd() *cobra.Command {
	var (
		dbPath string
		table  string
		area   string
	)

	cmd := &cobra.Command{
		Use:   "generate-id",
		Short: "Generate the next sequential format ID",
		Long:  "Queries the database for the highest existing format ID in the specified table and area, then returns the next sequential number.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runDBGenerateId(cmd.OutOrStdout(), dbPath, table, area)
		},
	}

	cmd.Flags().StringVar(&dbPath, "db", defaultDBPath(), "path to database file")
	cmd.Flags().StringVar(&table, "table", "", "table to query (tasks or epics)")
	cmd.Flags().StringVar(&area, "area", "", "area type (e.g., INF, CLI)")
	_ = cmd.MarkFlagRequired("table")
	_ = cmd.MarkFlagRequired("area")

	return cmd
}

// runDBGenerateId queries the DB for the next format ID sequence number and prints it.
func runDBGenerateId(w io.Writer, dbPath, table, area string) error {
	ctx := context.TODO()
	d, err := openDB(dbPath)
	if err != nil {
		return &exitError{code: ExitConfigError, err: err}
	}
	defer closeDB(d)

	// Adapt db.DB.QueryRow to the idgen.Querier interface.
	querier := idgen.QueryRowFunc(d.QueryRow)
	seq, err := idgen.NextFormatID(ctx, querier, table, area)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("generating format ID: %w", err)}
	}

	// Print the full format ID based on table type.
	switch table {
	case "epics":
		fmt.Fprintln(w, idgen.FormatEpicID(area, seq))
	case "tasks":
		// For tasks, we only output the sequence number since the caller
		// needs to combine it with the epic number to form the full ID.
		fmt.Fprintf(w, "%03d\n", seq)
	default:
		fmt.Fprintf(w, "%03d\n", seq)
	}

	return nil
}
