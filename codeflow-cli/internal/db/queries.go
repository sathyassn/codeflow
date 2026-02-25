package db

import (
	"context"
	"encoding/json"
	"fmt"
)

// QueryToJSON executes a SELECT query and returns the results as a JSON array.
// Each row is a JSON object mapping column names to values.
func (d *DB) QueryToJSON(ctx context.Context, query string, args ...any) ([]byte, error) {
	rows, err := d.Query(ctx, query, args...)
	if err != nil {
		return nil, fmt.Errorf("db: query to JSON: %w", err)
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
			// Convert []byte to string for clean JSON output.
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

	// Return empty array instead of null for zero results.
	if results == nil {
		results = []map[string]any{}
	}

	data, err := json.MarshalIndent(results, "", "  ")
	if err != nil {
		return nil, fmt.Errorf("db: marshaling results: %w", err)
	}

	return data, nil
}

// QueryToMaps executes a SELECT query and returns results as a slice of maps.
func (d *DB) QueryToMaps(ctx context.Context, query string, args ...any) ([]map[string]any, error) {
	rows, err := d.Query(ctx, query, args...)
	if err != nil {
		return nil, fmt.Errorf("db: query to maps: %w", err)
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

	if results == nil {
		results = []map[string]any{}
	}

	return results, nil
}

// CountRows returns the number of rows in a table.
func (d *DB) CountRows(ctx context.Context, table string) (int64, error) {
	// Use a safe query pattern - table name is not parameterizable in SQLite,
	// but we validate it is a simple identifier.
	if !isValidIdentifier(table) {
		return 0, fmt.Errorf("db: invalid table name: %q", table)
	}
	var count int64
	query := fmt.Sprintf("SELECT COUNT(*) FROM %s", table) //nolint:gosec // table validated above
	if err := d.QueryRow(ctx, query).Scan(&count); err != nil {
		return 0, fmt.Errorf("db: counting rows in %s: %w", table, err)
	}
	return count, nil
}

// isValidIdentifier checks that a string is a safe SQL identifier (letters,
// digits, underscores only, starting with a letter or underscore).
func isValidIdentifier(s string) bool {
	if len(s) == 0 {
		return false
	}
	for i, c := range s {
		if i == 0 {
			if !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_') {
				return false
			}
		} else {
			if !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_') {
				return false
			}
		}
	}
	return true
}
