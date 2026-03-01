package db

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

// CRDTClaim represents a claim in the rebuilt coordination state.
type CRDTClaim struct {
	ID           string `json:"id"`
	WorkID       string `json:"work_id"`
	Pattern      string `json:"pattern"`
	Mode         string `json:"mode"`
	OwnerID      string `json:"owner_id"`
	FencingToken int64  `json:"fencing_token"`
	ExpiresAt    string `json:"expires_at"`
	Status       string `json:"status"`
	CreatedAt    string `json:"created_at"`
}

// CRDTDoc represents the rebuilt coordination document.
type CRDTDoc struct {
	Claims       map[string]*CRDTClaim `json:"claims"`
	TokenCounter int64                 `json:"token_counter"`
}

// RebuildStats holds metrics from a CRDT rebuild operation.
type RebuildStats struct {
	EventsReplayed int            `json:"events_replayed"`
	ActiveClaims   int            `json:"active_claims"`
	ReleasedClaims int            `json:"released_claims"`
	MaxFencingToken int64         `json:"max_fencing_token"`
	DurationMs     int64          `json:"duration_ms"`
	StatusCounts   map[string]int `json:"status_counts"`
}

// RebuildResult contains the outcome of a CRDT rebuild operation.
type RebuildResult struct {
	Doc          *CRDTDoc      `json:"doc"`
	Stats        *RebuildStats `json:"stats"`
	RebuildType  string        `json:"rebuild_type"`
	Source       string        `json:"source"`
	Until        string        `json:"until,omitempty"`
	DryRun       bool          `json:"dry_run,omitempty"`
	Saved        bool          `json:"saved,omitempty"`
	BackupPath   string        `json:"backup_path,omitempty"`
}

// VerifyMismatch describes a difference between rebuilt and existing state.
type VerifyMismatch struct {
	ClaimID  string `json:"claim_id"`
	Issue    string `json:"issue"`
	Rebuilt  string `json:"rebuilt,omitempty"`
	Existing string `json:"existing,omitempty"`
}

// VerifyResult contains the outcome of a CRDT verify operation.
type VerifyResult struct {
	RebuildResult
	Mismatches []VerifyMismatch `json:"mismatches"`
	VerifiedOK bool             `json:"verified_ok"`
}

// RebuildOptions configures the CRDT rebuild operation.
type RebuildOptions struct {
	// Source is the path to sessions.jsonl. Required.
	Source string

	// Until is an optional ISO 8601 timestamp cutoff for point-in-time rebuild.
	Until string

	// DryRun shows what would be rebuilt without saving.
	DryRun bool

	// Verify compares rebuilt state against existing without replacing.
	Verify bool

	// StateDir is the base .state directory containing coordination/state.json.
	StateDir string
}

// RebuildCRDT rebuilds the coordination state from JSONL claim events.
// It replays claim_created, claim_released, and claim_renewed events from
// the sessions.jsonl file to reconstruct the full coordination document.
func RebuildCRDT(opts RebuildOptions) (*RebuildResult, error) {
	if opts.Source == "" {
		return nil, fmt.Errorf("crdt: source path is required")
	}

	if _, err := os.Stat(opts.Source); os.IsNotExist(err) {
		return nil, fmt.Errorf("crdt: source not found: %s", opts.Source)
	}

	start := time.Now()

	// Parse the until timestamp if provided.
	var untilTime *time.Time
	if opts.Until != "" {
		t, err := parseTimestamp(opts.Until)
		if err != nil {
			return nil, fmt.Errorf("crdt: parsing --until timestamp: %w", err)
		}
		untilTime = &t
	}

	// Read and normalize events from the JSONL file.
	events, parseErrs := ReadAndNormalize(opts.Source)
	_ = parseErrs // Tolerant parser: skip malformed lines.

	// Replay claim events to rebuild state.
	doc := &CRDTDoc{
		Claims: make(map[string]*CRDTClaim),
	}
	eventsReplayed := 0

	for _, event := range events {
		// Check time cutoff.
		if untilTime != nil && event.Timestamp != "" {
			eventTime, err := parseTimestamp(event.Timestamp)
			if err == nil && eventTime.After(*untilTime) {
				continue
			}
		}

		switch event.Event {
		case "claim_created":
			claimID := getClaimID(event.Raw)
			if claimID == "" {
				continue
			}

			fencingToken := getInt64(event.Raw, "fencing_token")
			doc.Claims[claimID] = &CRDTClaim{
				ID:           claimID,
				WorkID:       getString(event.Raw, "work_id"),
				Pattern:      getString(event.Raw, "pattern"),
				Mode:         getStringDefault(event.Raw, "mode", "exclusive"),
				OwnerID:      getString(event.Raw, "owner_id"),
				FencingToken: fencingToken,
				ExpiresAt:    getString(event.Raw, "expires_at"),
				Status:       "active",
				CreatedAt:    getStringDefault(event.Raw, "created_at", event.Timestamp),
			}
			if fencingToken > doc.TokenCounter {
				doc.TokenCounter = fencingToken
			}
			eventsReplayed++

		case "claim_released":
			claimID := getString(event.Raw, "claim_id")
			if claimID == "" {
				continue
			}
			if c, ok := doc.Claims[claimID]; ok {
				c.Status = "released"
				eventsReplayed++
			}

		case "claim_renewed":
			claimID := getString(event.Raw, "claim_id")
			if claimID == "" {
				continue
			}
			if c, ok := doc.Claims[claimID]; ok {
				c.ExpiresAt = getString(event.Raw, "expires_at")
				eventsReplayed++
			}
		}
	}

	durationMs := time.Since(start).Milliseconds()

	// Compute status counts.
	statusCounts := map[string]int{"active": 0, "released": 0}
	for _, c := range doc.Claims {
		statusCounts[c.Status]++
	}

	stats := &RebuildStats{
		EventsReplayed:  eventsReplayed,
		ActiveClaims:    statusCounts["active"],
		ReleasedClaims:  statusCounts["released"],
		MaxFencingToken: doc.TokenCounter,
		DurationMs:      durationMs,
		StatusCounts:    statusCounts,
	}

	rebuildType := "full"
	if opts.Until != "" {
		rebuildType = "point_in_time"
	}

	result := &RebuildResult{
		Doc:         doc,
		Stats:       stats,
		RebuildType: rebuildType,
		Source:      opts.Source,
		Until:       opts.Until,
		DryRun:      opts.DryRun,
	}

	if opts.DryRun {
		return result, nil
	}

	if opts.Verify {
		return nil, fmt.Errorf("crdt: use VerifyCRDT for verification")
	}

	// Save rebuilt state.
	if opts.StateDir == "" {
		return nil, fmt.Errorf("crdt: state directory is required for save")
	}

	backupPath, err := backupExistingState(opts.StateDir)
	if err != nil {
		return nil, fmt.Errorf("crdt: backup failed: %w", err)
	}
	result.BackupPath = backupPath

	if err := saveCoordinationDoc(opts.StateDir, doc); err != nil {
		return nil, fmt.Errorf("crdt: saving rebuilt state: %w", err)
	}
	result.Saved = true

	return result, nil
}

// VerifyCRDT rebuilds state from JSONL and compares against existing.
func VerifyCRDT(opts RebuildOptions) (*VerifyResult, error) {
	opts.DryRun = false
	opts.Verify = false // Prevent recursion.

	// Rebuild without saving.
	saveStateDir := opts.StateDir
	opts.StateDir = "" // Force dry-run-like behavior.
	opts.DryRun = true

	rebuildResult, err := RebuildCRDT(opts)
	if err != nil {
		return nil, err
	}

	// Load existing state.
	existing, err := loadCoordinationDoc(saveStateDir)
	if err != nil {
		return nil, fmt.Errorf("crdt: loading existing state for verify: %w", err)
	}

	// Compare.
	var mismatches []VerifyMismatch

	for claimID, rebuilt := range rebuildResult.Doc.Claims {
		existingClaim, ok := existing.Claims[claimID]
		if !ok {
			mismatches = append(mismatches, VerifyMismatch{
				ClaimID: claimID,
				Issue:   "missing_in_existing",
			})
			continue
		}
		if existingClaim.Status != rebuilt.Status {
			mismatches = append(mismatches, VerifyMismatch{
				ClaimID:  claimID,
				Issue:    "status_mismatch",
				Rebuilt:  rebuilt.Status,
				Existing: existingClaim.Status,
			})
		}
	}

	for claimID := range existing.Claims {
		if _, ok := rebuildResult.Doc.Claims[claimID]; !ok {
			mismatches = append(mismatches, VerifyMismatch{
				ClaimID: claimID,
				Issue:   "missing_in_rebuild",
			})
		}
	}

	rebuildResult.DryRun = false

	return &VerifyResult{
		RebuildResult: *rebuildResult,
		Mismatches:    mismatches,
		VerifiedOK:    len(mismatches) == 0,
	}, nil
}

// loadCoordinationDoc reads the coordination state from disk.
func loadCoordinationDoc(stateDir string) (*CRDTDoc, error) {
	path := filepath.Join(stateDir, "coordination", "state.json")
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return &CRDTDoc{Claims: make(map[string]*CRDTClaim)}, nil
		}
		return nil, fmt.Errorf("reading coordination state: %w", err)
	}

	var doc CRDTDoc
	if err := json.Unmarshal(data, &doc); err != nil {
		return nil, fmt.Errorf("parsing coordination state: %w", err)
	}
	if doc.Claims == nil {
		doc.Claims = make(map[string]*CRDTClaim)
	}
	return &doc, nil
}

// saveCoordinationDoc writes the coordination state atomically.
func saveCoordinationDoc(stateDir string, doc *CRDTDoc) error {
	dir := filepath.Join(stateDir, "coordination")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("creating coordination dir: %w", err)
	}

	data, err := json.MarshalIndent(doc, "", "  ")
	if err != nil {
		return fmt.Errorf("marshaling coordination state: %w", err)
	}

	path := filepath.Join(dir, "state.json")
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		return fmt.Errorf("writing temp file: %w", err)
	}

	if err := os.Rename(tmp, path); err != nil {
		_ = os.Remove(tmp)
		return fmt.Errorf("renaming state file: %w", err)
	}

	return nil
}

// backupExistingState creates a timestamped backup of the coordination state
// if it exists. Returns the backup path or empty string if no backup was needed.
func backupExistingState(stateDir string) (string, error) {
	src := filepath.Join(stateDir, "coordination", "state.json")
	if _, err := os.Stat(src); os.IsNotExist(err) {
		return "", nil
	}

	now := time.Now().UTC().Format("20060102T150405Z")
	backup := src + ".bak." + now

	data, err := os.ReadFile(src)
	if err != nil {
		return "", fmt.Errorf("reading existing state for backup: %w", err)
	}

	if err := os.WriteFile(backup, data, 0o644); err != nil {
		return "", fmt.Errorf("writing backup: %w", err)
	}

	return backup, nil
}

// parseTimestamp parses an ISO 8601 timestamp with Z or +00:00 suffix.
func parseTimestamp(s string) (time.Time, error) {
	// Try RFC 3339 first.
	if t, err := time.Parse(time.RFC3339, s); err == nil {
		return t, nil
	}
	// Try with Z suffix replaced.
	normalized := s
	if len(normalized) > 0 && normalized[len(normalized)-1] == 'Z' {
		normalized = normalized[:len(normalized)-1] + "+00:00"
	}
	return time.Parse("2006-01-02T15:04:05+00:00", normalized)
}

// getClaimID extracts the claim ID from the "id" or "claim_id" field.
func getClaimID(raw map[string]any) string {
	if id := getString(raw, "id"); id != "" {
		return id
	}
	return getString(raw, "claim_id")
}

// getInt64 extracts an int64 value from a map, handling both float64 (from JSON)
// and direct int64 values.
func getInt64(m map[string]any, key string) int64 {
	switch v := m[key].(type) {
	case float64:
		return int64(v)
	case int64:
		return v
	case int:
		return int64(v)
	default:
		return 0
	}
}
