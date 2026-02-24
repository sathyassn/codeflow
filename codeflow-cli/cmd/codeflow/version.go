package main

import (
	"fmt"

	"github.com/spf13/cobra"
)

// newVersionCmd creates the version subcommand that prints the CLI version.
func newVersionCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "version",
		Short: "Print the codeflow version",
		Args:  cobra.NoArgs,
		Run: func(cmd *cobra.Command, _ []string) {
			fmt.Fprintf(cmd.OutOrStdout(), "codeflow %s\n", version)
		},
	}
}
