package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/codeflow/codeflow-cli/internal/sentinel"
	"github.com/spf13/cobra"
)

// newSentinelCmd creates the "sentinel" command group for PathFlow sentinel CRUD.
func newSentinelCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "sentinel",
		Short: "Manage PathFlow sentinel files",
		Long: `Create, check, list, and delete PathFlow sentinel files.

Sentinels are empty marker files that track PathFlow phase and stage
progression. They are stored in .state/sentinels/pathflow/{session-id}/
and named "pathflow-{name}" (e.g., "pathflow-pf-3", "pathflow-ws-dev").`,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newSentinelCreateCmd())
	cmd.AddCommand(newSentinelCheckCmd())
	cmd.AddCommand(newSentinelListCmd())
	cmd.AddCommand(newSentinelDeleteCmd())
	return cmd
}

// resolveSession returns the session ID from the flag or the
// CODEFLOW_SESSION_ID environment variable.
func resolveSession(cmd *cobra.Command) (string, error) {
	session, _ := cmd.Flags().GetString("session")
	if session == "" {
		session = os.Getenv("CODEFLOW_SESSION_ID")
	}
	if session == "" {
		return "", fmt.Errorf("session ID required: use --session or set CODEFLOW_SESSION_ID")
	}
	return session, nil
}

// resolveScope validates and returns the scope flag value.
func resolveScope(cmd *cobra.Command) (sentinel.Scope, error) {
	scope, _ := cmd.Flags().GetString("scope")
	if scope != "pathflow" {
		return "", fmt.Errorf("invalid scope %q: only \"pathflow\" is supported", scope)
	}
	return sentinel.ScopePathFlow, nil
}

// addSentinelFlags adds --scope, --session, and --name flags to a command.
func addSentinelFlags(cmd *cobra.Command, needName bool) {
	cmd.Flags().String("scope", "pathflow", "sentinel scope (pathflow)")
	cmd.Flags().String("session", "", "session ID (defaults to CODEFLOW_SESSION_ID)")
	if needName {
		cmd.Flags().String("name", "", "sentinel name (e.g., pf-3, ws-dev)")
	}
}

// newSentinelCreateCmd creates the "sentinel create" subcommand.
func newSentinelCreateCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "create",
		Short: "Create a sentinel file",
		Long: `Create a PathFlow sentinel file.

Creates an empty marker file named "pathflow-{name}" in the sentinel
directory for the given session. Idempotent: creating an existing
sentinel is a no-op.

Example:
  codeflow sentinel create --scope pathflow --name pf-3 --session ses-abc123`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSentinelCreate(cmd)
		},
	}
	addSentinelFlags(cmd, true)
	return cmd
}

func runSentinelCreate(cmd *cobra.Command) error {
	scope, err := resolveScope(cmd)
	if err != nil {
		return err
	}
	session, err := resolveSession(cmd)
	if err != nil {
		return err
	}
	name, _ := cmd.Flags().GetString("name")
	if name == "" {
		return fmt.Errorf("--name is required")
	}

	mgr := &sentinel.Manager{BaseDir: detectProjectDir()}
	if err := mgr.Create(scope, session, name); err != nil {
		return err
	}
	fmt.Fprintf(cmd.OutOrStdout(), "Created sentinel: %s\n", name)
	return nil
}

// newSentinelCheckCmd creates the "sentinel check" subcommand.
func newSentinelCheckCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "check",
		Short: "Check if a sentinel exists",
		Long: `Check if a PathFlow sentinel file exists.

Exits 0 if the sentinel exists, 1 if it does not.

Example:
  codeflow sentinel check --scope pathflow --name pf-3 --session ses-abc123`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSentinelCheck(cmd)
		},
	}
	addSentinelFlags(cmd, true)
	return cmd
}

func runSentinelCheck(cmd *cobra.Command) error {
	scope, err := resolveScope(cmd)
	if err != nil {
		return err
	}
	session, err := resolveSession(cmd)
	if err != nil {
		return err
	}
	name, _ := cmd.Flags().GetString("name")
	if name == "" {
		return fmt.Errorf("--name is required")
	}

	mgr := &sentinel.Manager{BaseDir: detectProjectDir()}
	exists, err := mgr.Check(scope, session, name)
	if err != nil {
		return err
	}
	if !exists {
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("sentinel %q not found", name)}
	}
	fmt.Fprintf(cmd.OutOrStdout(), "Sentinel exists: %s\n", name)
	return nil
}

// newSentinelListCmd creates the "sentinel list" subcommand.
func newSentinelListCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "list",
		Short: "List all sentinels in scope",
		Long: `List all PathFlow sentinel files for a session.

Prints one sentinel name per line, sorted alphabetically.

Example:
  codeflow sentinel list --scope pathflow --session ses-abc123`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSentinelList(cmd)
		},
	}
	addSentinelFlags(cmd, false)
	return cmd
}

func runSentinelList(cmd *cobra.Command) error {
	scope, err := resolveScope(cmd)
	if err != nil {
		return err
	}
	session, err := resolveSession(cmd)
	if err != nil {
		return err
	}

	mgr := &sentinel.Manager{BaseDir: detectProjectDir()}
	names, err := mgr.List(scope, session)
	if err != nil {
		return err
	}
	if len(names) == 0 {
		fmt.Fprintln(cmd.OutOrStdout(), "(none)")
		return nil
	}
	fmt.Fprintln(cmd.OutOrStdout(), strings.Join(names, "\n"))
	return nil
}

// newSentinelDeleteCmd creates the "sentinel delete" subcommand.
func newSentinelDeleteCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "delete",
		Short: "Delete a sentinel file",
		Long: `Delete a PathFlow sentinel file.

Removes the sentinel file. Idempotent: deleting a non-existent
sentinel is a no-op.

Example:
  codeflow sentinel delete --scope pathflow --name pf-3 --session ses-abc123`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSentinelDelete(cmd)
		},
	}
	addSentinelFlags(cmd, true)
	return cmd
}

func runSentinelDelete(cmd *cobra.Command) error {
	scope, err := resolveScope(cmd)
	if err != nil {
		return err
	}
	session, err := resolveSession(cmd)
	if err != nil {
		return err
	}
	name, _ := cmd.Flags().GetString("name")
	if name == "" {
		return fmt.Errorf("--name is required")
	}

	mgr := &sentinel.Manager{BaseDir: detectProjectDir()}
	if err := mgr.Delete(scope, session, name); err != nil {
		return err
	}
	fmt.Fprintf(cmd.OutOrStdout(), "Deleted sentinel: %s\n", name)
	return nil
}
