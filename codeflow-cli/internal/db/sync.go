package db

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
)

// Canonical JSONL file names.
const (
	FileWorkGraph    = "work-graph.jsonl"
	FileMemoryEvents = "memory-events.jsonl"
	FileSessions     = "sessions.jsonl"
	FileConfig       = "config.jsonl"
)

// CanonicalFiles returns the list of all 4 canonical JSONL file names.
func CanonicalFiles() []string {
	return []string{FileWorkGraph, FileMemoryEvents, FileSessions, FileConfig}
}

// SyncResult contains the outcome of a JSONL sync operation.
type SyncResult struct {
	FilesProcessed int            `json:"files_processed"`
	EventsRead     int            `json:"events_read"`
	EventsApplied  int            `json:"events_applied"`
	EventsSkipped  int            `json:"events_skipped"`
	Errors         []string       `json:"errors,omitempty"`
	FileStats      map[string]int `json:"file_stats"`
}

// NormalizedEvent is a JSONL event normalized to canonical form.
type NormalizedEvent struct {
	Event     string         `json:"event"`
	Timestamp string         `json:"timestamp,omitempty"`
	Raw       map[string]any `json:"-"`
}

// SyncFromJSONL reads all 4 canonical JSONL files from ledgerDir, normalizes
// events, and rebuilds the SQLite database. This implements the tolerant JSONL
// parser from the design analysis (D7).
func (d *DB) SyncFromJSONL(ctx context.Context, ledgerDir string, logger *slog.Logger) (*SyncResult, error) {
	if logger == nil {
		logger = slog.Default()
	}

	result := &SyncResult{
		FileStats: make(map[string]int),
	}

	for _, filename := range CanonicalFiles() {
		path := filepath.Join(ledgerDir, filename)

		events, errs := ReadAndNormalize(path)
		result.FileStats[filename] = len(events)
		result.EventsRead += len(events)
		result.FilesProcessed++

		for _, e := range errs {
			result.Errors = append(result.Errors, fmt.Sprintf("%s: %s", filename, e))
			result.EventsSkipped++
		}

		for _, event := range events {
			if err := d.applyEvent(ctx, filename, event); err != nil {
				logger.WarnContext(ctx, "skipping event",
					slog.String("file", filename),
					slog.String("event", event.Event),
					slog.String("error", err.Error()),
				)
				result.EventsSkipped++
				result.Errors = append(result.Errors,
					fmt.Sprintf("%s: applying event %q: %s", filename, event.Event, err.Error()))
				continue
			}
			result.EventsApplied++
		}
	}

	return result, nil
}

// ReadAndNormalize reads a JSONL file and normalizes each line to canonical form.
// Returns normalized events and any parse errors (malformed lines are skipped).
func ReadAndNormalize(path string) ([]NormalizedEvent, []string) {
	f, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, []string{fmt.Sprintf("opening file: %s", err.Error())}
	}
	defer f.Close()

	return readAndNormalizeReader(f)
}

// readAndNormalizeReader reads from an io.Reader and normalizes JSONL events.
func readAndNormalizeReader(r io.Reader) ([]NormalizedEvent, []string) {
	var events []NormalizedEvent
	var errs []string
	lineNum := 0

	scanner := bufio.NewScanner(r)
	// Allow lines up to 1MB for large JSON events.
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)

	for scanner.Scan() {
		lineNum++
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}

		var raw map[string]any
		if err := json.Unmarshal([]byte(line), &raw); err != nil {
			errs = append(errs, fmt.Sprintf("line %d: invalid JSON: %s", lineNum, err.Error()))
			continue
		}

		event := normalizeEvent(raw)
		if event.Event == "" {
			errs = append(errs, fmt.Sprintf("line %d: no event type found", lineNum))
			continue
		}

		events = append(events, event)
	}

	if err := scanner.Err(); err != nil {
		errs = append(errs, fmt.Sprintf("reading file: %s", err.Error()))
	}

	return events, errs
}

// normalizeEvent applies the 4-pattern normalization rules from the design:
//
//	Pattern 1: {"event":"..."} -> already canonical
//	Pattern 2: {"type":"..."} -> rename "type" to "event"
//	Pattern 3: {"e":"..."}    -> rename "e" to "event", "ts" to "timestamp"
//	Pattern 4: {"op":"...","table":"..."} -> map to canonical event
func normalizeEvent(raw map[string]any) NormalizedEvent {
	event := NormalizedEvent{Raw: raw}

	// Pattern 1: "event" key (canonical).
	if e, ok := raw["event"].(string); ok {
		event.Event = e
	}

	// Pattern 2: "type" key.
	if event.Event == "" {
		if t, ok := raw["type"].(string); ok {
			event.Event = normalizeEventName(t)
			delete(raw, "type")
			raw["event"] = event.Event
		}
	}

	// Pattern 3: "e" key (shorthand from ledger.sh).
	if event.Event == "" {
		if e, ok := raw["e"].(string); ok {
			event.Event = e
			delete(raw, "e")
			raw["event"] = event.Event
		}
	}

	// Pattern 4: "op" + "table" keys.
	if event.Event == "" {
		if op, ok := raw["op"].(string); ok {
			if table, ok := raw["table"].(string); ok {
				event.Event = mapOpToEvent(op, table)
				delete(raw, "op")
				delete(raw, "table")
				raw["event"] = event.Event
			}
		}
	}

	// Normalize timestamp fields.
	if ts, ok := raw["ts"].(string); ok {
		event.Timestamp = ts
		delete(raw, "ts")
		raw["timestamp"] = ts
	}
	if ts, ok := raw["timestamp"].(string); ok {
		event.Timestamp = ts
	}

	// Normalize shorthand field names.
	renameField(raw, "sid", "session_id")
	renameField(raw, "wid", "work_id")
	renameField(raw, "from_status", "old_status")
	renameField(raw, "to_status", "new_status")

	return event
}

// normalizeEventName maps legacy event type names to canonical names.
func normalizeEventName(name string) string {
	switch name {
	case "memory_stored":
		return "memory_store"
	default:
		return name
	}
}

// mapOpToEvent maps an op+table combination to a canonical event name.
func mapOpToEvent(op, table string) string {
	op = strings.ToUpper(op)
	switch {
	case op == "INSERT" && table == "memory_events":
		return "memory_store"
	case op == "INSERT" && table == "sessions":
		return "session_start"
	case op == "INSERT" && table == "epics":
		return "epic_created"
	case op == "INSERT" && table == "tasks":
		return "task_created"
	case op == "UPDATE" && table == "tasks":
		return "task_status_changed"
	case op == "UPDATE" && table == "epics":
		return "epic_status_changed"
	case op == "INSERT" && table == "active_work":
		return "begin_work"
	case op == "INSERT":
		return fmt.Sprintf("%s_created", table)
	case op == "UPDATE":
		return fmt.Sprintf("%s_updated", table)
	default:
		return fmt.Sprintf("%s_%s", strings.ToLower(op), table)
	}
}

// renameField renames a key in the map if it exists.
func renameField(m map[string]any, oldKey, newKey string) {
	if val, ok := m[oldKey]; ok {
		if _, exists := m[newKey]; !exists {
			m[newKey] = val
			delete(m, oldKey)
		}
	}
}

// applyEvent routes a normalized event to the appropriate table insert/update.
func (d *DB) applyEvent(ctx context.Context, _ string, event NormalizedEvent) error {
	switch event.Event {
	case "epic_created":
		return d.applyEpicCreated(ctx, event.Raw)
	case "task_created":
		return d.applyTaskCreated(ctx, event.Raw)
	case "task_status_changed":
		return d.applyTaskStatusChanged(ctx, event.Raw)
	case "epic_status_changed":
		return d.applyEpicStatusChanged(ctx, event.Raw)
	case "session_start":
		return d.applySessionStart(ctx, event.Raw)
	case "session_end":
		return d.applySessionEnd(ctx, event.Raw)
	case "memory_store", "progress", "decision", "milestone", "finding", "blocker":
		return d.applyMemoryEvent(ctx, event)
	case "begin_work":
		return d.applyBeginWork(ctx, event.Raw)
	case "complete_work", "work_complete":
		return d.applyCompleteWork(ctx, event.Raw)
	case "config_set", "config_updated":
		// Config events are informational; no table mapping needed.
		return nil
	case "work_claimed":
		// Informational session event.
		return nil
	default:
		// Unknown events are skipped without error to be forward-compatible.
		return nil
	}
}

// applyEpicCreated inserts an epic record from a JSONL event.
func (d *DB) applyEpicCreated(ctx context.Context, raw map[string]any) error {
	id := getString(raw, "id")
	if id == "" {
		return fmt.Errorf("epic_created: missing id")
	}

	formatID := getString(raw, "format_id")
	if formatID == "" {
		formatID = id // Fallback: use id as format_id.
	}

	_, err := d.Execute(ctx,
		`INSERT OR IGNORE INTO epics (id, format_id, title, status, area_type, work_type, domain,
		 is_ongoing, file_scope, priority, created_at, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		id,
		formatID,
		getString(raw, "title"),
		getStringDefault(raw, "status", "draft"),
		getString(raw, "area_type"),
		getString(raw, "work_type"),
		getString(raw, "domain"),
		getBool(raw, "is_ongoing"),
		getJSONString(raw, "file_scope"),
		getStringDefault(raw, "priority", "normal"),
		getStringDefault(raw, "timestamp", ""),
		getStringDefault(raw, "timestamp", ""),
	)
	return err
}

// applyTaskCreated inserts a task record from a JSONL event.
func (d *DB) applyTaskCreated(ctx context.Context, raw map[string]any) error {
	id := getString(raw, "id")
	if id == "" {
		return fmt.Errorf("task_created: missing id")
	}

	epicID := getString(raw, "epic_id")
	if epicID == "" {
		return fmt.Errorf("task_created: missing epic_id")
	}

	formatID := getString(raw, "format_id")
	if formatID == "" {
		formatID = id
	}

	_, err := d.Execute(ctx,
		`INSERT OR IGNORE INTO tasks (id, format_id, epic_id, title, status, area_type,
		 work_type, domain, origin, priority, created_at, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		id,
		formatID,
		epicID,
		getString(raw, "title"),
		getStringDefault(raw, "status", "todo"),
		getString(raw, "area_type"),
		getString(raw, "work_type"),
		getString(raw, "domain"),
		getStringDefault(raw, "origin", "planned"),
		getStringDefault(raw, "priority", "normal"),
		getStringDefault(raw, "timestamp", ""),
		getStringDefault(raw, "timestamp", ""),
	)
	return err
}

// applyTaskStatusChanged updates a task's status.
func (d *DB) applyTaskStatusChanged(ctx context.Context, raw map[string]any) error {
	taskID := getString(raw, "task_id")
	if taskID == "" {
		return fmt.Errorf("task_status_changed: missing task_id")
	}

	newStatus := getString(raw, "new_status")
	if newStatus == "" {
		return fmt.Errorf("task_status_changed: missing new_status")
	}

	_, err := d.Execute(ctx,
		"UPDATE tasks SET status = ?, updated_at = ? WHERE id = ?",
		newStatus,
		getStringDefault(raw, "timestamp", ""),
		taskID,
	)
	return err
}

// applyEpicStatusChanged updates an epic's status.
func (d *DB) applyEpicStatusChanged(ctx context.Context, raw map[string]any) error {
	epicID := getString(raw, "epic_id")
	if epicID == "" {
		return fmt.Errorf("epic_status_changed: missing epic_id")
	}

	newStatus := getString(raw, "new_status")
	if newStatus == "" {
		return fmt.Errorf("epic_status_changed: missing new_status")
	}

	_, err := d.Execute(ctx,
		"UPDATE epics SET status = ?, updated_at = ? WHERE id = ?",
		newStatus,
		getStringDefault(raw, "timestamp", ""),
		epicID,
	)
	return err
}

// applySessionStart inserts or updates a session record.
func (d *DB) applySessionStart(ctx context.Context, raw map[string]any) error {
	sessionID := getString(raw, "session_id")
	if sessionID == "" {
		return nil // Skip sessions without ID.
	}

	// user_id is a NOT NULL FK to users(id). Use nil if not provided
	// to let the database handle the constraint.
	var userID any
	if v := getString(raw, "user_id"); v != "" {
		userID = v
	}

	_, err := d.Execute(ctx,
		`INSERT OR IGNORE INTO sessions (id, user_id, user_host, started_at, status)
		 VALUES (?, ?, ?, ?, ?)`,
		sessionID,
		userID,
		getStringDefault(raw, "user_host", "unknown"),
		getStringDefault(raw, "timestamp", ""),
		"active",
	)
	return err
}

// applySessionEnd updates a session's end state.
func (d *DB) applySessionEnd(ctx context.Context, raw map[string]any) error {
	sessionID := getString(raw, "session_id")
	if sessionID == "" {
		return nil
	}

	_, err := d.Execute(ctx,
		"UPDATE sessions SET ended_at = ?, status = ? WHERE id = ?",
		getStringDefault(raw, "timestamp", ""),
		getStringDefault(raw, "status", "completed"),
		sessionID,
	)
	return err
}

// applyMemoryEvent inserts a memory event record.
func (d *DB) applyMemoryEvent(ctx context.Context, event NormalizedEvent) error {
	id := getString(event.Raw, "id")
	if id == "" {
		return nil // Skip events without ID.
	}

	// Marshal the data field.
	dataStr := ""
	if data, ok := event.Raw["data"]; ok {
		b, err := json.Marshal(data)
		if err == nil {
			dataStr = string(b)
		}
	}

	eventType := getString(event.Raw, "event_type")
	if eventType == "" {
		eventType = event.Event
	}

	domain := getStringDefault(event.Raw, "domain", "development")

	_, err := d.Execute(ctx,
		`INSERT OR IGNORE INTO memory_events (id, event_type, domain, work_id, data,
		 memory_type, created_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?)`,
		id,
		eventType,
		domain,
		getString(event.Raw, "work_id"),
		dataStr,
		getString(event.Raw, "memory_type"),
		getStringDefault(event.Raw, "timestamp", ""),
	)
	return err
}

// applyBeginWork inserts an active_work record.
func (d *DB) applyBeginWork(ctx context.Context, raw map[string]any) error {
	id := getString(raw, "id")
	if id == "" {
		id = getString(raw, "work_id")
	}
	if id == "" {
		return nil
	}

	// task_id is a nullable FK to tasks(id). Use nil for empty values
	// to avoid FK constraint violations with empty strings.
	var taskID any
	if v := getString(raw, "task_id"); v != "" {
		taskID = v
	}

	_, err := d.Execute(ctx,
		`INSERT OR IGNORE INTO active_work (id, task_id, topic, status, branch,
		 session_id, created_at, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
		id,
		taskID,
		getStringDefault(raw, "topic", ""),
		"in_progress",
		getString(raw, "branch"),
		getString(raw, "session_id"),
		getStringDefault(raw, "timestamp", ""),
		getStringDefault(raw, "timestamp", ""),
	)
	return err
}

// applyCompleteWork updates an active_work record to complete.
func (d *DB) applyCompleteWork(ctx context.Context, raw map[string]any) error {
	id := getString(raw, "id")
	if id == "" {
		id = getString(raw, "work_id")
	}
	if id == "" {
		return nil
	}

	_, err := d.Execute(ctx,
		"UPDATE active_work SET status = 'complete', updated_at = ? WHERE id = ?",
		getStringDefault(raw, "timestamp", ""),
		id,
	)
	return err
}

// Helper functions for extracting typed values from a map.

func getString(m map[string]any, key string) string {
	if v, ok := m[key].(string); ok {
		return v
	}
	return ""
}

func getStringDefault(m map[string]any, key, def string) string {
	if v := getString(m, key); v != "" {
		return v
	}
	return def
}

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

func getJSONString(m map[string]any, key string) string {
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
