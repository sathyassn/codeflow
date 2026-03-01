package main

import (
	"errors"
	"fmt"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/cliutil"
)

func TestExitCodes(t *testing.T) {
	t.Parallel()

	if ExitSuccess != 0 {
		t.Errorf("ExitSuccess = %d, want 0", ExitSuccess)
	}
	if ExitGeneralError != 1 {
		t.Errorf("ExitGeneralError = %d, want 1", ExitGeneralError)
	}
	if ExitConfigError != 2 {
		t.Errorf("ExitConfigError = %d, want 2", ExitConfigError)
	}
	if ExitRuntimeError != 3 {
		t.Errorf("ExitRuntimeError = %d, want 3", ExitRuntimeError)
	}
	if ExitExternalError != 4 {
		t.Errorf("ExitExternalError = %d, want 4", ExitExternalError)
	}
	if ExitInternalError != 5 {
		t.Errorf("ExitInternalError = %d, want 5", ExitInternalError)
	}

	// Verify all 6 are distinct.
	codes := map[int]string{
		ExitSuccess:       "ExitSuccess",
		ExitGeneralError:  "ExitGeneralError",
		ExitConfigError:   "ExitConfigError",
		ExitRuntimeError:  "ExitRuntimeError",
		ExitExternalError: "ExitExternalError",
		ExitInternalError: "ExitInternalError",
	}
	if len(codes) != 6 {
		t.Error("exit codes must be distinct values")
	}
}

func TestExitError(t *testing.T) {
	t.Parallel()

	t.Run("Error returns wrapped error message", func(t *testing.T) {
		t.Parallel()
		ee := &exitError{code: ExitConfigError, err: fmt.Errorf("bad config")}
		if ee.Error() != "bad config" {
			t.Errorf("Error() = %q, want %q", ee.Error(), "bad config")
		}
	})

	t.Run("Unwrap returns wrapped error", func(t *testing.T) {
		t.Parallel()
		inner := fmt.Errorf("inner error")
		ee := &exitError{code: ExitRuntimeError, err: inner}
		if !errors.Is(ee, inner) {
			t.Error("Unwrap should return the inner error")
		}
	})
}

func TestExitCode(t *testing.T) {
	t.Parallel()

	t.Run("returns code from exitError", func(t *testing.T) {
		t.Parallel()
		err := &exitError{code: ExitConfigError, err: fmt.Errorf("test")}
		if got := exitCode(err); got != ExitConfigError {
			t.Errorf("exitCode = %d, want %d", got, ExitConfigError)
		}
	})

	t.Run("returns ExitGeneralError for plain error", func(t *testing.T) {
		t.Parallel()
		err := fmt.Errorf("plain error")
		if got := exitCode(err); got != ExitGeneralError {
			t.Errorf("exitCode = %d, want %d", got, ExitGeneralError)
		}
	})

	t.Run("returns code from wrapped exitError", func(t *testing.T) {
		t.Parallel()
		inner := &exitError{code: ExitExternalError, err: fmt.Errorf("git failed")}
		wrapped := fmt.Errorf("command failed: %w", inner)
		if got := exitCode(wrapped); got != ExitExternalError {
			t.Errorf("exitCode = %d, want %d (should unwrap)", got, ExitExternalError)
		}
	})

	t.Run("returns code from cliutil ExitError", func(t *testing.T) {
		t.Parallel()
		err := cliutil.NewExitError(cliutil.ExitConfigError, "bad config")
		if got := exitCode(err); got != ExitConfigError {
			t.Errorf("exitCode = %d, want %d (should handle cliutil.ExitError)", got, ExitConfigError)
		}
	})
}

func TestExitCodes_MatchCliutil(t *testing.T) {
	t.Parallel()

	// Verify re-exported constants match the cliutil source values.
	tests := []struct {
		name   string
		pkg    int
		shared int
	}{
		{"ExitSuccess", ExitSuccess, cliutil.ExitSuccess},
		{"ExitGeneralError", ExitGeneralError, cliutil.ExitGeneralError},
		{"ExitConfigError", ExitConfigError, cliutil.ExitConfigError},
		{"ExitRuntimeError", ExitRuntimeError, cliutil.ExitRuntimeError},
		{"ExitExternalError", ExitExternalError, cliutil.ExitExternalError},
		{"ExitInternalError", ExitInternalError, cliutil.ExitInternalError},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if tt.pkg != tt.shared {
				t.Errorf("%s: package value %d != cliutil value %d", tt.name, tt.pkg, tt.shared)
			}
		})
	}
}
