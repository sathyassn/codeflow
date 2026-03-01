package cliutil

import (
	"errors"
	"fmt"
)

// Exit codes for the codeflow CLI. These mirror the shell-lib conventions
// in .codeflow/scripts/shell-lib/errors.sh and extend them for Go-specific
// use cases.
const (
	// ExitSuccess indicates successful execution (hook: allow).
	ExitSuccess = 0

	// ExitGeneralError indicates a general runtime error.
	ExitGeneralError = 1

	// ExitConfigError indicates a configuration or setup error
	// (missing config, invalid flags, schema init failure).
	// Also used as the hook "block" exit code (exit 2 = block operation).
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

// ExitError wraps an error with a specific exit code.
type ExitError struct {
	Code int
	Err  error
}

func (e *ExitError) Error() string { return e.Err.Error() }
func (e *ExitError) Unwrap() error { return e.Err }

// NewExitError creates an ExitError with the given code and message.
func NewExitError(code int, format string, args ...any) *ExitError {
	return &ExitError{
		Code: code,
		Err:  fmt.Errorf(format, args...),
	}
}

// ExitCode extracts the exit code from an error. Returns ExitGeneralError
// if the error is not an *ExitError.
func ExitCode(err error) int {
	var ee *ExitError
	if errors.As(err, &ee) {
		return ee.Code
	}
	return ExitGeneralError
}

// ExitBlock returns an ExitError with code 2 (hook block convention).
func ExitBlock(reason string) *ExitError {
	return &ExitError{
		Code: ExitConfigError,
		Err:  fmt.Errorf("blocked: %s", reason),
	}
}

// ExitAllow returns nil, indicating that a hook allows the operation (exit 0).
// This is a semantic helper for hook implementations.
func ExitAllow() error {
	return nil
}
