package db

import (
	"context"
	"database/sql"
	"fmt"
	"strings"
)

// ExecResult contains the outcome of an exec operation.
type ExecResult struct {
	RowsAffected int64 `json:"rows_affected"`
	LastInsertID int64 `json:"last_insert_id"`
}

// ExecWithResult executes a write statement and returns a structured result.
func (d *DB) ExecWithResult(ctx context.Context, query string, args ...any) (*ExecResult, error) {
	result, err := d.Execute(ctx, query, args...)
	if err != nil {
		return nil, fmt.Errorf("db: exec with result: %w", err)
	}

	affected, _ := result.RowsAffected()
	lastID, _ := result.LastInsertId()

	return &ExecResult{
		RowsAffected: affected,
		LastInsertID:  lastID,
	}, nil
}

// BatchExec executes multiple statements within a single transaction.
// Each statement is a query string with optional args.
func (d *DB) BatchExec(ctx context.Context, statements []Statement) (*BatchResult, error) {
	if len(statements) == 0 {
		return &BatchResult{}, nil
	}

	var result BatchResult

	err := d.Transaction(ctx, func(tx *sql.Tx) error {
		for i, stmt := range statements {
			res, err := tx.ExecContext(ctx, stmt.Query, stmt.Args...)
			if err != nil {
				return fmt.Errorf("db: statement %d (%s): %w", i, truncateQuery(stmt.Query), err)
			}
			affected, _ := res.RowsAffected()
			result.TotalAffected += affected
			result.StatementCount++
		}
		return nil
	})
	if err != nil {
		return nil, fmt.Errorf("db: batch exec: %w", err)
	}

	return &result, nil
}

// Statement represents a SQL statement with parameters.
type Statement struct {
	Query string
	Args  []any
}

// BatchResult contains the outcome of a batch execution.
type BatchResult struct {
	StatementCount int   `json:"statement_count"`
	TotalAffected  int64 `json:"total_affected"`
}

// InsertRow inserts a single row into the specified table using column-value pairs.
// The table name is validated as a safe identifier.
func (d *DB) InsertRow(ctx context.Context, table string, columns []string, values []any) (*ExecResult, error) {
	if !isValidIdentifier(table) {
		return nil, fmt.Errorf("db: invalid table name: %q", table)
	}
	if len(columns) == 0 {
		return nil, fmt.Errorf("db: no columns specified for insert into %s", table)
	}
	if len(columns) != len(values) {
		return nil, fmt.Errorf("db: column count (%d) does not match value count (%d)", len(columns), len(values))
	}

	for _, col := range columns {
		if !isValidIdentifier(col) {
			return nil, fmt.Errorf("db: invalid column name: %q", col)
		}
	}

	// Build parameterized INSERT.
	var colBuilder strings.Builder
	var phBuilder strings.Builder
	for i, col := range columns {
		if i > 0 {
			colBuilder.WriteString(", ")
			phBuilder.WriteString(", ")
		}
		colBuilder.WriteString(col)
		phBuilder.WriteByte('?')
	}

	query := fmt.Sprintf("INSERT INTO %s (%s) VALUES (%s)", table, colBuilder.String(), phBuilder.String()) //nolint:gosec // table and columns validated above
	return d.ExecWithResult(ctx, query, values...)
}
