// Package security provides pre-tool-use security enforcement for CodeFlow hooks.
//
// It consolidates 9 shell enforcement modules into a single Go package:
// dangerous command detection, privilege escalation prevention,
// git protection, path protection, file operation validation,
// branch file protection, tmp file validation, network protection,
// and pattern matching utilities.
//
// Usage:
//
//	checker := security.NewChecker()
//	verdict := checker.Check(ctx)
//	if !verdict.Allow {
//	    fmt.Fprintf(os.Stderr, "%s", verdict.Message())
//	    os.Exit(2)
//	}
package security
