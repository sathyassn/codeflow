package db

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// writeJSONL creates a JSONL file from a slice of event maps.
func writeJSONL(t *testing.T, dir string, events []map[string]any) string {
	t.Helper()
	path := filepath.Join(dir, "sessions.jsonl")
	var lines []string
	for _, evt := range events {
		b, err := json.Marshal(evt)
		if err != nil {
			t.Fatalf("marshaling event: %v", err)
		}
		lines = append(lines, string(b))
	}
	if err := os.WriteFile(path, []byte(strings.Join(lines, "\n")+"\n"), 0o644); err != nil {
		t.Fatalf("writing JSONL: %v", err)
	}
	return path
}

func TestRebuildCRDT_FullReplay(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := filepath.Join(dir, ".state")

	events := []map[string]any{
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:00:00Z",
			"id":            "claim_abc",
			"work_id":       "work_001",
			"pattern":       "src/*.go",
			"mode":          "exclusive",
			"owner_id":      "agent-1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
			"created_at":    "2026-01-15T10:00:00Z",
		},
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:01:00Z",
			"id":            "claim_def",
			"work_id":       "work_002",
			"pattern":       "docs/*.md",
			"mode":          "shared",
			"owner_id":      "agent-2",
			"fencing_token": 2,
			"expires_at":    "2026-01-15T10:11:00Z",
			"created_at":    "2026-01-15T10:01:00Z",
		},
		{
			"event":     "claim_released",
			"timestamp": "2026-01-15T10:05:00Z",
			"claim_id":  "claim_abc",
		},
		{
			"event":      "claim_renewed",
			"timestamp":  "2026-01-15T10:08:00Z",
			"claim_id":   "claim_def",
			"expires_at": "2026-01-15T10:20:00Z",
		},
	}

	source := writeJSONL(t, dir, events)

	result, err := RebuildCRDT(RebuildOptions{
		Source:   source,
		StateDir: stateDir,
	})
	if err != nil {
		t.Fatalf("RebuildCRDT() error: %v", err)
	}

	if result.Stats.EventsReplayed != 4 {
		t.Errorf("EventsReplayed = %d, want 4", result.Stats.EventsReplayed)
	}
	if result.Stats.ActiveClaims != 1 {
		t.Errorf("ActiveClaims = %d, want 1", result.Stats.ActiveClaims)
	}
	if result.Stats.ReleasedClaims != 1 {
		t.Errorf("ReleasedClaims = %d, want 1", result.Stats.ReleasedClaims)
	}
	if result.Stats.MaxFencingToken != 2 {
		t.Errorf("MaxFencingToken = %d, want 2", result.Stats.MaxFencingToken)
	}
	if !result.Saved {
		t.Error("Saved = false, want true")
	}

	// Verify claim_abc is released.
	c := result.Doc.Claims["claim_abc"]
	if c == nil {
		t.Fatal("claim_abc not found")
	}
	if c.Status != "released" {
		t.Errorf("claim_abc.Status = %q, want released", c.Status)
	}

	// Verify claim_def is active with renewed expiry.
	d := result.Doc.Claims["claim_def"]
	if d == nil {
		t.Fatal("claim_def not found")
	}
	if d.Status != "active" {
		t.Errorf("claim_def.Status = %q, want active", d.Status)
	}
	if d.ExpiresAt != "2026-01-15T10:20:00Z" {
		t.Errorf("claim_def.ExpiresAt = %q, want 2026-01-15T10:20:00Z", d.ExpiresAt)
	}

	// Verify state file was written.
	statePath := filepath.Join(stateDir, "coordination", "state.json")
	if _, err := os.Stat(statePath); err != nil {
		t.Errorf("state file not created: %v", err)
	}
}

func TestRebuildCRDT_PointInTime(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	events := []map[string]any{
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:00:00Z",
			"id":            "claim_early",
			"work_id":       "work_001",
			"pattern":       "src/*.go",
			"mode":          "exclusive",
			"owner_id":      "agent-1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
		},
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T11:00:00Z",
			"id":            "claim_late",
			"work_id":       "work_002",
			"pattern":       "docs/*.md",
			"mode":          "shared",
			"owner_id":      "agent-2",
			"fencing_token": 2,
			"expires_at":    "2026-01-15T11:10:00Z",
		},
	}

	source := writeJSONL(t, dir, events)

	result, err := RebuildCRDT(RebuildOptions{
		Source: source,
		Until:  "2026-01-15T10:30:00Z",
		DryRun: true,
	})
	if err != nil {
		t.Fatalf("RebuildCRDT() error: %v", err)
	}

	if result.RebuildType != "point_in_time" {
		t.Errorf("RebuildType = %q, want point_in_time", result.RebuildType)
	}

	if result.Stats.EventsReplayed != 1 {
		t.Errorf("EventsReplayed = %d, want 1 (only claim_early before cutoff)", result.Stats.EventsReplayed)
	}

	if _, ok := result.Doc.Claims["claim_early"]; !ok {
		t.Error("claim_early should be in rebuilt doc")
	}
	if _, ok := result.Doc.Claims["claim_late"]; ok {
		t.Error("claim_late should NOT be in rebuilt doc (after cutoff)")
	}
}

func TestRebuildCRDT_DryRun(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	events := []map[string]any{
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:00:00Z",
			"id":            "claim_xyz",
			"work_id":       "work_001",
			"pattern":       "src/*.go",
			"mode":          "exclusive",
			"owner_id":      "agent-1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
		},
	}

	source := writeJSONL(t, dir, events)

	result, err := RebuildCRDT(RebuildOptions{
		Source: source,
		DryRun: true,
	})
	if err != nil {
		t.Fatalf("RebuildCRDT() error: %v", err)
	}

	if !result.DryRun {
		t.Error("DryRun = false, want true")
	}
	if result.Saved {
		t.Error("Saved = true, want false (dry run)")
	}

	// No state file should exist.
	statePath := filepath.Join(dir, ".state", "coordination", "state.json")
	if _, err := os.Stat(statePath); !os.IsNotExist(err) {
		t.Error("state file should not exist in dry run")
	}
}

func TestRebuildCRDT_SourceNotFound(t *testing.T) {
	t.Parallel()
	_, err := RebuildCRDT(RebuildOptions{
		Source: "/nonexistent/sessions.jsonl",
	})
	if err == nil {
		t.Fatal("expected error for nonexistent source")
	}
	if !strings.Contains(err.Error(), "source not found") {
		t.Errorf("error = %q, want to contain 'source not found'", err.Error())
	}
}

func TestRebuildCRDT_EmptySource(t *testing.T) {
	t.Parallel()
	_, err := RebuildCRDT(RebuildOptions{})
	if err == nil {
		t.Fatal("expected error for empty source")
	}
	if !strings.Contains(err.Error(), "source path is required") {
		t.Errorf("error = %q, want to contain 'source path is required'", err.Error())
	}
}

func TestRebuildCRDT_Backup(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := filepath.Join(dir, ".state")

	// Create existing state.
	coordDir := filepath.Join(stateDir, "coordination")
	if err := os.MkdirAll(coordDir, 0o755); err != nil {
		t.Fatal(err)
	}
	existing := `{"claims":{"old_claim":{"id":"old_claim","status":"active"}},"token_counter":5}`
	if err := os.WriteFile(filepath.Join(coordDir, "state.json"), []byte(existing), 0o644); err != nil {
		t.Fatal(err)
	}

	events := []map[string]any{
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:00:00Z",
			"id":            "new_claim",
			"work_id":       "work_001",
			"pattern":       "src/*.go",
			"mode":          "exclusive",
			"owner_id":      "agent-1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
		},
	}

	source := writeJSONL(t, dir, events)

	result, err := RebuildCRDT(RebuildOptions{
		Source:   source,
		StateDir: stateDir,
	})
	if err != nil {
		t.Fatalf("RebuildCRDT() error: %v", err)
	}

	if result.BackupPath == "" {
		t.Error("BackupPath should not be empty when existing state exists")
	}

	// Verify backup file was created.
	if _, err := os.Stat(result.BackupPath); err != nil {
		t.Errorf("backup file not found: %v", err)
	}

	// Verify backup contains original content.
	backupData, err := os.ReadFile(result.BackupPath)
	if err != nil {
		t.Fatalf("reading backup: %v", err)
	}
	if !strings.Contains(string(backupData), "old_claim") {
		t.Error("backup should contain original state with old_claim")
	}
}

func TestRebuildCRDT_NormalizedEventTypes(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	// Test with "type" key instead of "event" key (Pattern 2 normalization).
	events := []map[string]any{
		{
			"type":          "claim_created",
			"ts":            "2026-01-15T10:00:00Z",
			"id":            "claim_norm",
			"work_id":       "work_001",
			"pattern":       "src/*.go",
			"mode":          "exclusive",
			"owner_id":      "agent-1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
		},
	}

	source := writeJSONL(t, dir, events)

	result, err := RebuildCRDT(RebuildOptions{
		Source: source,
		DryRun: true,
	})
	if err != nil {
		t.Fatalf("RebuildCRDT() error: %v", err)
	}

	if result.Stats.EventsReplayed != 1 {
		t.Errorf("EventsReplayed = %d, want 1 (normalized 'type' key)", result.Stats.EventsReplayed)
	}

	if _, ok := result.Doc.Claims["claim_norm"]; !ok {
		t.Error("claim_norm should be in rebuilt doc")
	}
}

func TestRebuildCRDT_SkipsNonClaimEvents(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	events := []map[string]any{
		{
			"event":     "session_start",
			"timestamp": "2026-01-15T10:00:00Z",
			"session_id": "ses-abc",
		},
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:01:00Z",
			"id":            "claim_only",
			"work_id":       "work_001",
			"pattern":       "src/*.go",
			"mode":          "exclusive",
			"owner_id":      "agent-1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
		},
		{
			"event":      "session_end",
			"timestamp":  "2026-01-15T11:00:00Z",
			"session_id": "ses-abc",
		},
	}

	source := writeJSONL(t, dir, events)

	result, err := RebuildCRDT(RebuildOptions{
		Source: source,
		DryRun: true,
	})
	if err != nil {
		t.Fatalf("RebuildCRDT() error: %v", err)
	}

	if result.Stats.EventsReplayed != 1 {
		t.Errorf("EventsReplayed = %d, want 1 (only claim events counted)", result.Stats.EventsReplayed)
	}
}

func TestVerifyCRDT_Match(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := filepath.Join(dir, ".state")

	events := []map[string]any{
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:00:00Z",
			"id":            "claim_ver",
			"work_id":       "work_001",
			"pattern":       "src/*.go",
			"mode":          "exclusive",
			"owner_id":      "agent-1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
			"created_at":    "2026-01-15T10:00:00Z",
		},
	}

	source := writeJSONL(t, dir, events)

	// Create existing state that matches.
	coordDir := filepath.Join(stateDir, "coordination")
	if err := os.MkdirAll(coordDir, 0o755); err != nil {
		t.Fatal(err)
	}
	existingDoc := CRDTDoc{
		Claims: map[string]*CRDTClaim{
			"claim_ver": {
				ID:     "claim_ver",
				Status: "active",
			},
		},
		TokenCounter: 1,
	}
	data, _ := json.Marshal(existingDoc)
	if err := os.WriteFile(filepath.Join(coordDir, "state.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	result, err := VerifyCRDT(RebuildOptions{
		Source:   source,
		StateDir: stateDir,
	})
	if err != nil {
		t.Fatalf("VerifyCRDT() error: %v", err)
	}

	if !result.VerifiedOK {
		t.Errorf("VerifiedOK = false, want true; mismatches: %v", result.Mismatches)
	}
}

func TestVerifyCRDT_Mismatch(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := filepath.Join(dir, ".state")

	events := []map[string]any{
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:00:00Z",
			"id":            "claim_mis",
			"work_id":       "work_001",
			"pattern":       "src/*.go",
			"mode":          "exclusive",
			"owner_id":      "agent-1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
		},
		{
			"event":     "claim_released",
			"timestamp": "2026-01-15T10:05:00Z",
			"claim_id":  "claim_mis",
		},
	}

	source := writeJSONL(t, dir, events)

	// Create existing state with wrong status (active instead of released).
	coordDir := filepath.Join(stateDir, "coordination")
	if err := os.MkdirAll(coordDir, 0o755); err != nil {
		t.Fatal(err)
	}
	existingDoc := CRDTDoc{
		Claims: map[string]*CRDTClaim{
			"claim_mis": {
				ID:     "claim_mis",
				Status: "active", // Should be "released" per JSONL.
			},
		},
		TokenCounter: 1,
	}
	data, _ := json.Marshal(existingDoc)
	if err := os.WriteFile(filepath.Join(coordDir, "state.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	result, err := VerifyCRDT(RebuildOptions{
		Source:   source,
		StateDir: stateDir,
	})
	if err != nil {
		t.Fatalf("VerifyCRDT() error: %v", err)
	}

	if result.VerifiedOK {
		t.Error("VerifiedOK = true, want false (status mismatch)")
	}

	if len(result.Mismatches) == 0 {
		t.Fatal("expected at least one mismatch")
	}

	found := false
	for _, m := range result.Mismatches {
		if m.ClaimID == "claim_mis" && m.Issue == "status_mismatch" {
			found = true
			if m.Rebuilt != "released" || m.Existing != "active" {
				t.Errorf("mismatch values: rebuilt=%q, existing=%q", m.Rebuilt, m.Existing)
			}
		}
	}
	if !found {
		t.Error("expected status_mismatch for claim_mis")
	}
}

func TestVerifyCRDT_MissingInExisting(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := filepath.Join(dir, ".state")

	events := []map[string]any{
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:00:00Z",
			"id":            "claim_new",
			"work_id":       "work_001",
			"pattern":       "src/*.go",
			"mode":          "exclusive",
			"owner_id":      "agent-1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
		},
	}

	source := writeJSONL(t, dir, events)

	// Create empty existing state.
	coordDir := filepath.Join(stateDir, "coordination")
	if err := os.MkdirAll(coordDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(coordDir, "state.json"), []byte(`{"claims":{},"token_counter":0}`), 0o644); err != nil {
		t.Fatal(err)
	}

	result, err := VerifyCRDT(RebuildOptions{
		Source:   source,
		StateDir: stateDir,
	})
	if err != nil {
		t.Fatalf("VerifyCRDT() error: %v", err)
	}

	if result.VerifiedOK {
		t.Error("VerifiedOK = true, want false (missing in existing)")
	}

	found := false
	for _, m := range result.Mismatches {
		if m.ClaimID == "claim_new" && m.Issue == "missing_in_existing" {
			found = true
		}
	}
	if !found {
		t.Error("expected missing_in_existing for claim_new")
	}
}

func TestRebuildCRDT_EmptyJSONL(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	source := filepath.Join(dir, "sessions.jsonl")
	if err := os.WriteFile(source, []byte(""), 0o644); err != nil {
		t.Fatal(err)
	}

	result, err := RebuildCRDT(RebuildOptions{
		Source: source,
		DryRun: true,
	})
	if err != nil {
		t.Fatalf("RebuildCRDT() error: %v", err)
	}

	if result.Stats.EventsReplayed != 0 {
		t.Errorf("EventsReplayed = %d, want 0", result.Stats.EventsReplayed)
	}
	if len(result.Doc.Claims) != 0 {
		t.Errorf("Claims count = %d, want 0", len(result.Doc.Claims))
	}
}

func TestParseTimestamp(t *testing.T) {
	t.Parallel()
	tests := []struct {
		name    string
		input   string
		wantErr bool
	}{
		{"RFC3339 Z", "2026-01-15T10:00:00Z", false},
		{"RFC3339 offset", "2026-01-15T10:00:00+00:00", false},
		{"invalid", "not-a-timestamp", true},
		{"empty", "", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			_, err := parseTimestamp(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("parseTimestamp(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
			}
		})
	}
}

func TestGetClaimID(t *testing.T) {
	t.Parallel()
	tests := []struct {
		name string
		raw  map[string]any
		want string
	}{
		{"id field", map[string]any{"id": "claim_abc"}, "claim_abc"},
		{"claim_id field", map[string]any{"claim_id": "claim_def"}, "claim_def"},
		{"both prefers id", map[string]any{"id": "claim_abc", "claim_id": "claim_def"}, "claim_abc"},
		{"neither", map[string]any{"other": "value"}, ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := getClaimID(tt.raw)
			if got != tt.want {
				t.Errorf("getClaimID() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestGetInt64(t *testing.T) {
	t.Parallel()
	tests := []struct {
		name string
		raw  map[string]any
		want int64
	}{
		{"float64", map[string]any{"n": float64(42)}, 42},
		{"int64", map[string]any{"n": int64(99)}, 99},
		{"int", map[string]any{"n": int(7)}, 7},
		{"string", map[string]any{"n": "not-a-number"}, 0},
		{"missing", map[string]any{}, 0},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := getInt64(tt.raw, "n")
			if got != tt.want {
				t.Errorf("getInt64() = %d, want %d", got, tt.want)
			}
		})
	}
}

func TestLoadCoordinationDoc_NotExist(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	doc, err := loadCoordinationDoc(dir)
	if err != nil {
		t.Fatalf("loadCoordinationDoc() error: %v", err)
	}
	if doc == nil {
		t.Fatal("doc should not be nil")
	}
	if len(doc.Claims) != 0 {
		t.Errorf("Claims count = %d, want 0", len(doc.Claims))
	}
}

func TestLoadCoordinationDoc_CorruptJSON(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	coordDir := filepath.Join(dir, "coordination")
	if err := os.MkdirAll(coordDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(coordDir, "state.json"), []byte("not valid json"), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := loadCoordinationDoc(dir)
	if err == nil {
		t.Fatal("expected error for corrupt JSON")
	}
	if !strings.Contains(err.Error(), "parsing coordination state") {
		t.Errorf("error = %q, want to contain 'parsing coordination state'", err.Error())
	}
}

func TestLoadCoordinationDoc_NilClaims(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	coordDir := filepath.Join(dir, "coordination")
	if err := os.MkdirAll(coordDir, 0o755); err != nil {
		t.Fatal(err)
	}
	// JSON with null claims field.
	if err := os.WriteFile(filepath.Join(coordDir, "state.json"), []byte(`{"claims":null,"token_counter":0}`), 0o644); err != nil {
		t.Fatal(err)
	}

	doc, err := loadCoordinationDoc(dir)
	if err != nil {
		t.Fatalf("loadCoordinationDoc() error: %v", err)
	}
	if doc.Claims == nil {
		t.Error("Claims map should be initialized, not nil")
	}
}

func TestLoadCoordinationDoc_ReadError(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	coordDir := filepath.Join(dir, "coordination")
	if err := os.MkdirAll(coordDir, 0o755); err != nil {
		t.Fatal(err)
	}
	// Create state.json as a directory to trigger a read error.
	if err := os.Mkdir(filepath.Join(coordDir, "state.json"), 0o755); err != nil {
		t.Fatal(err)
	}

	_, err := loadCoordinationDoc(dir)
	if err == nil {
		t.Fatal("expected error for directory-as-file read")
	}
	if !strings.Contains(err.Error(), "reading coordination state") {
		t.Errorf("error = %q, want to contain 'reading coordination state'", err.Error())
	}
}

func TestSaveCoordinationDoc_Success(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	doc := &CRDTDoc{
		Claims: map[string]*CRDTClaim{
			"c1": {ID: "c1", Status: "active", Pattern: "*.go"},
		},
		TokenCounter: 5,
	}

	if err := saveCoordinationDoc(dir, doc); err != nil {
		t.Fatalf("saveCoordinationDoc() error: %v", err)
	}

	// Verify file exists and is valid JSON.
	loaded, err := loadCoordinationDoc(dir)
	if err != nil {
		t.Fatalf("loadCoordinationDoc() error: %v", err)
	}
	if loaded.TokenCounter != 5 {
		t.Errorf("TokenCounter = %d, want 5", loaded.TokenCounter)
	}
	if loaded.Claims["c1"] == nil {
		t.Error("claim c1 should exist")
	}
}

func TestRebuildCRDT_InvalidUntilTimestamp(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	source := filepath.Join(dir, "sessions.jsonl")
	if err := os.WriteFile(source, []byte(""), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := RebuildCRDT(RebuildOptions{
		Source: source,
		Until:  "not-a-timestamp",
	})
	if err == nil {
		t.Fatal("expected error for invalid --until timestamp")
	}
	if !strings.Contains(err.Error(), "parsing --until timestamp") {
		t.Errorf("error = %q, want to contain 'parsing --until timestamp'", err.Error())
	}
}

func TestRebuildCRDT_VerifyPathRedirects(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	source := filepath.Join(dir, "sessions.jsonl")
	if err := os.WriteFile(source, []byte(""), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := RebuildCRDT(RebuildOptions{
		Source:   source,
		Verify:   true,
		StateDir: dir,
	})
	if err == nil {
		t.Fatal("expected error redirecting to VerifyCRDT")
	}
	if !strings.Contains(err.Error(), "use VerifyCRDT") {
		t.Errorf("error = %q, want to contain 'use VerifyCRDT'", err.Error())
	}
}

func TestRebuildCRDT_NoStateDirForSave(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	events := []map[string]any{
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:00:00Z",
			"id":            "claim_x",
			"work_id":       "w1",
			"pattern":       "*.go",
			"mode":          "exclusive",
			"owner_id":      "a1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
		},
	}
	source := writeJSONL(t, dir, events)

	_, err := RebuildCRDT(RebuildOptions{
		Source:   source,
		StateDir: "", // No state dir.
	})
	if err == nil {
		t.Fatal("expected error when state directory is empty")
	}
	if !strings.Contains(err.Error(), "state directory is required") {
		t.Errorf("error = %q, want to contain 'state directory is required'", err.Error())
	}
}

func TestBackupExistingState_NoExistingFile(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	path, err := backupExistingState(dir)
	if err != nil {
		t.Fatalf("backupExistingState() error: %v", err)
	}
	if path != "" {
		t.Errorf("path = %q, want empty (no existing file)", path)
	}
}

func TestVerifyCRDT_NoExistingState(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	events := []map[string]any{
		{
			"event":         "claim_created",
			"timestamp":     "2026-01-15T10:00:00Z",
			"id":            "claim_v2",
			"work_id":       "w1",
			"pattern":       "*.go",
			"mode":          "exclusive",
			"owner_id":      "a1",
			"fencing_token": 1,
			"expires_at":    "2026-01-15T10:10:00Z",
		},
	}
	source := writeJSONL(t, dir, events)

	result, err := VerifyCRDT(RebuildOptions{
		Source:   source,
		StateDir: dir, // No existing state file.
	})
	if err != nil {
		t.Fatalf("VerifyCRDT() error: %v", err)
	}

	// Rebuilt has claim_v2, existing has nothing -> missing_in_existing.
	if result.VerifiedOK {
		t.Error("VerifiedOK = true, want false (claim missing in existing)")
	}
	if len(result.Mismatches) != 1 {
		t.Errorf("Mismatches count = %d, want 1", len(result.Mismatches))
	}
}

func TestRebuildCRDT_SkipsMissingClaimIDs(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	// Events with missing claim IDs should be silently skipped.
	events := []map[string]any{
		{
			"event":     "claim_created",
			"timestamp": "2026-01-15T10:00:00Z",
			// No "id" or "claim_id" field.
			"work_id": "w1",
			"pattern": "*.go",
		},
		{
			"event":     "claim_released",
			"timestamp": "2026-01-15T10:01:00Z",
			// No "claim_id" field.
		},
		{
			"event":     "claim_renewed",
			"timestamp": "2026-01-15T10:02:00Z",
			// No "claim_id" field.
		},
		{
			"event":     "claim_released",
			"timestamp": "2026-01-15T10:03:00Z",
			"claim_id":  "nonexistent_claim",
		},
		{
			"event":     "claim_renewed",
			"timestamp": "2026-01-15T10:04:00Z",
			"claim_id":  "nonexistent_claim",
		},
	}
	source := writeJSONL(t, dir, events)

	result, err := RebuildCRDT(RebuildOptions{
		Source: source,
		DryRun: true,
	})
	if err != nil {
		t.Fatalf("RebuildCRDT() error: %v", err)
	}

	if result.Stats.EventsReplayed != 0 {
		t.Errorf("EventsReplayed = %d, want 0 (all skipped)", result.Stats.EventsReplayed)
	}
	if len(result.Doc.Claims) != 0 {
		t.Errorf("Claims count = %d, want 0", len(result.Doc.Claims))
	}
}
