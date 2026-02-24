package main

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"
)

// defaultBinaryPath returns the default installation path for the codeflow binary.
func defaultBinaryPath() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return filepath.Join("~", ".local", "bin", "codeflow")
	}
	return filepath.Join(home, ".local", "bin", "codeflow")
}

// newUninstallCmd creates the uninstall subcommand that removes the codeflow binary.
func newUninstallCmd() *cobra.Command {
	var force bool

	cmd := &cobra.Command{
		Use:   "uninstall",
		Short: "Remove the codeflow binary",
		Long:  "Remove the codeflow binary from ~/.local/bin/codeflow.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runUninstall(cmd.InOrStdin(), cmd.OutOrStdout(), defaultBinaryPath(), force)
		},
	}

	cmd.Flags().BoolVarP(&force, "force", "f", false, "skip confirmation prompt")

	return cmd
}

// runUninstall performs the uninstall logic. It accepts reader/writer for
// testability and the binary path to allow testing without affecting real files.
func runUninstall(in io.Reader, out io.Writer, binaryPath string, force bool) error {
	if _, err := os.Stat(binaryPath); os.IsNotExist(err) {
		fmt.Fprintf(out, "codeflow binary not found at %s, nothing to remove\n", binaryPath)
		return nil
	}

	if !force {
		fmt.Fprintf(out, "Remove codeflow binary at %s? [y/N] ", binaryPath)
		scanner := bufio.NewScanner(in)
		if !scanner.Scan() {
			return fmt.Errorf("reading confirmation: %w", scanner.Err())
		}
		answer := strings.TrimSpace(strings.ToLower(scanner.Text()))
		if answer != "y" && answer != "yes" {
			fmt.Fprintln(out, "uninstall cancelled")
			return nil
		}
	}

	if err := os.Remove(binaryPath); err != nil {
		return fmt.Errorf("removing %s: %w", binaryPath, err)
	}

	fmt.Fprintf(out, "removed %s\n", binaryPath)
	return nil
}
