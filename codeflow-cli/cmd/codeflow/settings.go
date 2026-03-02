package main

import (
	"fmt"
	"os"

	"github.com/codeflow/codeflow-cli/internal/settings"
	"github.com/spf13/cobra"
)

// newSettingsCmd creates the "settings" command group.
func newSettingsCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "settings",
		Short: "Settings management subcommands",
		Long:  "Manage CodeFlow settings: setup managed settings, validate templates.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newSetupManagedCmd())
	cmd.AddCommand(newSettingsValidateCmd())
	return cmd
}

// newSetupManagedCmd creates the "setup-managed" subcommand that installs
// enterprise managed settings.
func newSetupManagedCmd() *cobra.Command {
	var dryRun bool

	cmd := &cobra.Command{
		Use:   "setup-managed",
		Short: "Install enterprise managed settings",
		Long: `Install or update the system-level managed-settings.json file.

Creates /Library/Application Support/ClaudeCode/managed-settings.json (macOS)
or /etc/claude-code/managed-settings.json (Linux) with sudo escalation.

These settings cannot be overridden by project-level settings and apply
to ALL Claude Code usage on this machine.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSetupManaged(cmd, detectProjectDir(), dryRun)
		},
	}

	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "show what would be done without making changes")
	return cmd
}

// runSetupManaged implements the setup-managed logic.
func runSetupManaged(cmd *cobra.Command, projectDir string, dryRun bool) error {
	sourceFile := settings.SourceFilePath(projectDir)

	result, err := settings.SetupManagedSettings(sourceFile, dryRun)
	if err != nil {
		return fmt.Errorf("setup managed settings: %w", err)
	}

	fmt.Fprint(cmd.OutOrStdout(), settings.FormatSetupOutput(result))
	return nil
}

// newSettingsValidateCmd creates the "validate" subcommand that runs
// standalone settings template validation.
func newSettingsValidateCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "validate",
		Short: "Validate settings template consistency",
		Long: `Run all settings template validation checks.

Validates:
  1. Hooks section SHA256 consistency across all 4 templates
  2. _version consistency across all templates
  3. Hook wiring audit (orphaned/broken scripts)
  4. Full file SHA256 of settings.json vs template source
  5. Full file SHA256 of settings.local.json vs template source

Exits non-zero if any check fails.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSettingsValidate(cmd, detectProjectDir())
		},
	}
}

// runSettingsValidate implements the standalone validation logic.
func runSettingsValidate(cmd *cobra.Command, projectDir string) error {

	results, err := settings.ValidateSettingsTemplates(projectDir)
	if err != nil {
		return fmt.Errorf("settings validation: %w", err)
	}

	fmt.Fprint(cmd.OutOrStdout(), settings.FormatResults(results))

	if settings.HasFailures(results) {
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("settings validation: %d check(s) failed", countFailures(results))}
	}

	return nil
}

func countFailures(results []settings.ValidationResult) int {
	n := 0
	for _, r := range results {
		if !r.Passed {
			n++
		}
	}
	return n
}

// runSettingsValidateHook implements the PostToolUse hook entry point.
// It filters for Edit/Write on settings-templates/*.json, always exits 0,
// and outputs hookSpecificOutput JSON.
func runSettingsValidateHook(cmd *cobra.Command, projectDirFn func() string) error {
	// Read stdin to check if this is a relevant tool call.
	data, err := readStdinBytes(cmd)
	if err != nil {
		return nil
	}

	// Filter: only process Edit/Write to settings-templates/*.json.
	if !isSettingsTemplateEdit(data) {
		return nil
	}

	projectDir := projectDirFn()
	results, err := settings.ValidateSettingsTemplates(projectDir)
	if err != nil {
		// Advisory hook - always exit 0.
		fmt.Fprintf(os.Stderr, "settings-validate hook: %v\n", err)
		return nil
	}

	output, err := settings.FormatHookOutput(results)
	if err != nil {
		fmt.Fprintf(os.Stderr, "settings-validate hook format error: %v\n", err)
		return nil
	}

	fmt.Fprintln(cmd.OutOrStdout(), output)
	return nil
}
