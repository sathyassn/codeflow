package main

import "errors"

// Exit codes for the codeflow CLI.
const (
	// ExitSuccess indicates successful execution.
	ExitSuccess = 0

	// ExitGeneralError indicates a general runtime error.
	ExitGeneralError = 1

	// ExitConfigError indicates a configuration or setup error
	// (missing config, invalid flags, schema init failure).
	ExitConfigError = 2

	// ExitRuntimeError indicates a runtime failure during command execution
	// (query failed, sync failed, migration failed).
	ExitRuntimeError = 3

	// ExitExternalError indicates a failure in an external dependency
	// (git not found, network unreachable, external API error).
	ExitExternalError = 4

	// ExitInternalError indicates an internal/unexpected error
	// (panic recovery, assertion failure, corrupt state).
	ExitInternalError = 5
)

// exitError wraps an error with a specific exit code.
type exitError struct {
	code int
	err  error
}

func (e *exitError) Error() string { return e.err.Error() }
func (e *exitError) Unwrap() error { return e.err }

// exitCode extracts the exit code from an error. Returns ExitGeneralError
// if the error is not an *exitError.
func exitCode(err error) int {
	var ee *exitError
	if errors.As(err, &ee) {
		return ee.code
	}
	return ExitGeneralError
}
