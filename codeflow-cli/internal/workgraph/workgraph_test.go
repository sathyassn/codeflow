package workgraph

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/codeflow/codeflow-cli/internal/ledger"

	_ "github.com/ncruces/go-sqlite3/driver"
	_ "github.com/ncruces/go-sqlite3/embed"
)

// fixedTime is used for deterministic timestamps in tests.
var fixedTime = time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)

// newTestService creates a Service backed by a real SQLite DB (with schema) and
// a real ledger writer to a temp directory. Callers get a fully functional
// Service suitable for integration-level tests.
func newTestService(t *testing.T) (*Service, string) {
	t.Helper()

	dbPath := filepath.Join(t.TempDir(), "test.db")
	d, err := db.NewDB(dbPath)
	if err != nil {
		t.Fatalf("newTestService: NewDB: %v", err)
	}
	t.Cleanup(func() { d.Close() })

	ctx := t.Context()
	if err := d.InitFromSchema(ctx); err != nil {
		t.Fatalf("newTestService: InitFromSchema: %v", err)
	}

	ledgerDir := t.TempDir()
	w, err := ledger.NewWriter(ledgerDir)
	if err != nil {
		t.Fatalf("newTestService: NewWriter: %v", err)
	}

	svc := NewService(d, w)
	svc.Now = func() time.Time { return fixedTime }

	return svc, ledgerDir
}

// readLedgerEvents reads all JSONL events from the specified file in the ledger dir.
func readLedgerEvents(t *testing.T, ledgerDir, filename string) ([]map[string]any, error) {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(ledgerDir, filename))
	if err != nil {
		return nil, err
	}
	var events []map[string]any
	for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		if line == "" {
			continue
		}
		var event map[string]any
		if err := json.Unmarshal([]byte(line), &event); err != nil {
			continue
		}
		events = append(events, event)
	}
	return events, nil
}

// --- NewService tests ---

func TestNewService(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	if svc.DB == nil {
		t.Error("Service.DB should not be nil")
	}
	if svc.Writer == nil {
		t.Error("Service.Writer should not be nil")
	}
	if svc.Now == nil {
		t.Error("Service.Now should not be nil")
	}
}

// --- parseInput tests ---

func TestParseInput_ValidJSON(t *testing.T) {
	t.Parallel()

	m, err := parseInput(`{"title": "test"}`)
	if err != nil {
		t.Fatalf("parseInput error: %v", err)
	}
	if m["title"] != "test" {
		t.Errorf("title = %v, want %q", m["title"], "test")
	}
}

func TestParseInput_EmptyString(t *testing.T) {
	t.Parallel()

	// Empty data with no stdin => error.
	// Note: Can't test stdin path in unit tests without mocking os.Stdin.
	// We test the error for empty raw input.
	_, err := parseInput("")
	if err == nil {
		t.Error("expected error for empty parseInput, got nil")
	}
	if !errors.Is(err, ErrInvalidInput) {
		t.Errorf("error = %v, want ErrInvalidInput", err)
	}
}

func TestParseInput_InvalidJSON(t *testing.T) {
	t.Parallel()

	_, err := parseInput("not json")
	if err == nil {
		t.Error("expected error for invalid JSON, got nil")
	}
	if !errors.Is(err, ErrInvalidInput) {
		t.Errorf("error = %v, want ErrInvalidInput", err)
	}
}

// --- helper function tests ---

func TestGetString(t *testing.T) {
	t.Parallel()

	m := map[string]any{"key": "value", "num": 42}
	if got := getString(m, "key"); got != "value" {
		t.Errorf("getString(key) = %q, want %q", got, "value")
	}
	if got := getString(m, "num"); got != "" {
		t.Errorf("getString(num) = %q, want empty", got)
	}
	if got := getString(m, "missing"); got != "" {
		t.Errorf("getString(missing) = %q, want empty", got)
	}
}

func TestGetStringDefault(t *testing.T) {
	t.Parallel()

	m := map[string]any{"key": "value"}
	if got := getStringDefault(m, "key", "default"); got != "value" {
		t.Errorf("getStringDefault(key) = %q, want %q", got, "value")
	}
	if got := getStringDefault(m, "missing", "default"); got != "default" {
		t.Errorf("getStringDefault(missing) = %q, want %q", got, "default")
	}
}

func TestGetBool(t *testing.T) {
	t.Parallel()

	m := map[string]any{"t": true, "f": false, "s": "true", "sf": "false", "n": 42}
	if got := getBool(m, "t"); !got {
		t.Error("getBool(true) should be true")
	}
	if got := getBool(m, "f"); got {
		t.Error("getBool(false) should be false")
	}
	if got := getBool(m, "s"); !got {
		t.Error("getBool(\"true\") should be true")
	}
	if got := getBool(m, "sf"); got {
		t.Error("getBool(\"false\") should be false")
	}
	if got := getBool(m, "n"); got {
		t.Error("getBool(42) should be false")
	}
}

func TestGetBoolDefault(t *testing.T) {
	t.Parallel()

	m := map[string]any{"key": false}
	if got := getBoolDefault(m, "key", true); got {
		t.Error("getBoolDefault(key=false, def=true) should return false")
	}
	if got := getBoolDefault(m, "missing", true); !got {
		t.Error("getBoolDefault(missing, def=true) should return true")
	}
}

func TestGetJSONArray(t *testing.T) {
	t.Parallel()

	m := map[string]any{
		"arr":  []any{"a", "b"},
		"str":  "pre-marshaled",
		"none": nil,
	}
	got := getJSONArray(m, "arr")
	if got != `["a","b"]` {
		t.Errorf("getJSONArray(arr) = %q, want %q", got, `["a","b"]`)
	}
	if s := getJSONArray(m, "str"); s != "pre-marshaled" {
		t.Errorf("getJSONArray(str) = %q, want %q", s, "pre-marshaled")
	}
	if s := getJSONArray(m, "missing"); s != "" {
		t.Errorf("getJSONArray(missing) = %q, want empty", s)
	}
}

func TestRequireString(t *testing.T) {
	t.Parallel()

	m := map[string]any{"key": "value"}
	got, err := requireString(m, "key")
	if err != nil {
		t.Fatalf("requireString error: %v", err)
	}
	if got != "value" {
		t.Errorf("requireString = %q, want %q", got, "value")
	}

	_, err = requireString(m, "missing")
	if err == nil {
		t.Error("expected error for missing required string")
	}
	if !errors.Is(err, ErrInvalidInput) {
		t.Errorf("error = %v, want ErrInvalidInput", err)
	}
}

func TestValidateEnum(t *testing.T) {
	t.Parallel()

	allowed := map[string]bool{"a": true, "b": true}
	if err := validateEnum("field", "a", allowed); err != nil {
		t.Errorf("validateEnum(a) error: %v", err)
	}
	if err := validateEnum("field", "c", allowed); err == nil {
		t.Error("validateEnum(c) expected error")
	} else if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestValidateOptionalEnum(t *testing.T) {
	t.Parallel()

	allowed := map[string]bool{"x": true}

	// Present and valid.
	v, present, err := validateOptionalEnum(map[string]any{"f": "x"}, "f", allowed)
	if err != nil {
		t.Fatalf("validateOptionalEnum error: %v", err)
	}
	if !present || v != "x" {
		t.Errorf("got (%q, %v), want (\"x\", true)", v, present)
	}

	// Absent.
	_, present, err = validateOptionalEnum(map[string]any{}, "f", allowed)
	if err != nil {
		t.Fatalf("validateOptionalEnum error: %v", err)
	}
	if present {
		t.Error("expected present=false for absent field")
	}

	// Present but invalid.
	_, _, err = validateOptionalEnum(map[string]any{"f": "z"}, "f", allowed)
	if err == nil {
		t.Error("expected error for invalid enum value")
	}
}

func TestNullString(t *testing.T) {
	t.Parallel()

	if v := nullString(""); v != nil {
		t.Errorf("nullString(\"\") = %v, want nil", v)
	}
	if v := nullString("hello"); v != "hello" {
		t.Errorf("nullString(\"hello\") = %v, want %q", v, "hello")
	}
}

// --- resolveEpic / resolveTask tests ---

func TestResolveEpic_NotFound(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	_, _, _, err := resolveEpic(ctx, svc.DB, map[string]any{"id": "nonexistent"})
	if err == nil {
		t.Error("expected error for nonexistent epic")
	}
	if !errors.Is(err, ErrNotFound) {
		t.Errorf("error = %v, want ErrNotFound", err)
	}
}

func TestResolveEpic_MissingIdentifier(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	_, _, _, err := resolveEpic(ctx, svc.DB, map[string]any{})
	if err == nil {
		t.Error("expected error for missing identifier")
	}
	if !errors.Is(err, ErrInvalidInput) {
		t.Errorf("error = %v, want ErrInvalidInput", err)
	}
}

func TestResolveTask_NotFound(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	_, _, _, err := resolveTask(ctx, svc.DB, map[string]any{"format_id": "INF-TSK-999-001"})
	if err == nil {
		t.Error("expected error for nonexistent task")
	}
	if !errors.Is(err, ErrNotFound) {
		t.Errorf("error = %v, want ErrNotFound", err)
	}
}

func TestResolveTask_MissingIdentifier(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	_, _, _, err := resolveTask(ctx, svc.DB, map[string]any{})
	if err == nil {
		t.Error("expected error for missing identifier")
	}
	if !errors.Is(err, ErrInvalidInput) {
		t.Errorf("error = %v, want ErrInvalidInput", err)
	}
}

// --- identifier helpers ---

func TestIdentifierLabel(t *testing.T) {
	t.Parallel()

	if got := identifierLabel("abc", ""); got != "id" {
		t.Errorf("identifierLabel with id = %q, want %q", got, "id")
	}
	if got := identifierLabel("", "fmt"); got != "format_id" {
		t.Errorf("identifierLabel with format_id = %q, want %q", got, "format_id")
	}
}

func TestIdentifierValue(t *testing.T) {
	t.Parallel()

	if got := identifierValue("abc", "fmt"); got != "abc" {
		t.Errorf("identifierValue with id = %q, want %q", got, "abc")
	}
	if got := identifierValue("", "fmt"); got != "fmt" {
		t.Errorf("identifierValue with format_id = %q, want %q", got, "fmt")
	}
}

// --- validateRefs tests ---

func TestValidateRefs_Valid(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	// Schema seeds INF, RFCT, GENL.
	if err := validateRefs(ctx, svc.DB, "INF", "RFCT", "GENL"); err != nil {
		t.Errorf("validateRefs for valid refs: %v", err)
	}
}

func TestValidateRefs_InvalidArea(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	err := validateRefs(ctx, svc.DB, "NOPE", "RFCT", "GENL")
	if err == nil {
		t.Error("expected error for invalid area_type")
	}
	if !errors.Is(err, ErrConstraint) {
		t.Errorf("error = %v, want ErrConstraint", err)
	}
}

func TestValidateRefs_InvalidWorkType(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	err := validateRefs(ctx, svc.DB, "INF", "NOPE", "GENL")
	if err == nil {
		t.Error("expected error for invalid work_type")
	}
}

func TestValidateRefs_InvalidDomain(t *testing.T) {
	t.Parallel()

	svc, _ := newTestService(t)
	ctx := t.Context()

	err := validateRefs(ctx, svc.DB, "INF", "RFCT", "NOPE")
	if err == nil {
		t.Error("expected error for invalid domain")
	}
}
