package db

import (
	"bytes"
	"log/slog"
	"testing"
)

func TestNewQueryLogger(t *testing.T) {
	t.Parallel()
	t.Run("creates logger with provided slog", func(t *testing.T) {
		d := newTestDB(t)
		logger := slog.New(slog.NewTextHandler(&bytes.Buffer{}, nil))

		ql := NewQueryLogger(d, logger)
		if ql == nil {
			t.Fatal("expected non-nil QueryLogger")
		}
		if ql.db != d {
			t.Error("expected QueryLogger.db to match input")
		}
	})

	t.Run("creates logger with nil slog uses default", func(t *testing.T) {
		d := newTestDB(t)

		ql := NewQueryLogger(d, nil)
		if ql == nil {
			t.Fatal("expected non-nil QueryLogger")
		}
		if ql.logger == nil {
			t.Error("expected non-nil logger even when nil passed")
		}
	})
}

func TestLogQuery(t *testing.T) {
	t.Parallel()
	t.Run("returns query results and logs", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE log_test (id INTEGER PRIMARY KEY, name TEXT);
			INSERT INTO log_test VALUES (1, 'alice');
			INSERT INTO log_test VALUES (2, 'bob');
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		var buf bytes.Buffer
		logger := slog.New(slog.NewTextHandler(&buf, &slog.HandlerOptions{Level: slog.LevelDebug}))
		ql := NewQueryLogger(d, logger)

		results, err := ql.LogQuery(ctx, "SELECT * FROM log_test ORDER BY id")
		if err != nil {
			t.Fatalf("LogQuery: %v", err)
		}

		if len(results) != 2 {
			t.Errorf("got %d rows, want 2", len(results))
		}

		logOutput := buf.String()
		if logOutput == "" {
			t.Error("expected log output, got empty")
		}
	})

	t.Run("logs error on failed query", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		var buf bytes.Buffer
		logger := slog.New(slog.NewTextHandler(&buf, &slog.HandlerOptions{Level: slog.LevelDebug}))
		ql := NewQueryLogger(d, logger)

		_, err := ql.LogQuery(ctx, "SELECT * FROM does_not_exist")
		if err == nil {
			t.Error("expected error for nonexistent table")
		}

		logOutput := buf.String()
		if logOutput == "" {
			t.Error("expected log output for failed query")
		}
	})

	t.Run("returns empty results for no rows", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, "CREATE TABLE log_empty (id INTEGER)")
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		var buf bytes.Buffer
		logger := slog.New(slog.NewTextHandler(&buf, nil))
		ql := NewQueryLogger(d, logger)

		results, err := ql.LogQuery(ctx, "SELECT * FROM log_empty")
		if err != nil {
			t.Fatalf("LogQuery: %v", err)
		}

		if len(results) != 0 {
			t.Errorf("got %d rows, want 0", len(results))
		}
	})

	t.Run("handles parameterized queries", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE log_param (k TEXT, v TEXT);
			INSERT INTO log_param VALUES ('x', 'found');
			INSERT INTO log_param VALUES ('y', 'other');
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		var buf bytes.Buffer
		logger := slog.New(slog.NewTextHandler(&buf, nil))
		ql := NewQueryLogger(d, logger)

		results, err := ql.LogQuery(ctx, "SELECT v FROM log_param WHERE k = ?", "x")
		if err != nil {
			t.Fatalf("LogQuery: %v", err)
		}

		if len(results) != 1 || results[0]["v"] != "found" {
			t.Errorf("unexpected results: %v", results)
		}
	})
}

func TestLogExec(t *testing.T) {
	t.Parallel()
	t.Run("returns rows affected and logs", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE log_exec (id INTEGER PRIMARY KEY, status TEXT);
			INSERT INTO log_exec VALUES (1, 'old');
			INSERT INTO log_exec VALUES (2, 'old');
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		var buf bytes.Buffer
		logger := slog.New(slog.NewTextHandler(&buf, &slog.HandlerOptions{Level: slog.LevelDebug}))
		ql := NewQueryLogger(d, logger)

		affected, err := ql.LogExec(ctx, "UPDATE log_exec SET status = 'new' WHERE status = 'old'")
		if err != nil {
			t.Fatalf("LogExec: %v", err)
		}

		if affected != 2 {
			t.Errorf("affected = %d, want 2", affected)
		}

		logOutput := buf.String()
		if logOutput == "" {
			t.Error("expected log output")
		}
	})

	t.Run("logs error on failed exec", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		var buf bytes.Buffer
		logger := slog.New(slog.NewTextHandler(&buf, &slog.HandlerOptions{Level: slog.LevelDebug}))
		ql := NewQueryLogger(d, logger)

		_, err := ql.LogExec(ctx, "INSERT INTO nonexistent VALUES (1)")
		if err == nil {
			t.Error("expected error")
		}

		logOutput := buf.String()
		if logOutput == "" {
			t.Error("expected log output for failed exec")
		}
	})
}

func TestTruncateQuery(t *testing.T) {
	t.Parallel()
	tests := []struct {
		name  string
		input string
		want  string
	}{
		{
			name:  "short query unchanged",
			input: "SELECT * FROM users",
			want:  "SELECT * FROM users",
		},
		{
			name:  "exactly 200 chars unchanged",
			input: string(make([]byte, 200)),
			want:  string(make([]byte, 200)),
		},
		{
			name:  "long query truncated",
			input: string(make([]byte, 250)),
			want:  string(make([]byte, 200)) + "...",
		},
		{
			name:  "empty string",
			input: "",
			want:  "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := truncateQuery(tt.input)
			if got != tt.want {
				t.Errorf("truncateQuery len=%d, want len=%d", len(got), len(tt.want))
			}
		})
	}
}
