package db

import (
	"context"
	"fmt"
	"log/slog"
	"time"
)

// LogEntry represents a database operation log entry.
type LogEntry struct {
	Operation string `json:"operation"`
	Query     string `json:"query"`
	Duration  string `json:"duration"`
	Error     string `json:"error,omitempty"`
}

// QueryLogger wraps a DB and logs query execution to slog.
type QueryLogger struct {
	db     *DB
	logger *slog.Logger
}

// NewQueryLogger creates a QueryLogger that wraps the given DB instance.
func NewQueryLogger(db *DB, logger *slog.Logger) *QueryLogger {
	if logger == nil {
		logger = slog.Default()
	}
	return &QueryLogger{db: db, logger: logger}
}

// LogQuery executes a SELECT query and logs the operation.
func (ql *QueryLogger) LogQuery(ctx context.Context, query string, args ...any) ([]map[string]any, error) {
	start := time.Now()

	rows, err := ql.db.Query(ctx, query, args...)
	duration := time.Since(start)

	if err != nil {
		ql.logger.WarnContext(ctx, "query failed",
			slog.String("query", truncateQuery(query)),
			slog.Duration("duration", duration),
			slog.String("error", err.Error()),
		)
		return nil, fmt.Errorf("db: logged query: %w", err)
	}
	defer rows.Close()

	columns, err := rows.Columns()
	if err != nil {
		return nil, fmt.Errorf("db: reading columns: %w", err)
	}

	var results []map[string]any
	for rows.Next() {
		values := make([]any, len(columns))
		valuePtrs := make([]any, len(columns))
		for i := range values {
			valuePtrs[i] = &values[i]
		}

		if err := rows.Scan(valuePtrs...); err != nil {
			return nil, fmt.Errorf("db: scanning row: %w", err)
		}

		row := make(map[string]any, len(columns))
		for i, col := range columns {
			val := values[i]
			// Convert []byte to string for JSON serialization.
			if b, ok := val.([]byte); ok {
				val = string(b)
			}
			row[col] = val
		}
		results = append(results, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("db: iterating rows: %w", err)
	}

	ql.logger.InfoContext(ctx, "query executed",
		slog.String("query", truncateQuery(query)),
		slog.Duration("duration", duration),
		slog.Int("rows", len(results)),
	)

	return results, nil
}

// LogExec executes a write statement and logs the operation.
func (ql *QueryLogger) LogExec(ctx context.Context, query string, args ...any) (int64, error) {
	start := time.Now()

	result, err := ql.db.Execute(ctx, query, args...)
	duration := time.Since(start)

	if err != nil {
		ql.logger.WarnContext(ctx, "exec failed",
			slog.String("query", truncateQuery(query)),
			slog.Duration("duration", duration),
			slog.String("error", err.Error()),
		)
		return 0, fmt.Errorf("db: logged exec: %w", err)
	}

	affected, _ := result.RowsAffected()

	ql.logger.InfoContext(ctx, "exec completed",
		slog.String("query", truncateQuery(query)),
		slog.Duration("duration", duration),
		slog.Int64("rows_affected", affected),
	)

	return affected, nil
}

// truncateQuery limits query string length for log readability.
func truncateQuery(q string) string {
	const maxLen = 200
	if len(q) <= maxLen {
		return q
	}
	return q[:maxLen] + "..."
}
