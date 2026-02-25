package db

import (
	"testing"
)

func TestExecWithResult(t *testing.T) {
	t.Run("returns rows affected and last insert ID", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, "CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT)")
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		result, err := d.ExecWithResult(ctx, "INSERT INTO items (name) VALUES (?)", "test")
		if err != nil {
			t.Fatalf("ExecWithResult: %v", err)
		}

		if result.RowsAffected != 1 {
			t.Errorf("RowsAffected = %d, want 1", result.RowsAffected)
		}
		if result.LastInsertID != 1 {
			t.Errorf("LastInsertID = %d, want 1", result.LastInsertID)
		}
	})

	t.Run("update returns correct rows affected", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE vals (id INTEGER PRIMARY KEY, v TEXT);
			INSERT INTO vals VALUES (1, 'a');
			INSERT INTO vals VALUES (2, 'a');
			INSERT INTO vals VALUES (3, 'b');
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		result, err := d.ExecWithResult(ctx, "UPDATE vals SET v = 'x' WHERE v = 'a'")
		if err != nil {
			t.Fatalf("ExecWithResult: %v", err)
		}

		if result.RowsAffected != 2 {
			t.Errorf("RowsAffected = %d, want 2", result.RowsAffected)
		}
	})

	t.Run("returns error for invalid SQL", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.ExecWithResult(ctx, "INSERT INTO nonexistent VALUES (1)")
		if err == nil {
			t.Error("expected error for nonexistent table")
		}
	})

	t.Run("delete returns correct rows affected", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE del_test (id INTEGER PRIMARY KEY);
			INSERT INTO del_test VALUES (1);
			INSERT INTO del_test VALUES (2);
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		result, err := d.ExecWithResult(ctx, "DELETE FROM del_test WHERE id = 1")
		if err != nil {
			t.Fatalf("ExecWithResult: %v", err)
		}

		if result.RowsAffected != 1 {
			t.Errorf("RowsAffected = %d, want 1", result.RowsAffected)
		}
	})

	t.Run("no-op returns zero rows affected", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, "CREATE TABLE noop (id INTEGER PRIMARY KEY)")
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		result, err := d.ExecWithResult(ctx, "DELETE FROM noop WHERE id = 999")
		if err != nil {
			t.Fatalf("ExecWithResult: %v", err)
		}

		if result.RowsAffected != 0 {
			t.Errorf("RowsAffected = %d, want 0", result.RowsAffected)
		}
	})
}

func TestBatchExec(t *testing.T) {
	t.Run("executes multiple statements in transaction", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, "CREATE TABLE batch (id INTEGER PRIMARY KEY, name TEXT)")
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		result, err := d.BatchExec(ctx, []Statement{
			{Query: "INSERT INTO batch (id, name) VALUES (?, ?)", Args: []any{1, "first"}},
			{Query: "INSERT INTO batch (id, name) VALUES (?, ?)", Args: []any{2, "second"}},
			{Query: "INSERT INTO batch (id, name) VALUES (?, ?)", Args: []any{3, "third"}},
		})
		if err != nil {
			t.Fatalf("BatchExec: %v", err)
		}

		if result.StatementCount != 3 {
			t.Errorf("StatementCount = %d, want 3", result.StatementCount)
		}
		if result.TotalAffected != 3 {
			t.Errorf("TotalAffected = %d, want 3", result.TotalAffected)
		}

		// Verify rows actually inserted.
		count, err := d.CountRows(ctx, "batch")
		if err != nil {
			t.Fatalf("CountRows: %v", err)
		}
		if count != 3 {
			t.Errorf("row count = %d, want 3", count)
		}
	})

	t.Run("rolls back on error", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, "CREATE TABLE batch2 (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		_, err = d.BatchExec(ctx, []Statement{
			{Query: "INSERT INTO batch2 (id, name) VALUES (?, ?)", Args: []any{1, "ok"}},
			{Query: "INSERT INTO batch2 (id, name) VALUES (?, ?)", Args: []any{2, nil}}, // NULL name violates NOT NULL
		})
		if err == nil {
			t.Fatal("expected error for NULL constraint violation")
		}

		// The first row should have been rolled back.
		count, err := d.CountRows(ctx, "batch2")
		if err != nil {
			t.Fatalf("CountRows: %v", err)
		}
		if count != 0 {
			t.Errorf("row count = %d after rollback, want 0", count)
		}
	})

	t.Run("empty statements returns empty result", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		result, err := d.BatchExec(ctx, []Statement{})
		if err != nil {
			t.Fatalf("BatchExec: %v", err)
		}

		if result.StatementCount != 0 {
			t.Errorf("StatementCount = %d, want 0", result.StatementCount)
		}
		if result.TotalAffected != 0 {
			t.Errorf("TotalAffected = %d, want 0", result.TotalAffected)
		}
	})

	t.Run("mixed insert and update", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, `
			CREATE TABLE batch3 (id INTEGER PRIMARY KEY, status TEXT);
			INSERT INTO batch3 VALUES (1, 'old');
		`)
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		result, err := d.BatchExec(ctx, []Statement{
			{Query: "INSERT INTO batch3 (id, status) VALUES (?, ?)", Args: []any{2, "new"}},
			{Query: "UPDATE batch3 SET status = ? WHERE id = ?", Args: []any{"updated", 1}},
		})
		if err != nil {
			t.Fatalf("BatchExec: %v", err)
		}

		if result.StatementCount != 2 {
			t.Errorf("StatementCount = %d, want 2", result.StatementCount)
		}
		if result.TotalAffected != 2 {
			t.Errorf("TotalAffected = %d, want 2", result.TotalAffected)
		}
	})

	t.Run("invalid SQL in batch", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.BatchExec(ctx, []Statement{
			{Query: "INSERT INTO does_not_exist VALUES (1)"},
		})
		if err == nil {
			t.Error("expected error for invalid table")
		}
	})
}

func TestInsertRow(t *testing.T) {
	t.Run("inserts single row", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.db.ExecContext(ctx, "CREATE TABLE ins (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, val INTEGER)")
		if err != nil {
			t.Fatalf("setup: %v", err)
		}

		result, err := d.InsertRow(ctx, "ins", []string{"name", "val"}, []any{"test", 42})
		if err != nil {
			t.Fatalf("InsertRow: %v", err)
		}

		if result.RowsAffected != 1 {
			t.Errorf("RowsAffected = %d, want 1", result.RowsAffected)
		}
		if result.LastInsertID != 1 {
			t.Errorf("LastInsertID = %d, want 1", result.LastInsertID)
		}

		// Verify data.
		var name string
		var val int
		if err := d.QueryRow(ctx, "SELECT name, val FROM ins WHERE id = 1").Scan(&name, &val); err != nil {
			t.Fatalf("verify: %v", err)
		}
		if name != "test" || val != 42 {
			t.Errorf("got (%q, %d), want (test, 42)", name, val)
		}
	})

	t.Run("rejects invalid table name", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.InsertRow(ctx, "bad;table", []string{"col"}, []any{"val"})
		if err == nil {
			t.Error("expected error for invalid table name")
		}
	})

	t.Run("rejects invalid column name", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.InsertRow(ctx, "test_tbl", []string{"bad col"}, []any{"val"})
		if err == nil {
			t.Error("expected error for invalid column name")
		}
	})

	t.Run("rejects mismatched column and value counts", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.InsertRow(ctx, "test_tbl", []string{"a", "b"}, []any{"one"})
		if err == nil {
			t.Error("expected error for mismatched counts")
		}
	})

	t.Run("rejects empty columns", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		_, err := d.InsertRow(ctx, "test_tbl", []string{}, []any{})
		if err == nil {
			t.Error("expected error for empty columns")
		}
	})
}
