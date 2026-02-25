// Package main provides the codeflow CLI entry point.
package main

import (
	"context"
	"fmt"
	"os"

	"github.com/spf13/cobra"
)

// version is set at build time via -ldflags "-X main.version=$(VERSION)".
var version = "dev"

// newRootCmd creates the root cobra command with all subcommands registered.
func newRootCmd() *cobra.Command {
	rootCmd := &cobra.Command{
		Use:   "codeflow",
		Short: "AI-native development framework CLI",
		Long:  "CodeFlow CLI provides tools for AI-native development workflows.",
		// Show help when no subcommand is given.
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
		// Disable cobra's default completion command.
		CompletionOptions: cobra.CompletionOptions{
			DisableDefaultCmd: true,
		},
		// Silence cobra's built-in error/usage printing so run() controls output.
		SilenceErrors: true,
		SilenceUsage:  true,
	}

	rootCmd.Version = version
	rootCmd.SetVersionTemplate("codeflow {{.Version}}\n")

	rootCmd.AddCommand(newVersionCmd())
	rootCmd.AddCommand(newUninstallCmd())
	rootCmd.AddCommand(newDBCmd())
	rootCmd.AddCommand(newSessionCmd())

	return rootCmd
}

func main() {
	ctx := context.Background()
	if err := run(ctx); err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}
}

// run executes the CLI logic. Accepts context.Context for cancellation
// and deadline propagation to subcommands.
func run(ctx context.Context) error {
	return newRootCmd().ExecuteContext(ctx)
}
