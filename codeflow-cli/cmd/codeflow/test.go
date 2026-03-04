package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"
)

// testRunnerScript is the path to the test runner relative to project root.
const testRunnerScript = ".codeflow/testing/run-all-tests.sh"

// newTestCmd creates the "test" command that runs the CodeFlow test suite.
func newTestCmd() *cobra.Command {
	var (
		coverage bool
		mode     string
		verbose  bool
	)

	cmd := &cobra.Command{
		Use:   "test",
		Short: "Run the CodeFlow test suite",
		Long:  "Delegates to .codeflow/testing/run-all-tests.sh with optional flags for coverage, mode, and verbosity.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runTest(cmd, coverage, mode, verbose)
		},
	}

	cmd.Flags().BoolVar(&coverage, "coverage", false, "run with coverage enforcement")
	cmd.Flags().StringVar(&mode, "mode", "", "test mode: standard, quick, full, or essential")
	cmd.Flags().BoolVar(&verbose, "verbose", false, "enable verbose output")

	return cmd
}

// findProjectRoot walks up from cwd looking for a directory containing
// .codeflow/. Returns the path or an error.
func findProjectRoot() (string, error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", fmt.Errorf("getting working directory: %w", err)
	}

	for {
		candidate := filepath.Join(dir, ".codeflow")
		if info, err := os.Stat(candidate); err == nil && info.IsDir() {
			return dir, nil
		}

		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}

	return "", fmt.Errorf("project root not found (no .codeflow/ directory in parent chain)")
}

// validTestModes lists the accepted values for --mode.
var validTestModes = map[string]bool{
	"standard":  true,
	"quick":     true,
	"full":      true,
	"essential": true,
}

// runTest implements the test command logic.
func runTest(cmd *cobra.Command, coverage bool, mode string, verbose bool) error {
	if mode != "" && !validTestModes[mode] {
		return &exitError{
			code: ExitConfigError,
			err:  fmt.Errorf("invalid test mode %q: must be one of %s", mode, strings.Join(testModeNames(), ", ")),
		}
	}

	root, err := findProjectRoot()
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: err}
	}

	scriptPath := filepath.Join(root, testRunnerScript)
	if _, err := os.Stat(scriptPath); err != nil {
		return &exitError{
			code: ExitRuntimeError,
			err:  fmt.Errorf("test runner not found at %s", scriptPath),
		}
	}

	args := []string{scriptPath}
	if coverage {
		args = append(args, "--coverage")
	}
	if mode != "" {
		args = append(args, "--mode", mode)
	}
	if verbose {
		args = append(args, "--verbose")
	}

	c := exec.Command("bash", args...)
	c.Dir = root
	c.Stdout = cmd.OutOrStdout()
	c.Stderr = cmd.ErrOrStderr()
	c.Stdin = cmd.InOrStdin()

	if err := c.Run(); err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return &exitError{
				code: exitErr.ExitCode(),
				err:  fmt.Errorf("test suite failed with exit code %d", exitErr.ExitCode()),
			}
		}
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("running test suite: %w", err)}
	}

	return nil
}

// testModeNames returns sorted mode names for error messages.
func testModeNames() []string {
	return []string{"essential", "full", "quick", "standard"}
}
