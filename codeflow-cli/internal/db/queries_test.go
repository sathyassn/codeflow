package db

import (
	"encoding/json"
	"testing"
)

func TestQueryToJSON(t *testing.T) {
	t.Parallel()
	t.Run("returns JSON array from SELECT", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, value REAL);
			INSERT INTO items (name, value) VALUES ('alpha', 1.5);
			INSERT INTO items (name, value) VALUES ('beta', 2.5);
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		data, err := d.QueryToJSON(ctx, "SELECT name, value FROM items ORDER BY name")
		if err != nil {
			t.Fatalf("QueryToJSON: %v", err)
		}

		var results []map[string]any
		if err := json.Unmarshal(data, &results); err != nil {
			t.Fatalf("unmarshal: %v", err)
		}

		if len(results) != 2 {
			t.Fatalf("got %d rows, want 2", len(results))
		}
		if results[0]["name"] != "alpha" {
			t.Errorf("first row name = %v, want alpha", results[0]["name"])
		}
	})

	t.Run("returns empty array for no results", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, "CREATE TABLE empty_tbl (id INTEGER PRIMARY KEY)")
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		data, err := d.QueryToJSON(ctx, "SELECT * FROM empty_tbl")
		if err != nil {
			t.Fatalf("QueryToJSON: %v", err)
		}

		if string(data) != "[]" {
			t.Errorf("expected empty JSON array, got %s", string(data))
		}
	})

	t.Run("handles parameterized queries", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE kv (key TEXT, val TEXT);
			INSERT INTO kv VALUES ('a', 'one');
			INSERT INTO kv VALUES ('b', 'two');
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		data, err := d.QueryToJSON(ctx, "SELECT val FROM kv WHERE key = ?", "b")
		if err != nil {
			t.Fatalf("QueryToJSON: %v", err)
		}

		var results []map[string]any
		if err := json.Unmarshal(data, &results); err != nil {
			t.Fatalf("unmarshal: %v", err)
		}

		if len(results) != 1 || results[0]["val"] != "two" {
			t.Errorf("unexpected results: %s", string(data))
		}
	})

	t.Run("returns error for invalid SQL", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.QueryToJSON(ctx, "SELECT * FROM nonexistent_table")
		if err == nil {
			t.Error("expected error for nonexistent table")
		}
	})

	t.Run("converts byte arrays to strings", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE blobs (id INTEGER PRIMARY KEY, data BLOB);
			INSERT INTO blobs (data) VALUES (x'48656c6c6f');
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		data, err := d.QueryToJSON(ctx, "SELECT data FROM blobs")
		if err != nil {
			t.Fatalf("QueryToJSON: %v", err)
		}

		var results []map[string]any
		if err := json.Unmarshal(data, &results); err != nil {
			t.Fatalf("unmarshal: %v", err)
		}

		// Verify data was converted to string (not base64 blob).
		if _, ok := results[0]["data"].(string); !ok {
			t.Errorf("expected string type for blob, got %T", results[0]["data"])
		}
	})
}

func TestQueryToMaps(t *testing.T) {
	t.Parallel()
	t.Run("returns maps from SELECT", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE people (name TEXT, age INTEGER);
			INSERT INTO people VALUES ('Alice', 30);
			INSERT INTO people VALUES ('Bob', 25);
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		results, err := d.QueryToMaps(ctx, "SELECT * FROM people ORDER BY name")
		if err != nil {
			t.Fatalf("QueryToMaps: %v", err)
		}

		if len(results) != 2 {
			t.Fatalf("got %d rows, want 2", len(results))
		}
		if results[0]["name"] != "Alice" {
			t.Errorf("first row name = %v, want Alice", results[0]["name"])
		}
	})

	t.Run("handles parameterized queries", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE kv2 (key TEXT, val TEXT);
			INSERT INTO kv2 VALUES ('x', 'one');
			INSERT INTO kv2 VALUES ('y', 'two');
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		results, err := d.QueryToMaps(ctx, "SELECT val FROM kv2 WHERE key = ?", "y")
		if err != nil {
			t.Fatalf("QueryToMaps: %v", err)
		}

		if len(results) != 1 {
			t.Fatalf("got %d rows, want 1", len(results))
		}
		if results[0]["val"] != "two" {
			t.Errorf("val = %v, want two", results[0]["val"])
		}
	})

	t.Run("converts byte arrays to strings", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE blobs2 (id INTEGER PRIMARY KEY, data BLOB);
			INSERT INTO blobs2 (data) VALUES (x'48656c6c6f');
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		results, err := d.QueryToMaps(ctx, "SELECT data FROM blobs2")
		if err != nil {
			t.Fatalf("QueryToMaps: %v", err)
		}

		if len(results) != 1 {
			t.Fatalf("got %d rows, want 1", len(results))
		}
		if _, ok := results[0]["data"].(string); !ok {
			t.Errorf("expected string type for blob, got %T", results[0]["data"])
		}
	})

	t.Run("returns error for invalid SQL", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.QueryToMaps(ctx, "SELECT * FROM nonexistent_table_maps")
		if err == nil {
			t.Error("expected error for nonexistent table")
		}
	})

	t.Run("returns empty slice for no rows", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, "CREATE TABLE empty2 (id INTEGER)")
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		results, err := d.QueryToMaps(ctx, "SELECT * FROM empty2")
		if err != nil {
			t.Fatalf("QueryToMaps: %v", err)
		}

		if len(results) != 0 {
			t.Errorf("expected 0 rows, got %d", len(results))
		}
	})
}

func TestCountRows(t *testing.T) {
	t.Parallel()
	t.Run("counts rows correctly", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE counted (id INTEGER PRIMARY KEY);
			INSERT INTO counted VALUES (1);
			INSERT INTO counted VALUES (2);
			INSERT INTO counted VALUES (3);
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		count, err := d.CountRows(ctx, "counted")
		if err != nil {
			t.Fatalf("CountRows: %v", err)
		}

		if count != 3 {
			t.Errorf("count = %d, want 3", count)
		}
	})

	t.Run("returns zero for empty table", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, "CREATE TABLE empty3 (id INTEGER)")
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		count, err := d.CountRows(ctx, "empty3")
		if err != nil {
			t.Fatalf("CountRows: %v", err)
		}

		if count != 0 {
			t.Errorf("count = %d, want 0", count)
		}
	})

	t.Run("rejects SQL injection in table name", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.CountRows(ctx, "users; DROP TABLE users")
		if err == nil {
			t.Error("expected error for injection attempt")
		}
	})

	t.Run("rejects empty table name", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.CountRows(ctx, "")
		if err == nil {
			t.Error("expected error for empty table name")
		}
	})
}

func TestIsValidIdentifier(t *testing.T) {
	t.Parallel()
	tests := []struct {
		name  string
		input string
		want  bool
	}{
		{"simple name", "users", true},
		{"underscore prefix", "_private", true},
		{"with digits", "table2", true},
		{"mixed case", "MyTable", true},
		{"underscore only", "_", true},
		{"empty", "", false},
		{"starts with digit", "1table", false},
		{"has space", "my table", false},
		{"has dash", "my-table", false},
		{"has semicolon", "table;drop", false},
		{"has dot", "schema.table", false},
		{"has parenthesis", "func()", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := isValidIdentifier(tt.input); got != tt.want {
				t.Errorf("isValidIdentifier(%q) = %v, want %v", tt.input, got, tt.want)
			}
		})
	}
}
