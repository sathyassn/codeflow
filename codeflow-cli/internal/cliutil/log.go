package cliutil

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"os"
	"sync"
	"time"
)

// logger is the package-level slog.Logger used by LogInfo, LogWarn, etc.
// It writes to stderr with a plain-text format matching the shell-lib
// conventions: [timestamp] [LEVEL] message
var (
	loggerOnce sync.Once
	logger     *slog.Logger
)

// initLogger lazily initialises the package-level logger.
func initLogger() *slog.Logger {
	loggerOnce.Do(func() {
		logger = slog.New(newPlainHandler(os.Stderr))
	})
	return logger
}

// SetLogOutput replaces the log output writer. Intended for testing.
func SetLogOutput(w io.Writer) {
	loggerOnce = sync.Once{} // reset so next call re-creates
	logger = nil
	loggerOnce.Do(func() {
		logger = slog.New(newPlainHandler(w))
	})
}

// LogInfo writes an INFO-level message to stderr.
func LogInfo(format string, args ...any) {
	initLogger().Info(fmt.Sprintf(format, args...))
}

// LogWarn writes a WARN-level message to stderr.
func LogWarn(format string, args ...any) {
	initLogger().Warn(fmt.Sprintf(format, args...))
}

// LogError writes an ERROR-level message to stderr.
func LogError(format string, args ...any) {
	initLogger().Error(fmt.Sprintf(format, args...))
}

// LogDebug writes a DEBUG-level message to stderr, but only when
// the CODEFLOW_DEBUG environment variable is set to "1".
func LogDebug(format string, args ...any) {
	if os.Getenv("CODEFLOW_DEBUG") != "1" {
		return
	}
	initLogger().Debug(fmt.Sprintf(format, args...))
}

// plainHandler is a slog.Handler that produces plain-text output matching
// the shell-lib logging format: [timestamp] [LEVEL] message
type plainHandler struct {
	w io.Writer
}

func newPlainHandler(w io.Writer) *plainHandler {
	return &plainHandler{w: w}
}

func (h *plainHandler) Enabled(_ context.Context, _ slog.Level) bool {
	return true
}

func (h *plainHandler) Handle(_ context.Context, r slog.Record) error {
	ts := r.Time.UTC().Format(time.RFC3339)
	level := r.Level.String()
	_, err := fmt.Fprintf(h.w, "[%s] [%s] %s\n", ts, level, r.Message)
	return err
}

func (h *plainHandler) WithAttrs(_ []slog.Attr) slog.Handler {
	return h
}

func (h *plainHandler) WithGroup(_ string) slog.Handler {
	return h
}
