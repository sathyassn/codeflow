// Package workgraph provides atomic CLI commands for epic and task lifecycle
// management. Each operation generates IDs, writes to the database, and appends
// a JSONL event in a single call.
package workgraph

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/codeflow/codeflow-cli/internal/idgen"
	"github.com/codeflow/codeflow-cli/internal/ledger"
)

// Sentinel errors for workgraph operations.
var (
	// ErrInvalidInput indicates the input JSON is malformed or missing required fields.
	ErrInvalidInput = errors.New("workgraph: invalid input")

	// ErrNotFound indicates the requested epic or task does not exist.
	ErrNotFound = errors.New("workgraph: not found")

	// ErrConstraint indicates a CHECK constraint or FK validation failure.
	ErrConstraint = errors.New("workgraph: constraint violation")
)

// Valid CHECK constraint values from schema.sql.
var (
	ValidEpicStatuses = map[string]bool{
		"draft": true, "planning": true, "in_progress": true,
		"blocked": true, "complete": true, "archived": true,
	}
	ValidTaskStatuses = map[string]bool{
		"todo": true, "blocked": true, "in_progress": true,
		"complete": true, "cancelled": true,
	}
	ValidPriorities = map[string]bool{
		"low": true, "normal": true, "high": true, "critical": true,
	}
	ValidOrigins = map[string]bool{
		"planned": true, "informal": true, "auto": true,
	}
	ValidScopePolicies = map[string]bool{
		"soft": true, "hard": true, "permissive": true,
	}
	ValidStages = map[string]bool{
		"dev": true, "plan": true, "docs": true, "test": true,
		"review": true, "qa": true, "done": true,
	}
	ValidStageStatuses = map[string]bool{
		"pending": true, "in_progress": true, "complete": true, "failed": true,
	}
)

// Service holds references to the database and ledger writer required by all
// workgraph operations.
type Service struct {
	DB     *db.DB
	Writer *ledger.Writer
	// Now returns the current time. Override in tests for deterministic output.
	Now func() time.Time
}

// NewService creates a Service with the given DB and ledger writer.
func NewService(d *db.DB, w *ledger.Writer) *Service {
	return &Service{
		DB:     d,
		Writer: w,
		Now:    func() time.Time { return time.Now().UTC() },
	}
}

// parseInput reads JSON from the data string (if non-empty) or from stdin.
func parseInput(data string) (map[string]any, error) {
	var raw []byte
	var err error
	if data != "" {
		raw = []byte(data)
	} else {
		raw, err = io.ReadAll(os.Stdin)
		if err != nil {
			return nil, fmt.Errorf("%w: reading stdin: %w", ErrInvalidInput, err)
		}
	}
	if len(raw) == 0 {
		return nil, fmt.Errorf("%w: empty input", ErrInvalidInput)
	}

	var m map[string]any
	if err := json.Unmarshal(raw, &m); err != nil {
		return nil, fmt.Errorf("%w: %w", ErrInvalidInput, err)
	}
	return m, nil
}

// getString extracts a string value from the input map.
func getString(m map[string]any, key string) string {
	if v, ok := m[key].(string); ok {
		return v
	}
	return ""
}

// getStringDefault extracts a string value with a default fallback.
func getStringDefault(m map[string]any, key, def string) string {
	if v := getString(m, key); v != "" {
		return v
	}
	return def
}

// getBool extracts a boolean value from the input map.
func getBool(m map[string]any, key string) bool {
	switch v := m[key].(type) {
	case bool:
		return v
	case string:
		return v == "true"
	default:
		return false
	}
}

// getBoolDefault extracts a boolean value with a default fallback.
func getBoolDefault(m map[string]any, key string, def bool) bool {
	if _, ok := m[key]; !ok {
		return def
	}
	return getBool(m, key)
}

// getJSONArray extracts a value and marshals it to a JSON string for storage.
func getJSONArray(m map[string]any, key string) string {
	v, ok := m[key]
	if !ok {
		return ""
	}
	if s, ok := v.(string); ok {
		return s
	}
	b, err := json.Marshal(v)
	if err != nil {
		return ""
	}
	return string(b)
}

// requireString checks that a required string field is present and non-empty.
func requireString(m map[string]any, key string) (string, error) {
	v := getString(m, key)
	if v == "" {
		return "", fmt.Errorf("%w: missing required field %q", ErrInvalidInput, key)
	}
	return v, nil
}

// validateEnum checks that a value is one of the allowed values.
func validateEnum(field, value string, allowed map[string]bool) error {
	if !allowed[value] {
		return fmt.Errorf("%w: invalid %s %q", ErrConstraint, field, value)
	}
	return nil
}

// validateOptionalEnum checks an optional value against allowed values.
// Returns true if the field was present, false if absent.
func validateOptionalEnum(m map[string]any, field string, allowed map[string]bool) (string, bool, error) {
	v := getString(m, field)
	if v == "" {
		return "", false, nil
	}
	if err := validateEnum(field, v, allowed); err != nil {
		return "", true, err
	}
	return v, true, nil
}

// validateRefTable checks that a value exists in a reference table.
func validateRefTable(ctx context.Context, d *db.DB, table, column, value string) error {
	var count int
	err := d.QueryRow(ctx, fmt.Sprintf("SELECT COUNT(*) FROM %s WHERE code = ?", table), value).Scan(&count) //nolint:gosec // table is hardcoded by callers
	if err != nil {
		return fmt.Errorf("workgraph: checking %s reference: %w", column, err)
	}
	if count == 0 {
		return fmt.Errorf("%w: %s %q not found in %s", ErrConstraint, column, value, table)
	}
	return nil
}

// validateRefs validates area_type, work_type, and domain against reference tables.
func validateRefs(ctx context.Context, d *db.DB, areaType, workType, domain string) error {
	if err := validateRefTable(ctx, d, "area_types", "area_type", areaType); err != nil {
		return err
	}
	if err := validateRefTable(ctx, d, "work_types", "work_type", workType); err != nil {
		return err
	}
	return validateRefTable(ctx, d, "domains", "domain", domain)
}

// resolveEpic finds an epic by id or format_id. Returns the ULID id, format_id,
// and current status.
func resolveEpic(ctx context.Context, d *db.DB, input map[string]any) (string, string, string, error) {
	id := getString(input, "id")
	formatID := getString(input, "format_id")
	if id == "" && formatID == "" {
		return "", "", "", fmt.Errorf("%w: must provide id or format_id", ErrInvalidInput)
	}

	var query string
	var arg string
	if id != "" {
		query = "SELECT id, format_id, status FROM epics WHERE id = ?"
		arg = id
	} else {
		query = "SELECT id, format_id, status FROM epics WHERE format_id = ?"
		arg = formatID
	}

	var epicID, epicFormatID, epicStatus string
	err := d.QueryRow(ctx, query, arg).Scan(&epicID, &epicFormatID, &epicStatus)
	if errors.Is(err, sql.ErrNoRows) {
		return "", "", "", fmt.Errorf("%w: epic with %s=%q", ErrNotFound, identifierLabel(id, formatID), identifierValue(id, formatID))
	}
	if err != nil {
		return "", "", "", fmt.Errorf("workgraph: resolving epic: %w", err)
	}
	return epicID, epicFormatID, epicStatus, nil
}

// resolveTask finds a task by id or format_id. Returns the ULID id, format_id,
// and current status.
func resolveTask(ctx context.Context, d *db.DB, input map[string]any) (string, string, string, error) {
	id := getString(input, "id")
	formatID := getString(input, "format_id")
	if id == "" && formatID == "" {
		return "", "", "", fmt.Errorf("%w: must provide id or format_id", ErrInvalidInput)
	}

	var query string
	var arg string
	if id != "" {
		query = "SELECT id, format_id, status FROM tasks WHERE id = ?"
		arg = id
	} else {
		query = "SELECT id, format_id, status FROM tasks WHERE format_id = ?"
		arg = formatID
	}

	var taskID, taskFormatID, taskStatus string
	err := d.QueryRow(ctx, query, arg).Scan(&taskID, &taskFormatID, &taskStatus)
	if errors.Is(err, sql.ErrNoRows) {
		return "", "", "", fmt.Errorf("%w: task with %s=%q", ErrNotFound, identifierLabel(id, formatID), identifierValue(id, formatID))
	}
	if err != nil {
		return "", "", "", fmt.Errorf("workgraph: resolving task: %w", err)
	}
	return taskID, taskFormatID, taskStatus, nil
}

func identifierLabel(id, formatID string) string {
	if id != "" {
		return "id"
	}
	return "format_id"
}

func identifierValue(id, formatID string) string {
	if id != "" {
		return id
	}
	return formatID
}

// newQuerier wraps the DB's QueryRow into an idgen.Querier.
func newQuerier(d *db.DB) idgen.Querier {
	return idgen.QueryRowFunc(d.QueryRow)
}
