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

// defaultConfigDir returns the default config directory for codeflow.
func defaultConfigDir() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return filepath.Join("~", ".config", "codeflow")
	}
	return filepath.Join(home, ".config", "codeflow")
}

// newUninstallCmd creates the uninstall subcommand that removes the codeflow binary.
func newUninstallCmd() *cobra.Command {
	var (
		force      bool
		keepConfig bool
	)

	cmd := &cobra.Command{
		Use:   "uninstall",
		Short: "Remove the codeflow binary and config",
		Long:  "Remove the codeflow binary from ~/.local/bin/codeflow and optionally remove ~/.config/codeflow/.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runUninstall(cmd.InOrStdin(), cmd.OutOrStdout(), defaultBinaryPath(), defaultConfigDir(), force, keepConfig)
		},
	}

	cmd.Flags().BoolVarP(&force, "force", "f", false, "skip confirmation prompt")
	cmd.Flags().BoolVar(&keepConfig, "keep-config", false, "keep ~/.config/codeflow/ directory")

	return cmd
}

// runUninstall performs the uninstall logic. It accepts reader/writer for
// testability and the binary/config paths to allow testing without affecting real files.
func runUninstall(in io.Reader, out io.Writer, binaryPath, configDir string, force, keepConfig bool) error {
	binaryExists := true
	if _, err := os.Stat(binaryPath); os.IsNotExist(err) {
		binaryExists = false
	}

	configExists := true
	if _, err := os.Stat(configDir); os.IsNotExist(err) {
		configExists = false
	}

	if !binaryExists && (!configExists || keepConfig) {
		fmt.Fprintf(out, "codeflow binary not found at %s, nothing to remove\n", binaryPath)
		return nil
	}

	if !force {
		if keepConfig {
			fmt.Fprintf(out, "Remove codeflow binary at %s? [y/N] ", binaryPath)
		} else {
			fmt.Fprintf(out, "Remove codeflow binary at %s and config at %s? [y/N] ", binaryPath, configDir)
		}
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

	if binaryExists {
		if err := os.Remove(binaryPath); err != nil {
			return fmt.Errorf("removing %s: %w", binaryPath, err)
		}
		fmt.Fprintf(out, "removed %s\n", binaryPath)
	}

	if !keepConfig && configExists {
		if err := os.RemoveAll(configDir); err != nil {
			return fmt.Errorf("removing config directory %s: %w", configDir, err)
		}
		fmt.Fprintf(out, "removed %s\n", configDir)
	}

	return nil
}
