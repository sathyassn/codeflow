package idgen

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
)

// allowedTables is the allowlist of table names that NextFormatID may query.
// This prevents SQL injection since table names cannot be parameterized.
var allowedTables = map[string]bool{
	"tasks": true,
	"epics": true,
}

// Sentinel errors for format ID generation.
var (
	// ErrInvalidTable indicates the table name is not in the allowlist.
	ErrInvalidTable = errors.New("idgen: table not in allowlist")

	// ErrEmptyArea indicates the area type was empty.
	ErrEmptyArea = errors.New("idgen: area type must not be empty")
)

// Querier abstracts the database query interface needed by NextFormatID.
// Both *sql.DB (via QueryRowContext) and the project's *db.DB (via QueryRow)
// can be adapted to this interface.
type Querier interface {
	QueryRowContext(ctx context.Context, query string, args ...any) *sql.Row
}

// QueryRowFunc is an adapter that wraps a QueryRow-style function to satisfy
// the Querier interface. Use this to adapt db.DB.QueryRow which has
// the same signature as QueryRowContext.
type QueryRowFunc func(ctx context.Context, query string, args ...any) *sql.Row

// QueryRowContext implements the Querier interface by delegating to the
// wrapped function.
func (f QueryRowFunc) QueryRowContext(ctx context.Context, query string, args ...any) *sql.Row {
	return f(ctx, query, args...)
}

// NextFormatID queries the database for the highest existing format_id sequence
// number in the given table filtered by area_type, increments it, and returns
// the next sequential format ID zero-padded to 3 digits.
//
// Format ID patterns:
//   - epics: "{AREA}-EPC-{NNN}" (e.g., INF-EPC-021)
//   - tasks: "{AREA}-TSK-{epicNNN}-{NNN}" (e.g., INF-TSK-021-001)
//
// For tasks, the returned ID contains only the trailing 3-digit sequence.
// The caller is responsible for constructing the full task format ID by
// combining it with the epic number.
//
// The tableName parameter must be one of: "tasks", "epics".
// If the table is empty for the given area_type, the sequence starts at 1.
func NextFormatID(ctx context.Context, q Querier, tableName string, areaType string) (int, error) {
	if !allowedTables[tableName] {
		return 0, fmt.Errorf("%w: %q (allowed: tasks, epics)", ErrInvalidTable, tableName)
	}
	if areaType == "" {
		return 0, ErrEmptyArea
	}

	// Query the max trailing 3-digit sequence from existing format_id values.
	// SUBSTR(format_id, -3) extracts the last 3 characters (the sequence number).
	// CAST to INTEGER handles zero-padding (e.g., "001" -> 1).
	query := fmt.Sprintf( //nolint:gosec // tableName validated against allowlist above
		"SELECT COALESCE(MAX(CAST(SUBSTR(format_id, -3) AS INTEGER)), 0) FROM %s WHERE area_type = ?",
		tableName,
	)

	var maxSeq int
	if err := q.QueryRowContext(ctx, query, areaType).Scan(&maxSeq); err != nil {
		return 0, fmt.Errorf("idgen: querying max format_id in %s: %w", tableName, err)
	}

	return maxSeq + 1, nil
}

// FormatEpicID formats an epic format ID from area type and sequence number.
// Example: FormatEpicID("INF", 21) returns "INF-EPC-021".
func FormatEpicID(areaType string, seq int) string {
	return fmt.Sprintf("%s-EPC-%03d", areaType, seq)
}

// FormatTaskID formats a task format ID from area type, epic number, and
// task sequence number.
// Example: FormatTaskID("INF", 21, 1) returns "INF-TSK-021-001".
func FormatTaskID(areaType string, epicSeq int, taskSeq int) string {
	return fmt.Sprintf("%s-TSK-%03d-%03d", areaType, epicSeq, taskSeq)
}
