package cliutil

import (
	"bytes"
	"context"
	"log/slog"
	"os"
	"regexp"
	"strings"
	"testing"
	"time"
)

// logLineRe matches the expected log format: [timestamp] [LEVEL] message
var logLineRe = regexp.MustCompile(`^\[\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z\] \[(INFO|WARN|ERROR|DEBUG)\] .+\n$`)

// newTestLogger creates an isolated logger writing to buf. Each test gets its
// own instance, avoiding mutation of the package-global logger so tests can
// run in parallel.
func newTestLogger(buf *bytes.Buffer) *slog.Logger {
	return slog.New(newPlainHandler(buf))
}

func TestPlainHandler_InfoFormat(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	l := newTestLogger(&buf)

	l.Info("test message 42")

	got := buf.String()
	if !logLineRe.MatchString(got) {
		t.Errorf("Info output format mismatch.\ngot:  %q\nwant: [timestamp] [INFO] message", got)
	}
	if !strings.Contains(got, "[INFO]") {
		t.Errorf("Info output missing [INFO] tag: %q", got)
	}
	if !strings.Contains(got, "test message 42") {
		t.Errorf("Info output missing message: %q", got)
	}
}

func TestPlainHandler_WarnFormat(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	l := newTestLogger(&buf)

	l.Warn("warning here")

	got := buf.String()
	if !logLineRe.MatchString(got) {
		t.Errorf("Warn output format mismatch.\ngot:  %q\nwant: [timestamp] [WARN] message", got)
	}
	if !strings.Contains(got, "[WARN]") {
		t.Errorf("Warn output missing [WARN] tag: %q", got)
	}
	if !strings.Contains(got, "warning here") {
		t.Errorf("Warn output missing message: %q", got)
	}
}

func TestPlainHandler_ErrorFormat(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	l := newTestLogger(&buf)

	l.Error("error occurred: disk full")

	got := buf.String()
	if !logLineRe.MatchString(got) {
		t.Errorf("Error output format mismatch.\ngot:  %q\nwant: [timestamp] [ERROR] message", got)
	}
	if !strings.Contains(got, "[ERROR]") {
		t.Errorf("Error output missing [ERROR] tag: %q", got)
	}
	if !strings.Contains(got, "error occurred: disk full") {
		t.Errorf("Error output missing message: %q", got)
	}
}

func TestPlainHandler_DebugFormat(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	l := newTestLogger(&buf)

	l.Debug("debug info 99")

	got := buf.String()
	if !logLineRe.MatchString(got) {
		t.Errorf("Debug output format mismatch.\ngot:  %q\nwant: [timestamp] [DEBUG] message", got)
	}
	if !strings.Contains(got, "[DEBUG]") {
		t.Errorf("Debug output missing [DEBUG] tag: %q", got)
	}
	if !strings.Contains(got, "debug info 99") {
		t.Errorf("Debug output missing message: %q", got)
	}
}

func TestPlainHandler_MultipleMessages(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	l := newTestLogger(&buf)

	l.Info("first")
	l.Info("second")

	lines := strings.Split(strings.TrimRight(buf.String(), "\n"), "\n")
	if len(lines) != 2 {
		t.Errorf("expected 2 log lines, got %d: %q", len(lines), buf.String())
	}
}

func TestPlainHandler_FormatString(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	l := newTestLogger(&buf)

	l.Info("count=5 name=test")

	got := buf.String()
	if !strings.Contains(got, "count=5 name=test") {
		t.Errorf("format string interpolation failed: %q", got)
	}
}

func TestPlainHandler_Enabled(t *testing.T) {
	t.Parallel()

	h := newPlainHandler(&bytes.Buffer{})

	// Handler reports all levels as enabled; level gating happens
	// in the public Log* functions (e.g. LogDebug checks CODEFLOW_DEBUG).
	levels := []slog.Level{slog.LevelDebug, slog.LevelInfo, slog.LevelWarn, slog.LevelError}
	for _, lvl := range levels {
		if !h.Enabled(context.TODO(), lvl) {
			t.Errorf("Enabled(%v) = false, want true", lvl)
		}
	}
}

func TestPlainHandler_WithAttrsAndWithGroup(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	h := newPlainHandler(&buf)

	// WithAttrs and WithGroup are no-op interface methods required by slog.Handler.
	// They return the same handler unchanged.
	h2 := h.WithAttrs([]slog.Attr{slog.String("key", "val")})
	if h2 != h {
		t.Error("WithAttrs should return the same handler")
	}

	h3 := h.WithGroup("group")
	if h3 != h {
		t.Error("WithGroup should return the same handler")
	}
}

func TestPlainHandler_TimestampUTC(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	h := newPlainHandler(&buf)

	// Use a non-UTC timezone to verify the handler converts to UTC.
	rec := slog.NewRecord(time.Date(2025, 6, 15, 14, 30, 0, 0, time.FixedZone("EST", -5*3600)), slog.LevelInfo, "utc check", 0)
	if err := h.Handle(context.TODO(), rec); err != nil {
		t.Fatalf("Handle error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "2025-06-15T19:30:00Z") {
		t.Errorf("timestamp not in UTC: %q", got)
	}
}

// TestLogPublicAPI exercises the public Log* functions and SetLogOutput.
// Subtests run sequentially (no t.Parallel on subtests) because they share
// the package-global logger via SetLogOutput. The parent test IS parallel --
// safe because no other test in this file touches the global logger.
func TestLogPublicAPI(t *testing.T) {
	t.Parallel()

	t.Run("LogInfo format", func(t *testing.T) {
		var buf bytes.Buffer
		SetLogOutput(&buf)
		LogInfo("test message %d", 42)
		got := buf.String()
		if !logLineRe.MatchString(got) {
			t.Errorf("LogInfo output format mismatch.\ngot:  %q\nwant: [timestamp] [INFO] message", got)
		}
		if !strings.Contains(got, "[INFO]") {
			t.Errorf("LogInfo output missing [INFO] tag: %q", got)
		}
		if !strings.Contains(got, "test message 42") {
			t.Errorf("LogInfo output missing message: %q", got)
		}
	})

	t.Run("LogWarn format", func(t *testing.T) {
		var buf bytes.Buffer
		SetLogOutput(&buf)
		LogWarn("warning %s", "here")
		got := buf.String()
		if !strings.Contains(got, "[WARN]") {
			t.Errorf("LogWarn output missing [WARN] tag: %q", got)
		}
		if !strings.Contains(got, "warning here") {
			t.Errorf("LogWarn output missing message: %q", got)
		}
	})

	t.Run("LogError format", func(t *testing.T) {
		var buf bytes.Buffer
		SetLogOutput(&buf)
		LogError("error occurred: %v", "disk full")
		got := buf.String()
		if !strings.Contains(got, "[ERROR]") {
			t.Errorf("LogError output missing [ERROR] tag: %q", got)
		}
		if !strings.Contains(got, "error occurred: disk full") {
			t.Errorf("LogError output missing message: %q", got)
		}
	})

	t.Run("LogDebug enabled", func(t *testing.T) {
		var buf bytes.Buffer
		SetLogOutput(&buf)
		prev := os.Getenv("CODEFLOW_DEBUG")
		os.Setenv("CODEFLOW_DEBUG", "1")
		t.Cleanup(func() {
			if prev == "" {
				os.Unsetenv("CODEFLOW_DEBUG")
			} else {
				os.Setenv("CODEFLOW_DEBUG", prev)
			}
		})
		LogDebug("debug info %d", 99)
		got := buf.String()
		if !strings.Contains(got, "[DEBUG]") {
			t.Errorf("LogDebug with CODEFLOW_DEBUG=1 missing [DEBUG] tag: %q", got)
		}
		if !strings.Contains(got, "debug info 99") {
			t.Errorf("LogDebug output missing message: %q", got)
		}
	})

	t.Run("LogDebug disabled empty", func(t *testing.T) {
		var buf bytes.Buffer
		SetLogOutput(&buf)
		prev := os.Getenv("CODEFLOW_DEBUG")
		os.Setenv("CODEFLOW_DEBUG", "")
		t.Cleanup(func() {
			if prev == "" {
				os.Unsetenv("CODEFLOW_DEBUG")
			} else {
				os.Setenv("CODEFLOW_DEBUG", prev)
			}
		})
		LogDebug("should not appear")
		if buf.String() != "" {
			t.Errorf("LogDebug without CODEFLOW_DEBUG should produce no output, got: %q", buf.String())
		}
	})

	t.Run("LogDebug disabled zero", func(t *testing.T) {
		var buf bytes.Buffer
		SetLogOutput(&buf)
		prev := os.Getenv("CODEFLOW_DEBUG")
		os.Setenv("CODEFLOW_DEBUG", "0")
		t.Cleanup(func() {
			if prev == "" {
				os.Unsetenv("CODEFLOW_DEBUG")
			} else {
				os.Setenv("CODEFLOW_DEBUG", prev)
			}
		})
		LogDebug("should not appear")
		if buf.String() != "" {
			t.Errorf("LogDebug with CODEFLOW_DEBUG=0 should produce no output, got: %q", buf.String())
		}
	})

	t.Run("multiple messages", func(t *testing.T) {
		var buf bytes.Buffer
		SetLogOutput(&buf)
		LogInfo("first")
		LogInfo("second")
		lines := strings.Split(strings.TrimRight(buf.String(), "\n"), "\n")
		if len(lines) != 2 {
			t.Errorf("expected 2 log lines, got %d: %q", len(lines), buf.String())
		}
	})

	t.Run("format string interpolation", func(t *testing.T) {
		var buf bytes.Buffer
		SetLogOutput(&buf)
		LogInfo("count=%d name=%s", 5, "test")
		got := buf.String()
		if !strings.Contains(got, "count=5 name=test") {
			t.Errorf("LogInfo format string interpolation failed: %q", got)
		}
	})
}
