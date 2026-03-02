package main

import (
	"fmt"
	"io"
	"path/filepath"

	"github.com/codeflow/codeflow-cli/internal/report"
	"github.com/spf13/cobra"
)

// newReportCmd creates the top-level "report" command with subcommands.
func newReportCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "report",
		Short: "Generate progress tracking reports",
		Long:  "Generate markdown reports from project data, including epic progress tracking.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newReportEpicTrackerCmd())

	return cmd
}

// newReportEpicTrackerCmd creates the "report epic-tracker" subcommand.
func newReportEpicTrackerCmd() *cobra.Command {
	var (
		epicID string
		write  bool
	)

	cmd := &cobra.Command{
		Use:   "epic-tracker",
		Short: "Generate epic tracking report",
		Long:  "Scan project-management/epics/ filesystem, parse YAML frontmatter, and produce a progress tracking markdown report.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runReportEpicTracker(cmd.OutOrStdout(), epicID, write)
		},
	}

	cmd.Flags().StringVar(&epicID, "epic", "", "generate report for a specific epic ID")
	cmd.Flags().BoolVar(&write, "write", false, "write report to tracking/epic-tracker.md")

	return cmd
}

// runReportEpicTracker implements the epic tracker report logic.
func runReportEpicTracker(w io.Writer, epicID string, write bool) error {
	projectDir := detectProjectDir()
	epicsDir := filepath.Join(projectDir, "project-management", "epics")

	tracker := &report.EpicTracker{
		EpicsDir: epicsDir,
	}

	if epicID != "" {
		content, err := tracker.Generate(epicID)
		if err != nil {
			return fmt.Errorf("generating epic report: %w", err)
		}
		fmt.Fprint(w, content)
		return nil
	}

	if write {
		outputPath, err := tracker.WriteReport()
		if err != nil {
			return fmt.Errorf("writing report: %w", err)
		}
		fmt.Fprintf(w, "Generated: %s\n", outputPath)
		return nil
	}

	content, err := tracker.GenerateAll()
	if err != nil {
		return fmt.Errorf("generating report: %w", err)
	}
	fmt.Fprint(w, content)
	return nil
}
