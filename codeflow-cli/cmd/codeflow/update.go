package main

import (
	"fmt"
	"net/http"

	"github.com/codeflow/codeflow-cli/internal/update"
	"github.com/spf13/cobra"
)

// newUpdateCmd creates the "update" command for CLI self-update.
func newUpdateCmd() *cobra.Command {
	var (
		checkOnly bool
		force     bool
	)

	cmd := &cobra.Command{
		Use:   "update",
		Short: "Update codeflow to the latest version",
		Long:  "Check for newer versions and update the codeflow binary. Also syncs .codeflow/ template files.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runUpdate(cmd, checkOnly, force)
		},
	}

	cmd.Flags().BoolVar(&checkOnly, "check", false, "only check for updates without applying")
	cmd.Flags().BoolVar(&force, "force", false, "bypass version compatibility check (allows downgrades)")

	return cmd
}

// runUpdate implements the update command logic.
func runUpdate(cmd *cobra.Command, checkOnly, force bool) error {
	w := cmd.OutOrStdout()

	opts := &update.Options{
		BinaryPath:     defaultBinaryPath(),
		ConfigDir:      defaultConfigDir(),
		CurrentVersion: version,
		HTTPGet:        http.Get,
		CheckOnly:      checkOnly,
		Force:          force,
	}

	// Check for updates.
	fmt.Fprintln(w, "Checking for updates...")
	info, err := update.Check(opts)
	if err != nil {
		return &exitError{code: ExitExternalError, err: fmt.Errorf("checking for updates: %w", err)}
	}

	if !info.UpdateAvailable && !force {
		fmt.Fprintf(w, "Already at latest version (%s)\n", info.Current)
		return nil
	}

	if info.UpdateAvailable {
		fmt.Fprintf(w, "Update available: %s -> %s\n", info.Current, info.Latest)
	} else {
		fmt.Fprintf(w, "Current version: %s (force mode)\n", info.Current)
	}

	if checkOnly {
		return nil
	}

	// Apply update.
	fmt.Fprintln(w, "Downloading update...")
	if err := update.Apply(opts, info); err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("applying update: %w", err)}
	}
	fmt.Fprintf(w, "Updated to %s\n", info.Latest)

	// Sync templates.
	fmt.Fprintln(w, "Syncing templates...")
	updated, err := update.SyncTemplates(opts, "")
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("syncing templates: %w", err)}
	}

	if len(updated) > 0 {
		fmt.Fprintf(w, "Updated %d template(s)\n", len(updated))
		for _, f := range updated {
			fmt.Fprintf(w, "  - %s\n", f)
		}
	} else {
		fmt.Fprintln(w, "Templates up to date")
	}

	return nil
}
