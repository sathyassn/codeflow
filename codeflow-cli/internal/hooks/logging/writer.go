package logging

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

// ActivityWriter appends structured log entries to date-rotated JSONL files
// with flock-based file locking. It is distinct from the ledger.Writer which
// handles Tier 0 WorkGraph events with schema validation.
type ActivityWriter struct {
	// dir is the log directory (e.g., .state/logs/sessions).
	dir string

	// Now returns the current time. Override in tests for deterministic output.
	Now func() time.Time
}

// NewActivityWriter creates an ActivityWriter for the given project directory.
// It resolves the log directory from the config and creates it if needed.
func NewActivityWriter(projectDir string, logDir string) (*ActivityWriter, error) {
	absDir := resolveLogDir(projectDir, logDir)
	if err := os.MkdirAll(absDir, 0o755); err != nil {
		return nil, fmt.Errorf("logging: creating directory %s: %w", absDir, err)
	}
	return &ActivityWriter{
		dir: absDir,
		Now: func() time.Time { return time.Now().UTC() },
	}, nil
}

// Append writes a log record to a date-rotated JSONL file.
// The logType determines the file prefix (e.g., "session", "tool-use", "prompts").
// The resulting file name is "{logType}-{YYYY-MM-DD}.jsonl".
func (w *ActivityWriter) Append(logType string, record map[string]any) error {
	now := w.Now()
	dateStr := now.Format("2006-01-02")
	filename := fmt.Sprintf("%s-%s.jsonl", logType, dateStr)

	data, err := json.Marshal(record)
	if err != nil {
		return fmt.Errorf("logging: marshaling record: %w", err)
	}
	data = append(data, '\n')

	filePath := filepath.Join(w.dir, filename)
	lockPath := filePath + ".lock"

	lockFile, err := os.OpenFile(lockPath, os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("logging: opening lock file: %w", err)
	}
	defer lockFile.Close()

	if err := syscall.Flock(int(lockFile.Fd()), syscall.LOCK_EX); err != nil {
		return fmt.Errorf("logging: acquiring lock: %w", err)
	}
	defer func() {
		_ = syscall.Flock(int(lockFile.Fd()), syscall.LOCK_UN)
	}()

	f, err := os.OpenFile(filePath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("logging: opening file: %w", err)
	}
	defer f.Close()

	if _, err := f.Write(data); err != nil {
		return fmt.Errorf("logging: writing record: %w", err)
	}

	return nil
}

// Dir returns the log directory path.
func (w *ActivityWriter) Dir() string {
	return w.dir
}

// Timestamp returns a formatted timestamp matching shell hook convention.
// Format: "2006-01-02T15:04:05.000Z" (ms precision, literal Z).
func (w *ActivityWriter) Timestamp() string {
	return w.Now().Format("2006-01-02T15:04:05.000Z")
}

// ResolveSessionID determines the session ID using the canonical priority:
//  1. Parse codeflow-env.sh at .state/runtime/codeflow-env.sh
//  2. CODEFLOW_SESSION_ID environment variable
//  3. .state/runtime/current-session-id file
//  4. Fallback to "unknown"
func ResolveSessionID(projectDir string) string {
	// Priority 1: env file.
	envFilePath := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	if data, err := os.ReadFile(envFilePath); err == nil {
		if sid := parseEnvFileSessionID(string(data)); sid != "" {
			return sid
		}
	}

	// Priority 2: CODEFLOW_SESSION_ID environment variable.
	if sid := os.Getenv("CODEFLOW_SESSION_ID"); sid != "" {
		return sid
	}

	// Priority 3: current-session-id file.
	csidPath := filepath.Join(projectDir, ".state", "runtime", "current-session-id")
	if data, err := os.ReadFile(csidPath); err == nil {
		if sid := strings.TrimSpace(string(data)); sid != "" {
			return sid
		}
	}

	return "unknown"
}

// parseEnvFileSessionID extracts CODEFLOW_SESSION_ID from env file content.
func parseEnvFileSessionID(content string) string {
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "export CODEFLOW_SESSION_ID=") {
			val := strings.TrimPrefix(line, "export CODEFLOW_SESSION_ID=")
			val = strings.Trim(val, "'\"")
			return val
		}
	}
	return ""
}
