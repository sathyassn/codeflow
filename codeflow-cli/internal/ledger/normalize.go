package ledger

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// NormalizeConfig controls normalization behavior.
type NormalizeConfig struct {
	// DryRun reports what would change without writing files.
	DryRun bool

	// LedgerDir is the directory containing canonical JSONL files.
	// Used by NormalizeAll; ignored by NormalizeFile.
	LedgerDir string
}

// NormalizeResult contains the outcome of normalizing a single JSONL file.
type NormalizeResult struct {
	File            string   `json:"file"`
	RecordsRead     int      `json:"records_read"`
	RecordsWritten  int      `json:"records_written"`
	RecordsDropped  int      `json:"records_dropped"`
	FieldsRenamed   int      `json:"fields_renamed"`
	MalformedLines  int      `json:"malformed_lines"`
	Warnings        []string `json:"warnings,omitempty"`
}

// NormalizeFile reads a JSONL file, applies canonical normalization rules
// consistent with db/sync.go:normalizeEvent(), and atomically replaces the
// file with the normalized output. Malformed JSON lines are preserved in the
// output with a warning.
//
// The normalization rules are:
//   - Rename "type" -> "event" (with event name normalization, e.g. memory_stored -> memory_store)
//   - Rename "e" -> "event"
//   - Rename "ts" -> "timestamp"
//   - Rename "sid" -> "session_id"
//   - Rename "wid" -> "work_id"
//   - Rename "from_status" -> "old_status"
//   - Rename "to_status" -> "new_status"
//   - Map "op" + "table" -> canonical event name; drop records where mapping yields empty event
//   - Records with no resolvable event type are dropped
//
// Idempotent: running twice produces identical output.
func NormalizeFile(filePath string, config NormalizeConfig) (*NormalizeResult, error) {
	f, err := os.Open(filePath)
	if err != nil {
		return nil, fmt.Errorf("opening %s: %w", filePath, err)
	}
	defer f.Close()

	result := &NormalizeResult{
		File: filepath.Base(filePath),
	}

	var outputLines []string

	scanner := bufio.NewScanner(f)
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	lineNum := 0

	for scanner.Scan() {
		lineNum++
		line := scanner.Text()
		trimmed := strings.TrimSpace(line)

		// Preserve blank lines as-is.
		if trimmed == "" {
			outputLines = append(outputLines, "")
			continue
		}

		result.RecordsRead++

		var raw map[string]any
		if err := json.Unmarshal([]byte(trimmed), &raw); err != nil {
			// Preserve malformed lines in output with a warning.
			result.MalformedLines++
			result.Warnings = append(result.Warnings,
				fmt.Sprintf("line %d: invalid JSON (preserved): %s", lineNum, err.Error()))
			outputLines = append(outputLines, trimmed)
			result.RecordsWritten++
			continue
		}

		renamed, event := normalizeRecord(raw)
		result.FieldsRenamed += renamed

		// Drop records with no resolvable event type.
		if event == "" {
			result.RecordsDropped++
			result.Warnings = append(result.Warnings,
				fmt.Sprintf("line %d: dropped (no resolvable event type)", lineNum))
			continue
		}

		normalized, err := json.Marshal(raw)
		if err != nil {
			// Should not happen for data that was already valid JSON.
			result.Warnings = append(result.Warnings,
				fmt.Sprintf("line %d: marshal error (preserved original): %s", lineNum, err.Error()))
			outputLines = append(outputLines, trimmed)
			result.RecordsWritten++
			continue
		}

		outputLines = append(outputLines, string(normalized))
		result.RecordsWritten++
	}

	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("reading %s: %w", filePath, err)
	}

	if config.DryRun {
		return result, nil
	}

	// Atomic write: write to temp file in the same directory, then rename.
	dir := filepath.Dir(filePath)
	tmpFile, err := os.CreateTemp(dir, ".normalize-*.tmp")
	if err != nil {
		return nil, fmt.Errorf("creating temp file: %w", err)
	}
	tmpPath := tmpFile.Name()

	writer := bufio.NewWriter(tmpFile)
	for i, line := range outputLines {
		if _, err := writer.WriteString(line); err != nil {
			tmpFile.Close()
			os.Remove(tmpPath)
			return nil, fmt.Errorf("writing temp file: %w", err)
		}
		if i < len(outputLines)-1 {
			if err := writer.WriteByte('\n'); err != nil {
				tmpFile.Close()
				os.Remove(tmpPath)
				return nil, fmt.Errorf("writing temp file: %w", err)
			}
		}
	}
	// Ensure file ends with newline if non-empty.
	if len(outputLines) > 0 {
		if err := writer.WriteByte('\n'); err != nil {
			tmpFile.Close()
			os.Remove(tmpPath)
			return nil, fmt.Errorf("writing temp file: %w", err)
		}
	}

	if err := writer.Flush(); err != nil {
		tmpFile.Close()
		os.Remove(tmpPath)
		return nil, fmt.Errorf("flushing temp file: %w", err)
	}
	if err := tmpFile.Close(); err != nil {
		os.Remove(tmpPath)
		return nil, fmt.Errorf("closing temp file: %w", err)
	}

	// Preserve original file permissions.
	info, err := os.Stat(filePath)
	if err == nil {
		os.Chmod(tmpPath, info.Mode()) //nolint:errcheck // best-effort
	}

	if err := os.Rename(tmpPath, filePath); err != nil {
		os.Remove(tmpPath)
		return nil, fmt.Errorf("replacing %s: %w", filePath, err)
	}

	return result, nil
}

// NormalizeAll normalizes all 4 canonical JSONL files in the given directory.
// Files that do not exist are skipped without error.
func NormalizeAll(ledgerDir string, config NormalizeConfig) ([]*NormalizeResult, error) {
	var results []*NormalizeResult

	for _, filename := range CanonicalFiles() {
		path := filepath.Join(ledgerDir, filename)

		if _, err := os.Stat(path); os.IsNotExist(err) {
			continue
		}

		result, err := NormalizeFile(path, config)
		if err != nil {
			return results, fmt.Errorf("normalizing %s: %w", filename, err)
		}

		results = append(results, result)
	}

	return results, nil
}

// PrintResults writes a human-readable summary of normalization results.
func PrintResults(w io.Writer, results []*NormalizeResult, dryRun bool) {
	if dryRun {
		fmt.Fprintln(w, "DRY RUN — no files modified")
		fmt.Fprintln(w)
	}

	totalRead := 0
	totalWritten := 0
	totalDropped := 0
	totalRenamed := 0
	totalMalformed := 0

	for _, r := range results {
		fmt.Fprintf(w, "  %s: %d read, %d written, %d dropped, %d fields renamed",
			r.File, r.RecordsRead, r.RecordsWritten, r.RecordsDropped, r.FieldsRenamed)
		if r.MalformedLines > 0 {
			fmt.Fprintf(w, ", %d malformed", r.MalformedLines)
		}
		fmt.Fprintln(w)

		for _, warn := range r.Warnings {
			fmt.Fprintf(w, "    WARN: %s\n", warn)
		}

		totalRead += r.RecordsRead
		totalWritten += r.RecordsWritten
		totalDropped += r.RecordsDropped
		totalRenamed += r.FieldsRenamed
		totalMalformed += r.MalformedLines
	}

	fmt.Fprintln(w)
	fmt.Fprintf(w, "Total: %d files, %d read, %d written, %d dropped, %d fields renamed",
		len(results), totalRead, totalWritten, totalDropped, totalRenamed)
	if totalMalformed > 0 {
		fmt.Fprintf(w, ", %d malformed", totalMalformed)
	}
	fmt.Fprintln(w)
}

// normalizeRecord applies normalization rules to a single JSONL record in-place.
// Returns the number of field renames performed and the resolved event name.
// This is consistent with db/sync.go:normalizeEvent() but operates on-disk
// rather than in-memory during sync.
func normalizeRecord(raw map[string]any) (renamed int, event string) {
	// Pattern 1: "event" key (canonical).
	if e, ok := raw["event"].(string); ok {
		event = e
	}

	// Pattern 2: "type" key.
	if event == "" {
		if t, ok := raw["type"].(string); ok {
			event = normalizeEventName(t)
			delete(raw, "type")
			raw["event"] = event
			renamed++
		}
	}

	// Pattern 3: "e" key (shorthand from ledger.sh).
	if event == "" {
		if e, ok := raw["e"].(string); ok {
			event = e
			delete(raw, "e")
			raw["event"] = event
			renamed++
		}
	}

	// Pattern 4: "op" + "table" keys.
	if event == "" {
		if op, ok := raw["op"].(string); ok {
			if table, ok := raw["table"].(string); ok {
				event = mapOpToEventName(op, table)
				delete(raw, "op")
				delete(raw, "table")
				if event != "" {
					raw["event"] = event
					renamed++
				}
			}
		}
	}

	// Normalize timestamp fields.
	if _, ok := raw["ts"]; ok {
		if ts, ok := raw["ts"].(string); ok {
			if _, exists := raw["timestamp"]; !exists {
				raw["timestamp"] = ts
			}
			delete(raw, "ts")
			renamed++
		}
	}

	// Normalize shorthand field names.
	renamed += renameFieldIfPresent(raw, "sid", "session_id")
	renamed += renameFieldIfPresent(raw, "wid", "work_id")
	renamed += renameFieldIfPresent(raw, "from_status", "old_status")
	renamed += renameFieldIfPresent(raw, "to_status", "new_status")

	return renamed, event
}

// normalizeEventName maps legacy event type names to canonical names.
// Consistent with db/sync.go:normalizeEventName().
func normalizeEventName(name string) string {
	switch name {
	case "memory_stored":
		return "memory_store"
	default:
		return name
	}
}

// mapOpToEventName maps an op+table combination to a canonical event name.
// Consistent with db/sync.go:mapOpToEvent().
func mapOpToEventName(op, table string) string {
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

// renameFieldIfPresent renames a key in the map if it exists and the target
// does not. Returns 1 if a rename occurred, 0 otherwise.
func renameFieldIfPresent(m map[string]any, oldKey, newKey string) int {
	if val, ok := m[oldKey]; ok {
		if _, exists := m[newKey]; !exists {
			m[newKey] = val
			delete(m, oldKey)
			return 1
		}
	}
	return 0
}
