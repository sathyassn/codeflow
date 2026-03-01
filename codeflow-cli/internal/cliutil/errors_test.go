package cliutil

import (
	"errors"
	"fmt"
	"testing"
)

func TestExitCode_WithExitError(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		err      error
		wantCode int
	}{
		{
			name:     "ExitSuccess code",
			err:      &ExitError{Code: ExitSuccess, Err: errors.New("ok")},
			wantCode: ExitSuccess,
		},
		{
			name:     "ExitConfigError code",
			err:      &ExitError{Code: ExitConfigError, Err: errors.New("config")},
			wantCode: ExitConfigError,
		},
		{
			name:     "ExitRuntimeError code",
			err:      &ExitError{Code: ExitRuntimeError, Err: errors.New("runtime")},
			wantCode: ExitRuntimeError,
		},
		{
			name:     "ExitExternalError code",
			err:      &ExitError{Code: ExitExternalError, Err: errors.New("external")},
			wantCode: ExitExternalError,
		},
		{
			name:     "ExitInternalError code",
			err:      &ExitError{Code: ExitInternalError, Err: errors.New("internal")},
			wantCode: ExitInternalError,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := ExitCode(tt.err)
			if got != tt.wantCode {
				t.Errorf("ExitCode() = %d, want %d", got, tt.wantCode)
			}
		})
	}
}

func TestExitCode_NonExitError(t *testing.T) {
	t.Parallel()

	got := ExitCode(errors.New("plain error"))
	if got != ExitGeneralError {
		t.Errorf("ExitCode(plain error) = %d, want %d", got, ExitGeneralError)
	}
}

func TestExitCode_WrappedExitError(t *testing.T) {
	t.Parallel()

	inner := &ExitError{Code: ExitRuntimeError, Err: errors.New("inner")}
	wrapped := fmt.Errorf("outer: %w", inner)

	got := ExitCode(wrapped)
	if got != ExitRuntimeError {
		t.Errorf("ExitCode(wrapped) = %d, want %d", got, ExitRuntimeError)
	}
}

func TestExitError_Error(t *testing.T) {
	t.Parallel()

	e := &ExitError{Code: ExitConfigError, Err: errors.New("bad config")}
	if e.Error() != "bad config" {
		t.Errorf("ExitError.Error() = %q, want %q", e.Error(), "bad config")
	}
}

func TestExitError_Unwrap(t *testing.T) {
	t.Parallel()

	inner := errors.New("root cause")
	e := &ExitError{Code: ExitGeneralError, Err: inner}
	if !errors.Is(e, inner) {
		t.Error("ExitError.Unwrap() does not expose inner error via errors.Is")
	}
}

func TestNewExitError(t *testing.T) {
	t.Parallel()

	e := NewExitError(ExitConfigError, "missing %s", "file")
	if e.Code != ExitConfigError {
		t.Errorf("NewExitError().Code = %d, want %d", e.Code, ExitConfigError)
	}
	if e.Error() != "missing file" {
		t.Errorf("NewExitError().Error() = %q, want %q", e.Error(), "missing file")
	}
}

func TestExitBlock(t *testing.T) {
	t.Parallel()

	e := ExitBlock("not allowed")
	if e.Code != ExitConfigError {
		t.Errorf("ExitBlock().Code = %d, want %d (ExitConfigError/block)", e.Code, ExitConfigError)
	}
	if e.Error() != "blocked: not allowed" {
		t.Errorf("ExitBlock().Error() = %q, want %q", e.Error(), "blocked: not allowed")
	}
}

func TestExitAllow(t *testing.T) {
	t.Parallel()

	err := ExitAllow()
	if err != nil {
		t.Errorf("ExitAllow() = %v, want nil", err)
	}
}

func TestExitCodeConstants(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name  string
		code  int
		value int
	}{
		{"ExitSuccess", ExitSuccess, 0},
		{"ExitGeneralError", ExitGeneralError, 1},
		{"ExitConfigError", ExitConfigError, 2},
		{"ExitRuntimeError", ExitRuntimeError, 3},
		{"ExitExternalError", ExitExternalError, 4},
		{"ExitInternalError", ExitInternalError, 5},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if tt.code != tt.value {
				t.Errorf("%s = %d, want %d", tt.name, tt.code, tt.value)
			}
		})
	}
}
