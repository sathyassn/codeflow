package session

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
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

// gitConfigValue runs "git config" and returns the trimmed output.
// Returns empty string on any error.
func gitConfigValue(key string) string {
	out, err := exec.Command("git", "config", key).Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

// GitUser holds the identity from git config.
type GitUser struct {
	Email       string
	DisplayName string
}

// resolveGitUser reads git config user.email and user.name.
// Falls back to the provided claudeID if git config is unavailable.
func resolveGitUser(claudeID string) GitUser {
	email := gitConfigValue("user.email")
	name := gitConfigValue("user.name")
	if email == "" {
		email = claudeID
	}
	if name == "" {
		name = email
	}
	return GitUser{Email: email, DisplayName: name}
}

// Start creates a new session. It generates a ULID-based session ID, inserts
// a record into the sessions table, and writes a session_start event to
// sessions.jsonl.
//
// The caller is responsible for writing current-session-id after Start returns.
// In the hook path (start.go), this is guarded by source (only for "startup").
// In the CLI path (codeflow session start), it is written unconditionally.
// codeflow-env.sh is the single source of truth for session ID.
//
// The user record is created from git config (user.email and user.name).
// The claudeID (agent UUID) is stored in the session metadata field.
//
// runtimeDir is retained for API compatibility but is no longer used by Start.
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

	// Resolve real git user for the user record.
	gitUser := resolveGitUser(claudeID)

	// Store claude agent UUID in session metadata.
	metadataJSON, _ := json.Marshal(map[string]string{
		"claude_id": claudeID,
	})

	// Wrap both DB operations in a transaction so a failure in the session
	// INSERT rolls back the user INSERT, preventing orphaned records.
	err = d.Transaction(ctx, func(tx *sql.Tx) error {
		// Ensure the user row exists (FK constraint: sessions.user_id -> users.id).
		// Use email as the user ID for deduplication by real identity.
		if _, txErr := tx.ExecContext(ctx,
			`INSERT OR IGNORE INTO users (id, email, display_name) VALUES (?, ?, ?)`,
			gitUser.Email, gitUser.Email, gitUser.DisplayName,
		); txErr != nil {
			return fmt.Errorf("ensuring user exists: %w", txErr)
		}

		// Insert the session record with metadata containing the claude agent UUID.
		if _, txErr := tx.ExecContext(ctx,
			`INSERT INTO sessions (id, user_id, user_host, started_at, status, metadata)
			 VALUES (?, ?, ?, ?, ?, ?)`,
			sessionID, gitUser.Email, hostname, now, "active", string(metadataJSON),
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
		"user_id":    gitUser.Email,
		"user_host":  hostname,
		"timestamp":  now,
		"claude_id":  claudeID,
	}); err != nil {
		return "", fmt.Errorf("session: writing JSONL event: %w", err)
	}

	// Note: current-session-id is NOT written here. The caller is responsible
	// for writing it. In the hook path (start.go:StartInit), it is written only
	// for new sessions (source=startup). In the CLI path (codeflow session start),
	// it is written after this function returns. codeflow-env.sh is the single
	// source of truth for session ID.

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

// UpdateTracking sets the V4 PathFlow tracking columns on a session record.
func UpdateTracking(ctx context.Context, d *db.DB, sessionID string, pathflowMode string, trackingLevel string) error {
	now := time.Now().UTC().Format(time.RFC3339)
	_, err := d.Execute(ctx,
		`UPDATE sessions SET pathflow_mode = ?, tracking_level = ?, metadata = json_set(COALESCE(metadata, '{}'), '$.updated_at', ?)
		 WHERE id = ?`,
		pathflowMode, trackingLevel, now, sessionID,
	)
	if err != nil {
		return fmt.Errorf("session: updating tracking: %w", err)
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
