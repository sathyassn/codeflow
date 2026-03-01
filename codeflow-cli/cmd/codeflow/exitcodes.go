package main

import (
	"errors"

	"github.com/codeflow/codeflow-cli/internal/cliutil"
)

// Exit codes imported from the shared cliutil package. All cmd/ packages
// should use these constants via cliutil directly for new code.
const (
	ExitSuccess       = cliutil.ExitSuccess
	ExitGeneralError  = cliutil.ExitGeneralError
	ExitConfigError   = cliutil.ExitConfigError
	ExitRuntimeError  = cliutil.ExitRuntimeError
	ExitExternalError = cliutil.ExitExternalError
	ExitInternalError = cliutil.ExitInternalError
)

// exitError wraps an error with a specific exit code.
// Retained as a package-private type for backward compatibility with existing
// &exitError{code: ..., err: ...} constructions across cmd/codeflow files.
type exitError struct {
	code int
	err  error
}

func (e *exitError) Error() string { return e.err.Error() }
func (e *exitError) Unwrap() error { return e.err }

// exitCode extracts the exit code from an error. Checks both the
// package-private exitError and the shared cliutil.ExitError types.
func exitCode(err error) int {
	var ee *exitError
	if errors.As(err, &ee) {
		return ee.code
	}
	return cliutil.ExitCode(err)
}
