// Package main provides the codeflow CLI entry point.
package main

import (
	"context"
	"fmt"
	"os"
)

// version is set at build time via -ldflags.
var version = "dev"

func main() {
	ctx := context.Background()
	if err := run(ctx, os.Args[1:]); err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}
}

// run executes the CLI logic. Accepts context.Context as first parameter
// per Go convention for cancellation and deadline propagation.
func run(ctx context.Context, args []string) error {
	_ = ctx // will be used by subcommands that perform I/O
	if len(args) > 0 && args[0] == "version" {
		fmt.Printf("codeflow %s\n", version)
		return nil
	}
	fmt.Println("codeflow CLI")
	return nil
}
