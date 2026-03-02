package idgen

import (
	"database/sql"
	"testing"

	_ "github.com/ncruces/go-sqlite3/driver"
	_ "github.com/ncruces/go-sqlite3/embed"
)

// openTestDB creates a file-backed SQLite database in t.TempDir with the
// required tables. Using a temp file avoids in-memory connection isolation
// issues when multiple goroutines query the same database.
func openTestDB(t *testing.T) *sql.DB {
	t.Helper()
	dbPath := t.TempDir() + "/test.db"
	db, err := sql.Open("sqlite3", dbPath)
	if err != nil {
		t.Fatalf("opening test db: %v", err)
	}
	// Single connection avoids SQLITE_BUSY in parallel subtests.
	db.SetMaxOpenConns(1)
	t.Cleanup(func() { db.Close() })

	// Create minimal tables matching the CodeFlow schema.
	_, err = db.Exec(`
		CREATE TABLE epics (
			id TEXT PRIMARY KEY,
			format_id TEXT NOT NULL,
			area_type TEXT NOT NULL
		);
		CREATE TABLE tasks (
			id TEXT PRIMARY KEY,
			format_id TEXT NOT NULL,
			area_type TEXT NOT NULL
		);
	`)
	if err != nil {
		t.Fatalf("creating test tables: %v", err)
	}
	return db
}

func TestNextFormatID_EmptyTable(t *testing.T) {
	t.Parallel()

	db := openTestDB(t)
	ctx := t.Context()

	tests := []struct {
		name    string
		table   string
		area    string
		wantSeq int
	}{
		{name: "empty epics table", table: "epics", area: "INF", wantSeq: 1},
		{name: "empty tasks table", table: "tasks", area: "INF", wantSeq: 1},
		{name: "empty tasks different area", table: "tasks", area: "CLI", wantSeq: 1},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			seq, err := NextFormatID(ctx, db, tt.table, tt.area)
			if err != nil {
				t.Fatalf("NextFormatID(%q, %q) error: %v", tt.table, tt.area, err)
			}
			if seq != tt.wantSeq {
				t.Errorf("NextFormatID(%q, %q) = %d, want %d",
					tt.table, tt.area, seq, tt.wantSeq)
			}
		})
	}
}

func TestNextFormatID_ExistingIDs(t *testing.T) {
	t.Parallel()

	db := openTestDB(t)
	ctx := t.Context()

	// Insert some existing epics.
	for _, epic := range []struct {
		id       string
		formatID string
		area     string
	}{
		{"e1", "INF-EPC-001", "INF"},
		{"e2", "INF-EPC-002", "INF"},
		{"e3", "INF-EPC-021", "INF"},
		{"e4", "CLI-EPC-001", "CLI"},
	} {
		_, err := db.ExecContext(ctx,
			"INSERT INTO epics (id, format_id, area_type) VALUES (?, ?, ?)",
			epic.id, epic.formatID, epic.area)
		if err != nil {
			t.Fatalf("inserting epic: %v", err)
		}
	}

	tests := []struct {
		name    string
		table   string
		area    string
		wantSeq int
	}{
		{name: "INF epics next after 021", table: "epics", area: "INF", wantSeq: 22},
		{name: "CLI epics next after 001", table: "epics", area: "CLI", wantSeq: 2},
		{name: "nonexistent area", table: "epics", area: "XXX", wantSeq: 1},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			seq, err := NextFormatID(ctx, db, tt.table, tt.area)
			if err != nil {
				t.Fatalf("NextFormatID(%q, %q) error: %v", tt.table, tt.area, err)
			}
			if seq != tt.wantSeq {
				t.Errorf("NextFormatID(%q, %q) = %d, want %d",
					tt.table, tt.area, seq, tt.wantSeq)
			}
		})
	}
}

func TestNextFormatID_TasksWithGaps(t *testing.T) {
	t.Parallel()

	db := openTestDB(t)
	ctx := t.Context()

	// Insert tasks with gaps in sequence numbers.
	for _, task := range []struct {
		id       string
		formatID string
		area     string
	}{
		{"t1", "INF-TSK-021-001", "INF"},
		{"t2", "INF-TSK-021-003", "INF"}, // gap: 002 is missing
		{"t3", "INF-TSK-021-005", "INF"}, // gap: 004 is missing
	} {
		_, err := db.ExecContext(ctx,
			"INSERT INTO tasks (id, format_id, area_type) VALUES (?, ?, ?)",
			task.id, task.formatID, task.area)
		if err != nil {
			t.Fatalf("inserting task: %v", err)
		}
	}

	// Should return max+1 = 6, not fill the gap.
	seq, err := NextFormatID(ctx, db, "tasks", "INF")
	if err != nil {
		t.Fatalf("NextFormatID error: %v", err)
	}
	if seq != 6 {
		t.Errorf("NextFormatID for tasks with gaps = %d, want 6", seq)
	}
}

func TestNextFormatID_InvalidTable(t *testing.T) {
	t.Parallel()

	db := openTestDB(t)
	ctx := t.Context()

	tests := []struct {
		name  string
		table string
	}{
		{name: "unknown table", table: "users"},
		{name: "SQL injection attempt", table: "tasks; DROP TABLE tasks--"},
		{name: "empty table name", table: ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			_, err := NextFormatID(ctx, db, tt.table, "INF")
			if err == nil {
				t.Errorf("NextFormatID(%q, %q) expected error, got nil", tt.table, "INF")
			}
		})
	}
}

func TestNextFormatID_EmptyArea(t *testing.T) {
	t.Parallel()

	db := openTestDB(t)
	ctx := t.Context()

	_, err := NextFormatID(ctx, db, "epics", "")
	if err == nil {
		t.Error("NextFormatID with empty area should return error")
	}
}

// ---- QueryRowFunc Adapter Tests ----

func TestQueryRowFunc_SatisfiesQuerier(t *testing.T) {
	t.Parallel()

	db := openTestDB(t)
	ctx := t.Context()

	// Wrap *sql.DB.QueryRowContext as a QueryRowFunc to exercise lines 40-42.
	qrf := QueryRowFunc(db.QueryRowContext)

	seq, err := NextFormatID(ctx, qrf, "epics", "INF")
	if err != nil {
		t.Fatalf("NextFormatID via QueryRowFunc error: %v", err)
	}
	// Empty table should return 1.
	if seq != 1 {
		t.Errorf("NextFormatID via QueryRowFunc = %d, want 1", seq)
	}
}

func TestNextFormatID_ScanError(t *testing.T) {
	t.Parallel()

	db := openTestDB(t)
	ctx := t.Context()

	// Close the database to force a scan error on the next query.
	db.Close()

	_, err := NextFormatID(ctx, db, "epics", "INF")
	if err == nil {
		t.Fatal("NextFormatID on closed DB should return error")
	}
}

// ---- ParseEpicSeq Tests ----

func TestParseEpicSeq(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		input   string
		want    int
		wantErr bool
	}{
		{name: "standard", input: "INF-EPC-021", want: 21},
		{name: "single digit", input: "CLI-EPC-001", want: 1},
		{name: "three digits", input: "INF-EPC-100", want: 100},
		{name: "large number", input: "INF-EPC-999", want: 999},
		{name: "wrong segment count", input: "INF-EPC", wantErr: true},
		{name: "not EPC", input: "INF-TSK-021", wantErr: true},
		{name: "empty area", input: "-EPC-021", wantErr: true},
		{name: "non-numeric seq", input: "INF-EPC-abc", wantErr: true},
		{name: "zero seq", input: "INF-EPC-000", wantErr: true},
		{name: "negative seq", input: "INF-EPC--01", wantErr: true},
		{name: "too many segments", input: "INF-EPC-021-001", wantErr: true},
		{name: "empty string", input: "", wantErr: true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got, err := ParseEpicSeq(tt.input)
			if tt.wantErr {
				if err == nil {
					t.Errorf("ParseEpicSeq(%q) expected error, got %d", tt.input, got)
				}
				return
			}
			if err != nil {
				t.Fatalf("ParseEpicSeq(%q) error: %v", tt.input, err)
			}
			if got != tt.want {
				t.Errorf("ParseEpicSeq(%q) = %d, want %d", tt.input, got, tt.want)
			}
		})
	}
}

// ---- Format Helper Tests ----

func TestFormatEpicID(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		area string
		seq  int
		want string
	}{
		{name: "standard", area: "INF", seq: 21, want: "INF-EPC-021"},
		{name: "single digit", area: "CLI", seq: 1, want: "CLI-EPC-001"},
		{name: "three digits", area: "INF", seq: 100, want: "INF-EPC-100"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := FormatEpicID(tt.area, tt.seq)
			if got != tt.want {
				t.Errorf("FormatEpicID(%q, %d) = %q, want %q", tt.area, tt.seq, got, tt.want)
			}
		})
	}
}

func TestFormatTaskID(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		area    string
		epicSeq int
		taskSeq int
		want    string
	}{
		{name: "standard", area: "INF", epicSeq: 21, taskSeq: 1, want: "INF-TSK-021-001"},
		{name: "larger sequence", area: "INF", epicSeq: 21, taskSeq: 15, want: "INF-TSK-021-015"},
		{name: "different area", area: "CLI", epicSeq: 1, taskSeq: 3, want: "CLI-TSK-001-003"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := FormatTaskID(tt.area, tt.epicSeq, tt.taskSeq)
			if got != tt.want {
				t.Errorf("FormatTaskID(%q, %d, %d) = %q, want %q",
					tt.area, tt.epicSeq, tt.taskSeq, got, tt.want)
			}
		})
	}
}
