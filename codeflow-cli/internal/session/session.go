package session

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/oklog/ulid/v2"
)

// Sentinel errors for session operations.
var (
	// ErrEmptyClaudeID indicates that the claude ID was empty.
	ErrEmptyClaudeID = errors.New("session: claude ID must not be empty")

	// ErrNoActiveSession indicates no active session was found.
	ErrNoActiveSession = errors.New("session: no active session")

	// ErrAlreadyEnded indicates the session has already been ended.
	ErrAlreadyEnded = errors.New("session: session already ended")
)

// Default paths relative to the project root.
const (
	DefaultLedgerDir  = ".state/ledger"
	DefaultRuntimeDir = ".state/runtime"
	CurrentSessionFile = "current-session-id"
)

// generateID creates a new session ID in the format "ses-{ulid}".
func generateID() string {
	return "ses-" + strings.ToLower(ulid.Make().String())
}

// Start creates a new session. It generates a ULID-based session ID, inserts
// a record into the sessions table, writes a session_start event to
// sessions.jsonl, and writes the session ID to current-session-id.
func Start(ctx context.Context, d *db.DB, claudeID string, ledgerDir string, runtimeDir string) (string, error) {
	if claudeID == "" {
		return "", ErrEmptyClaudeID
	}

	sessionID := generateID()
	now := time.Now().UTC().Format(time.RFC3339)

	hostname, err := os.Hostname()
	if err != nil {
		hostname = "unknown"
	}

	// Wrap both DB operations in a transaction so a failure in the session
	// INSERT rolls back the user INSERT, preventing orphaned records.
	err = d.Transaction(ctx, func(tx *sql.Tx) error {
		// Ensure the user row exists (FK constraint: sessions.user_id -> users.id).
		if _, txErr := tx.ExecContext(ctx,
			`INSERT OR IGNORE INTO users (id, email, display_name) VALUES (?, ?, ?)`,
			claudeID, claudeID, claudeID,
		); txErr != nil {
			return fmt.Errorf("ensuring user exists: %w", txErr)
		}

		// Insert the session record.
		if _, txErr := tx.ExecContext(ctx,
			`INSERT INTO sessions (id, user_id, user_host, started_at, status)
			 VALUES (?, ?, ?, ?, ?)`,
			sessionID, claudeID, hostname, now, "active",
		); txErr != nil {
			return fmt.Errorf("inserting session record: %w", txErr)
		}

		return nil
	})
	if err != nil {
		return "", fmt.Errorf("session: %w", err)
	}

	// Write JSONL event.
	if err := writeJSONLEvent(ledgerDir, map[string]string{
		"event":      "session_start",
		"session_id": sessionID,
		"user_id":    claudeID,
		"user_host":  hostname,
		"timestamp":  now,
	}); err != nil {
		return "", fmt.Errorf("session: writing JSONL event: %w", err)
	}

	// Write current-session-id file.
	if err := writeCurrentSessionID(runtimeDir, sessionID); err != nil {
		return "", fmt.Errorf("session: writing current-session-id: %w", err)
	}

	return sessionID, nil
}

// End completes the current active session. It reads the current session ID,
// updates the session record with ended_at, duration_seconds, and status=completed,
// writes a session_end event to sessions.jsonl, and removes the current-session-id file.
func End(ctx context.Context, d *db.DB, ledgerDir string, runtimeDir string) error {
	sessionID, err := Current(runtimeDir)
	if err != nil {
		return err
	}

	// Read the session's started_at to calculate duration.
	var startedAt string
	var status string
	err = d.QueryRow(ctx,
		"SELECT started_at, status FROM sessions WHERE id = ?", sessionID,
	).Scan(&startedAt, &status)
	if err != nil {
		return fmt.Errorf("session: reading session record: %w", err)
	}

	if status == "completed" {
		return fmt.Errorf("%w: %s", ErrAlreadyEnded, sessionID)
	}

	now := time.Now().UTC()
	nowStr := now.Format(time.RFC3339)

	// Calculate duration.
	var durationSeconds int64
	if startedAt != "" {
		if started, parseErr := time.Parse(time.RFC3339, startedAt); parseErr == nil {
			durationSeconds = int64(now.Sub(started).Seconds())
		}
	}

	// Update the session record.
	_, err = d.Execute(ctx,
		`UPDATE sessions SET ended_at = ?, duration_seconds = ?, status = ?
		 WHERE id = ?`,
		nowStr, durationSeconds, "completed", sessionID,
	)
	if err != nil {
		return fmt.Errorf("session: updating session record: %w", err)
	}

	// Write JSONL event.
	if err := writeJSONLEvent(ledgerDir, map[string]any{
		"event":            "session_end",
		"session_id":       sessionID,
		"timestamp":        nowStr,
		"duration_seconds": durationSeconds,
	}); err != nil {
		return fmt.Errorf("session: writing JSONL event: %w", err)
	}

	// Remove current-session-id file.
	idFile := filepath.Join(runtimeDir, CurrentSessionFile)
	if err := os.Remove(idFile); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("session: removing current-session-id: %w", err)
	}

	return nil
}

// Current reads and returns the current session ID from the current-session-id file.
func Current(runtimeDir string) (string, error) {
	idFile := filepath.Join(runtimeDir, CurrentSessionFile)
	data, err := os.ReadFile(idFile)
	if err != nil {
		if os.IsNotExist(err) {
			return "", ErrNoActiveSession
		}
		return "", fmt.Errorf("session: reading current-session-id: %w", err)
	}

	sessionID := strings.TrimSpace(string(data))
	if sessionID == "" {
		return "", ErrNoActiveSession
	}

	return sessionID, nil
}

// writeJSONLEvent appends a JSON event line to sessions.jsonl in the ledger directory.
func writeJSONLEvent(ledgerDir string, event any) error {
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		return fmt.Errorf("creating ledger directory: %w", err)
	}

	jsonlPath := filepath.Join(ledgerDir, db.FileSessions)

	data, err := json.Marshal(event)
	if err != nil {
		return fmt.Errorf("marshaling event: %w", err)
	}

	f, err := os.OpenFile(jsonlPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("opening JSONL file: %w", err)
	}
	defer f.Close()

	if _, err := f.Write(append(data, '\n')); err != nil {
		return fmt.Errorf("writing event: %w", err)
	}

	return nil
}

// writeCurrentSessionID writes the session ID to the current-session-id file.
func writeCurrentSessionID(runtimeDir string, sessionID string) error {
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		return fmt.Errorf("creating runtime directory: %w", err)
	}

	idFile := filepath.Join(runtimeDir, CurrentSessionFile)
	return os.WriteFile(idFile, []byte(sessionID), 0o644)
}
