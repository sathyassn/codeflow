package main

import (
	"context"
	"errors"
	"fmt"
	"os/exec"

	"github.com/codeflow/codeflow-cli/internal/initialize"
	"github.com/spf13/cobra"
)

// newInitCmd creates the "init" command for project initialization.
func newInitCmd() *cobra.Command {
	var dir string

	cmd := &cobra.Command{
		Use:   "init",
		Short: "Initialize a new CodeFlow project",
		Long:  "Runs an interactive 7-step wizard to set up a CodeFlow project: location, prerequisites, authentication, git provider, configuration, setup, and verification.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runInit(cmd, dir)
		},
	}

	cmd.Flags().StringVar(&dir, "dir", "", "project directory (defaults to current directory)")

	return cmd
}

// runInit implements the init command logic.
func runInit(cmd *cobra.Command, dir string) error {
	w := &initialize.Wizard{
		In:       cmd.InOrStdin(),
		Out:      cmd.OutOrStdout(),
		Dir:      dir,
		LookPath: exec.LookPath,
		RunCmd:   defaultRunCmd,
	}

	ctx := cmd.Context()
	if ctx == nil {
		ctx = context.Background()
	}

	cfg, err := w.Run(ctx)
	if err != nil {
		return &exitError{code: classifyInitError(err), err: err}
	}

	fmt.Fprintf(cmd.OutOrStdout(), "\nProject: %s (%s)\n", cfg.ProjectName, cfg.Dir)
	return nil
}

// defaultRunCmd executes a command and returns its combined output.
func defaultRunCmd(name string, args ...string) ([]byte, error) {
	return exec.Command(name, args...).CombinedOutput()
}

// classifyInitError maps wizard errors to exit codes.
func classifyInitError(err error) int {
	switch {
	case errors.Is(err, initialize.ErrPrereqMissing):
		return ExitExternalError
	case errors.Is(err, initialize.ErrAuthFailed):
		return ExitExternalError
	case errors.Is(err, initialize.ErrGitProviderFailed):
		return ExitExternalError
	case errors.Is(err, initialize.ErrSetupFailed):
		return ExitConfigError
	case errors.Is(err, initialize.ErrVerificationFailed):
		return ExitConfigError
	case errors.Is(err, initialize.ErrCancelled):
		return ExitGeneralError
	default:
		return ExitGeneralError
	}
}
