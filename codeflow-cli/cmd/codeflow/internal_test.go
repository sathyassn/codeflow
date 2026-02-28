package main

import (
	"regexp"
	"strings"
	"testing"

	"path/filepath"
)

// ulidOutputPattern matches a line containing a 26-character lowercase Crockford base32 ULID.
var ulidOutputPattern = regexp.MustCompile(`^[0-9a-z]{26}\n$`)

// prefixedUlidOutputPattern matches {prefix}-{26-char ULID}\n.
var prefixedUlidOutputPattern = regexp.MustCompile(`^[a-z]+-[0-9a-z]{26}\n$`)

// ---- newInternalCmd tests ----

func TestNewInternalCmd(t *testing.T) {
	t.Parallel()

	cmd := newInternalCmd()
	if cmd.Use != "internal" {
		t.Errorf("Use = %q, want %q", cmd.Use, "internal")
	}
	if !cmd.Hidden {
		t.Error("internal command should be hidden")
	}
	if cmd.Short == "" {
		t.Error("internal command should have a short description")
	}
}

func TestInternalCmd_Subcommands(t *testing.T) {
	t.Parallel()

	cmd := newInternalCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Name()] = true
	}

	if !subcommands["ulid"] {
		t.Error("expected subcommand 'ulid' to be registered on internal cmd")
	}
}

func TestInternalCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	found := false
	for _, sub := range cmd.Commands() {
		if sub.Name() == "internal" {
			found = true
			if !sub.Hidden {
				t.Error("internal command should be hidden in root cmd")
			}
			break
		}
	}
	if !found {
		t.Error("expected 'internal' subcommand to be registered in root cmd")
	}
}

func TestInternalCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"internal", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("internal --help returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "ulid") {
		t.Errorf("internal help missing %q:\n%s", "ulid", got)
	}
}

// ---- newInternalUlidCmd tests ----

func TestNewInternalUlidCmd(t *testing.T) {
	t.Parallel()

	cmd := newInternalUlidCmd()
	if cmd.Use != "ulid" {
		t.Errorf("Use = %q, want %q", cmd.Use, "ulid")
	}
	if cmd.Short == "" {
		t.Error("ulid command should have a short description")
	}
}

func TestInternalUlidCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newInternalUlidCmd()
	prefixFlag := cmd.Flags().Lookup("prefix")
	if prefixFlag == nil {
		t.Fatal("internal ulid should have --prefix flag")
	}
	if prefixFlag.DefValue != "" {
		t.Errorf("--prefix default = %q, want empty", prefixFlag.DefValue)
	}
}

// ---- runInternalUlid tests ----

func TestRunInternalUlid_BareULID(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runInternalUlid(&buf, "")
	if err != nil {
		t.Fatalf("runInternalUlid returned error: %v", err)
	}

	got := buf.String()
	if !ulidOutputPattern.MatchString(got) {
		t.Errorf("runInternalUlid() = %q, does not match bare ULID pattern", got)
	}
}

func TestRunInternalUlid_WithPrefix(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name   string
		prefix string
		want   string // expected prefix in output
	}{
		{name: "task prefix", prefix: "task", want: "task-"},
		{name: "epic prefix", prefix: "epic", want: "epic-"},
		{name: "ses prefix", prefix: "ses", want: "ses-"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			var buf strings.Builder
			err := runInternalUlid(&buf, tt.prefix)
			if err != nil {
				t.Fatalf("runInternalUlid returned error: %v", err)
			}

			got := strings.TrimSpace(buf.String())
			if !strings.HasPrefix(got, tt.want) {
				t.Errorf("runInternalUlid(%q) = %q, want prefix %q", tt.prefix, got, tt.want)
			}
			// Verify the ULID portion after prefix.
			ulidPart := got[len(tt.want):]
			if len(ulidPart) != 26 {
				t.Errorf("ULID portion length = %d, want 26; got %q", len(ulidPart), ulidPart)
			}
		})
	}
}

func TestRunInternalUlid_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"internal", "ulid"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("internal ulid via root cmd returned error: %v", err)
	}

	got := buf.String()
	if !ulidOutputPattern.MatchString(got) {
		t.Errorf("internal ulid output = %q, does not match ULID pattern", got)
	}
}

func TestRunInternalUlid_ViaRootCmd_WithPrefix(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"internal", "ulid", "--prefix", "task"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("internal ulid --prefix via root cmd returned error: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	if !strings.HasPrefix(got, "task-") {
		t.Errorf("internal ulid --prefix output = %q, want prefix %q", got, "task-")
	}
}

func TestRunInternalUlid_Uniqueness(t *testing.T) {
	t.Parallel()

	const count = 50
	seen := make(map[string]bool, count)
	for range count {
		var buf strings.Builder
		if err := runInternalUlid(&buf, ""); err != nil {
			t.Fatalf("runInternalUlid error: %v", err)
		}
		id := strings.TrimSpace(buf.String())
		if seen[id] {
			t.Fatalf("duplicate ULID: %q", id)
		}
		seen[id] = true
	}
}

// ---- newDBGenerateIdCmd tests ----

func TestNewDBGenerateIdCmd(t *testing.T) {
	t.Parallel()

	cmd := newDBGenerateIdCmd()
	if cmd.Use != "generate-id" {
		t.Errorf("Use = %q, want %q", cmd.Use, "generate-id")
	}
	if cmd.Short == "" {
		t.Error("generate-id command should have a short description")
	}
}

func TestDBGenerateIdCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newDBGenerateIdCmd()

	dbFlag := cmd.Flags().Lookup("db")
	if dbFlag == nil {
		t.Fatal("db generate-id should have --db flag")
	}
	if dbFlag.DefValue != defaultDBPath() {
		t.Errorf("--db default = %q, want %q", dbFlag.DefValue, defaultDBPath())
	}

	tableFlag := cmd.Flags().Lookup("table")
	if tableFlag == nil {
		t.Fatal("db generate-id should have --table flag")
	}

	areaFlag := cmd.Flags().Lookup("area")
	if areaFlag == nil {
		t.Fatal("db generate-id should have --area flag")
	}
}

func TestDBGenerateIdCmd_RequiresTableFlag(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"db", "generate-id", "--area", "INF"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error when --table is not provided")
	}
}

func TestDBGenerateIdCmd_RequiresAreaFlag(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetErr(&buf)
	cmd.SetArgs([]string{"db", "generate-id", "--table", "epics"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error when --area is not provided")
	}
}

// ---- runDBGenerateId tests ----

func TestRunDBGenerateId_EmptyEpics(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	var buf strings.Builder
	err := runDBGenerateId(&buf, dbPath, "epics", "INF")
	if err != nil {
		t.Fatalf("runDBGenerateId returned error: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	// Empty table → seq 1 → FormatEpicID("INF", 1) = "INF-EPC-001"
	if got != "INF-EPC-001" {
		t.Errorf("runDBGenerateId(epics, INF) = %q, want %q", got, "INF-EPC-001")
	}
}

func TestRunDBGenerateId_EmptyTasks(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	var buf strings.Builder
	err := runDBGenerateId(&buf, dbPath, "tasks", "INF")
	if err != nil {
		t.Fatalf("runDBGenerateId returned error: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	// Empty table → seq 1 → formatted as "001"
	if got != "001" {
		t.Errorf("runDBGenerateId(tasks, INF) = %q, want %q", got, "001")
	}
}

// seedReferenceData inserts required reference rows so FK constraints pass.
func seedReferenceData(t *testing.T, dbPath string) {
	t.Helper()
	for _, stmt := range []struct {
		sql  string
		args string
	}{
		{sql: "INSERT OR IGNORE INTO area_types (code, name) VALUES (?, ?)", args: `["INF", "Infrastructure"]`},
		{sql: "INSERT OR IGNORE INTO area_types (code, name) VALUES (?, ?)", args: `["CLI", "CLI"]`},
		{sql: "INSERT OR IGNORE INTO work_types (code, name, branch_prefix) VALUES (?, ?, ?)", args: `["FEAT", "Feature", "feat/"]`},
		{sql: "INSERT OR IGNORE INTO work_types (code, name, branch_prefix) VALUES (?, ?, ?)", args: `["RFCT", "Refactor", "refactor/"]`},
		{sql: "INSERT OR IGNORE INTO domains (code, name) VALUES (?, ?)", args: `["GENL", "General"]`},
	} {
		var buf strings.Builder
		if err := runDBExec(&buf, dbPath, stmt.sql, stmt.args); err != nil {
			t.Fatalf("seeding reference data: %v", err)
		}
	}
}

func TestRunDBGenerateId_EpicsWithExistingData(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	seedReferenceData(t, dbPath)

	// Insert existing epics.
	for _, stmt := range []struct {
		sql  string
		args string
	}{
		{
			sql:  "INSERT INTO epics (id, format_id, title, area_type, work_type, domain) VALUES (?, ?, ?, ?, ?, ?)",
			args: `["e1", "INF-EPC-001", "Epic 1", "INF", "FEAT", "GENL"]`,
		},
		{
			sql:  "INSERT INTO epics (id, format_id, title, area_type, work_type, domain) VALUES (?, ?, ?, ?, ?, ?)",
			args: `["e2", "INF-EPC-021", "Epic 21", "INF", "RFCT", "GENL"]`,
		},
	} {
		var execBuf strings.Builder
		if err := runDBExec(&execBuf, dbPath, stmt.sql, stmt.args); err != nil {
			t.Fatalf("inserting epic: %v", err)
		}
	}

	var buf strings.Builder
	err := runDBGenerateId(&buf, dbPath, "epics", "INF")
	if err != nil {
		t.Fatalf("runDBGenerateId returned error: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	// Max existing is 021 → next is 022
	if got != "INF-EPC-022" {
		t.Errorf("runDBGenerateId(epics, INF) = %q, want %q", got, "INF-EPC-022")
	}
}

func TestRunDBGenerateId_TasksWithExistingData(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	seedReferenceData(t, dbPath)

	// Insert a parent epic first (tasks FK to epics).
	{
		var buf strings.Builder
		if err := runDBExec(&buf, dbPath,
			"INSERT INTO epics (id, format_id, title, area_type, work_type, domain) VALUES (?, ?, ?, ?, ?, ?)",
			`["e1", "INF-EPC-021", "Epic 21", "INF", "RFCT", "GENL"]`); err != nil {
			t.Fatalf("inserting epic: %v", err)
		}
	}

	// Insert existing tasks.
	for _, stmt := range []struct {
		sql  string
		args string
	}{
		{
			sql:  "INSERT INTO tasks (id, epic_id, format_id, title, area_type, work_type, domain) VALUES (?, ?, ?, ?, ?, ?, ?)",
			args: `["t1", "e1", "INF-TSK-021-001", "Task 1", "INF", "RFCT", "GENL"]`,
		},
		{
			sql:  "INSERT INTO tasks (id, epic_id, format_id, title, area_type, work_type, domain) VALUES (?, ?, ?, ?, ?, ?, ?)",
			args: `["t2", "e1", "INF-TSK-021-005", "Task 5", "INF", "RFCT", "GENL"]`,
		},
	} {
		var execBuf strings.Builder
		if err := runDBExec(&execBuf, dbPath, stmt.sql, stmt.args); err != nil {
			t.Fatalf("inserting task: %v", err)
		}
	}

	var buf strings.Builder
	err := runDBGenerateId(&buf, dbPath, "tasks", "INF")
	if err != nil {
		t.Fatalf("runDBGenerateId returned error: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	// Max existing is 005 → next is 006
	if got != "006" {
		t.Errorf("runDBGenerateId(tasks, INF) = %q, want %q", got, "006")
	}
}

func TestRunDBGenerateId_InvalidDBPath(t *testing.T) {
	t.Parallel()

	var buf strings.Builder
	err := runDBGenerateId(&buf, "/nonexistent/path/db.db", "epics", "INF")
	if err == nil {
		t.Error("expected error for nonexistent DB path")
	}
}

func TestRunDBGenerateId_InvalidTable(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	var buf strings.Builder
	err := runDBGenerateId(&buf, dbPath, "invalid_table", "INF")
	if err == nil {
		t.Error("expected error for invalid table name")
	}
}

func TestRunDBGenerateId_EmptyArea(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	var buf strings.Builder
	err := runDBGenerateId(&buf, dbPath, "epics", "")
	if err == nil {
		t.Error("expected error for empty area")
	}
}

func TestRunDBGenerateId_ViaRootCmd_Epics(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "generate-id", "--db", dbPath, "--table", "epics", "--area", "CLI"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db generate-id via root cmd returned error: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	if got != "CLI-EPC-001" {
		t.Errorf("db generate-id output = %q, want %q", got, "CLI-EPC-001")
	}
}

func TestRunDBGenerateId_ViaRootCmd_Tasks(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	cmd := newRootCmd()
	var buf strings.Builder
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"db", "generate-id", "--db", dbPath, "--table", "tasks", "--area", "INF"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("db generate-id via root cmd returned error: %v", err)
	}

	got := strings.TrimSpace(buf.String())
	if got != "001" {
		t.Errorf("db generate-id output = %q, want %q", got, "001")
	}
}

func TestRunDBGenerateId_ExitErrorCodes(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		dbPath   string
		table    string
		area     string
		wantCode int
	}{
		{
			name:     "config error for bad db path",
			dbPath:   "/nonexistent/path/db.db",
			table:    "epics",
			area:     "INF",
			wantCode: ExitConfigError,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			var buf strings.Builder
			err := runDBGenerateId(&buf, tt.dbPath, tt.table, tt.area)
			if err == nil {
				t.Fatal("expected error")
			}
			gotCode := exitCode(err)
			if gotCode != tt.wantCode {
				t.Errorf("exitCode = %d, want %d", gotCode, tt.wantCode)
			}
		})
	}
}

func TestRunDBGenerateId_RuntimeErrorForInvalidTable(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "test.db")

	var initBuf strings.Builder
	if err := runDBInit(&initBuf, dbPath); err != nil {
		t.Fatalf("runDBInit: %v", err)
	}

	var buf strings.Builder
	err := runDBGenerateId(&buf, dbPath, "nonexistent", "INF")
	if err == nil {
		t.Fatal("expected error for invalid table")
	}
	gotCode := exitCode(err)
	if gotCode != ExitRuntimeError {
		t.Errorf("exitCode = %d, want %d (ExitRuntimeError)", gotCode, ExitRuntimeError)
	}
}
