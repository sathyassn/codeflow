package claim

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/codeflow/codeflow-cli/internal/ledger"
)

// testManager creates a Manager with a fixed clock and temp directories.
func testManager(t *testing.T, now time.Time) *Manager {
	t.Helper()
	dir := t.TempDir()
	stateDir := filepath.Join(dir, ".state")
	ledgerDir := filepath.Join(dir, "ledger")
	w, err := ledger.NewWriter(ledgerDir)
	if err != nil {
		t.Fatalf("NewWriter: %v", err)
	}
	mgr := NewManager(stateDir, w)
	mgr.Now = func() time.Time { return now }
	return mgr
}

func TestAcquire_Success(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	c, err := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)
	if err != nil {
		t.Fatalf("Acquire: %v", err)
	}

	if !strings.HasPrefix(c.ID, "claim_") {
		t.Errorf("claim ID = %q, want prefix 'claim_'", c.ID)
	}
	if c.WorkID != "task-001" {
		t.Errorf("WorkID = %q, want %q", c.WorkID, "task-001")
	}
	if c.Pattern != "src/**" {
		t.Errorf("Pattern = %q, want %q", c.Pattern, "src/**")
	}
	if c.Mode != "exclusive" {
		t.Errorf("Mode = %q, want %q", c.Mode, "exclusive")
	}
	if c.FencingToken != 1 {
		t.Errorf("FencingToken = %d, want 1", c.FencingToken)
	}
	if c.Status != "active" {
		t.Errorf("Status = %q, want %q", c.Status, "active")
	}
}

func TestAcquire_ConflictExclusive(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, err := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)
	if err != nil {
		t.Fatalf("first Acquire: %v", err)
	}

	_, err = mgr.Acquire("task-002", "src/**", "agent-2", "exclusive", 600)
	if err == nil {
		t.Fatal("second Acquire expected conflict error")
	}
	if !strings.Contains(err.Error(), "conflict") {
		t.Errorf("error = %q, want to contain 'conflict'", err.Error())
	}
}

func TestAcquire_SharedNoConflict(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, err := mgr.Acquire("task-001", "src/**", "agent-1", "shared", 600)
	if err != nil {
		t.Fatalf("first Acquire: %v", err)
	}

	c, err := mgr.Acquire("task-002", "src/**", "agent-2", "shared", 600)
	if err != nil {
		t.Fatalf("second Acquire (shared) should not conflict: %v", err)
	}
	if c.FencingToken != 2 {
		t.Errorf("FencingToken = %d, want 2", c.FencingToken)
	}
}

func TestAcquire_SharedVsExclusiveConflict(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, err := mgr.Acquire("task-001", "src/**", "agent-1", "shared", 600)
	if err != nil {
		t.Fatalf("first Acquire: %v", err)
	}

	_, err = mgr.Acquire("task-002", "src/**", "agent-2", "exclusive", 600)
	if err == nil {
		t.Fatal("exclusive vs shared should conflict")
	}
}

func TestAcquire_ExpiredClaimAllowsNew(t *testing.T) {
	t.Parallel()
	past := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, past)

	_, err := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 60)
	if err != nil {
		t.Fatalf("first Acquire: %v", err)
	}

	// Advance time past expiration.
	future := past.Add(120 * time.Second)
	mgr.Now = func() time.Time { return future }

	c, err := mgr.Acquire("task-002", "src/**", "agent-2", "exclusive", 600)
	if err != nil {
		t.Fatalf("Acquire after expiry should succeed: %v", err)
	}
	if c.OwnerID != "agent-2" {
		t.Errorf("OwnerID = %q, want %q", c.OwnerID, "agent-2")
	}
}

func TestAcquire_LedgerEvent(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, err := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)
	if err != nil {
		t.Fatalf("Acquire: %v", err)
	}

	// Verify ledger event was written.
	data, err := os.ReadFile(filepath.Join(mgr.LedgerWriter.Dir(), ledger.FileSessions))
	if err != nil {
		t.Fatalf("reading sessions.jsonl: %v", err)
	}
	if !strings.Contains(string(data), "claim_created") {
		t.Error("sessions.jsonl should contain claim_created event")
	}
}

func TestCheck_NoConflict(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	result, err := mgr.Check("src/auth.ts", "", false)
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if result.Claimed {
		t.Error("expected no claims for empty state")
	}
	if !result.CanProceed {
		t.Error("expected can_proceed=true")
	}
}

func TestCheck_BlockExclusive(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, err := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)
	if err != nil {
		t.Fatalf("Acquire: %v", err)
	}

	result, err := mgr.Check("src/**", "agent-2", false)
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if !result.Claimed {
		t.Error("expected claimed=true")
	}
	if result.CanProceed {
		t.Error("expected can_proceed=false for exclusive conflict")
	}
	if len(result.Claims) != 1 {
		t.Fatalf("expected 1 claim, got %d", len(result.Claims))
	}
	if result.Claims[0].Action != "BLOCK" {
		t.Errorf("action = %q, want BLOCK", result.Claims[0].Action)
	}
}

func TestCheck_AllowOwn(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, err := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)
	if err != nil {
		t.Fatalf("Acquire: %v", err)
	}

	result, err := mgr.Check("src/**", "agent-1", false)
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if !result.CanProceed {
		t.Error("expected can_proceed=true for own claim")
	}
	if len(result.Claims) != 1 {
		t.Fatalf("expected 1 claim, got %d", len(result.Claims))
	}
	if result.Claims[0].Action != "ALLOW" {
		t.Errorf("action = %q, want ALLOW", result.Claims[0].Action)
	}
	if !result.Claims[0].IsOwn {
		t.Error("expected is_own=true")
	}
}

func TestCheck_WarnExpired(t *testing.T) {
	t.Parallel()
	past := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, past)

	_, err := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 60)
	if err != nil {
		t.Fatalf("Acquire: %v", err)
	}

	future := past.Add(120 * time.Second)
	mgr.Now = func() time.Time { return future }

	result, err := mgr.Check("src/**", "agent-2", true)
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(result.Claims) != 1 {
		t.Fatalf("expected 1 claim with include-expired, got %d", len(result.Claims))
	}
	if result.Claims[0].Action != "WARN" {
		t.Errorf("action = %q, want WARN for expired claim", result.Claims[0].Action)
	}
}

func TestList_Empty(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	claims, err := mgr.List(ListOptions{})
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(claims) != 0 {
		t.Errorf("expected 0 claims, got %d", len(claims))
	}
}

func TestList_WithFilters(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, _ = mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)
	_, _ = mgr.Acquire("task-002", "docs/**", "agent-2", "shared", 600)

	// Filter by owner.
	claims, err := mgr.List(ListOptions{OwnerID: "agent-1"})
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(claims) != 1 {
		t.Errorf("expected 1 claim for agent-1, got %d", len(claims))
	}

	// All claims.
	claims, err = mgr.List(ListOptions{Status: "all"})
	if err != nil {
		t.Fatalf("List all: %v", err)
	}
	if len(claims) != 2 {
		t.Errorf("expected 2 claims, got %d", len(claims))
	}
}

func TestList_EffectiveExpired(t *testing.T) {
	t.Parallel()
	past := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, past)

	_, _ = mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 60)

	future := past.Add(120 * time.Second)
	mgr.Now = func() time.Time { return future }

	// Active filter should return 0 (expired).
	claims, err := mgr.List(ListOptions{})
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(claims) != 0 {
		t.Errorf("expected 0 active claims (expired), got %d", len(claims))
	}

	// Include expired.
	claims, err = mgr.List(ListOptions{IncludeExpired: true})
	if err != nil {
		t.Fatalf("List expired: %v", err)
	}
	if len(claims) != 1 {
		t.Errorf("expected 1 claim with include-expired, got %d", len(claims))
	}
	if claims[0].EffectiveStatus != "expired" {
		t.Errorf("effective_status = %q, want 'expired'", claims[0].EffectiveStatus)
	}
}

func TestRelease_ByID(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	c, _ := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)

	released, err := mgr.Release(c.ID, "", "work completed")
	if err != nil {
		t.Fatalf("Release: %v", err)
	}
	if released.Status != "released" {
		t.Errorf("Status = %q, want 'released'", released.Status)
	}
	if released.ReleasedAt == "" {
		t.Error("ReleasedAt should be set")
	}
}

func TestRelease_ByPattern(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, _ = mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)

	released, err := mgr.Release("", "src/**", "")
	if err != nil {
		t.Fatalf("Release by pattern: %v", err)
	}
	if released.Status != "released" {
		t.Errorf("Status = %q, want 'released'", released.Status)
	}
}

func TestRelease_NotFound(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, err := mgr.Release("claim_nonexistent", "", "")
	if err == nil {
		t.Fatal("expected error for non-existent claim")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Errorf("error = %q, want to contain 'not found'", err.Error())
	}
}

func TestRelease_AlreadyReleased(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	c, _ := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)
	_, _ = mgr.Release(c.ID, "", "")

	_, err := mgr.Release(c.ID, "", "")
	if err == nil {
		t.Fatal("expected error for already-released claim")
	}
	if !strings.Contains(err.Error(), "already released") {
		t.Errorf("error = %q, want to contain 'already released'", err.Error())
	}
}

func TestRelease_LedgerEvent(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	c, _ := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)
	_, _ = mgr.Release(c.ID, "", "done")

	data, err := os.ReadFile(filepath.Join(mgr.LedgerWriter.Dir(), ledger.FileSessions))
	if err != nil {
		t.Fatalf("reading sessions.jsonl: %v", err)
	}
	if !strings.Contains(string(data), "claim_released") {
		t.Error("sessions.jsonl should contain claim_released event")
	}
}

func TestRenew_Active(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	c, _ := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 60)

	result, err := mgr.Renew(c.ID, 1200)
	if err != nil {
		t.Fatalf("Renew: %v", err)
	}
	if result.NewExpiresAt == result.PreviousExpiresAt {
		t.Error("new expires should differ from previous")
	}
	expected := formatUTC(now.Add(1200 * time.Second))
	if result.NewExpiresAt != expected {
		t.Errorf("NewExpiresAt = %q, want %q", result.NewExpiresAt, expected)
	}
}

func TestRenew_Expired(t *testing.T) {
	t.Parallel()
	past := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, past)

	c, _ := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 60)

	future := past.Add(120 * time.Second)
	mgr.Now = func() time.Time { return future }

	_, err := mgr.Renew(c.ID, 600)
	if err == nil {
		t.Fatal("expected error renewing expired claim")
	}
	if !strings.Contains(err.Error(), "expired") {
		t.Errorf("error = %q, want to contain 'expired'", err.Error())
	}
}

func TestRenew_NotFound(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, err := mgr.Renew("claim_nonexistent", 600)
	if err == nil {
		t.Fatal("expected error for non-existent claim")
	}
}

func TestRenew_Released(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	c, _ := mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)
	_, _ = mgr.Release(c.ID, "", "")

	_, err := mgr.Renew(c.ID, 600)
	if err == nil {
		t.Fatal("expected error renewing released claim")
	}
	if !strings.Contains(err.Error(), "not active") {
		t.Errorf("error = %q, want to contain 'not active'", err.Error())
	}
}

func TestRenewAll(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, _ = mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)
	_, _ = mgr.Acquire("task-002", "docs/**", "agent-2", "shared", 600)

	results, err := mgr.RenewAll(1200)
	if err != nil {
		t.Fatalf("RenewAll: %v", err)
	}
	if len(results) != 2 {
		t.Errorf("expected 2 renewals, got %d", len(results))
	}
}

func TestPatternsConflict(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		a, b     string
		conflict bool
	}{
		{"exact match", "src/auth.ts", "src/auth.ts", true},
		{"glob match a->b", "src/auth.ts", "src/*", true},
		{"glob match b->a", "src/*", "src/auth.ts", true},
		{"no match", "src/auth.ts", "docs/readme.md", false},
		{"dir containment file-in-dir", "file:src/auth.ts", "dir:src", true},
		{"dir containment dir-contains-file", "dir:src", "file:src/auth.ts", true},
		{"dir containment nested", "dir:src", "dir:src/models", true},
		{"no dir containment", "dir:src", "dir:docs", false},
		{"double star", "src/**", "src/**", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := patternsConflict(tt.a, tt.b)
			if got != tt.conflict {
				t.Errorf("patternsConflict(%q, %q) = %v, want %v", tt.a, tt.b, got, tt.conflict)
			}
		})
	}
}

func TestCoordinationDoc_Persistence(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
	mgr := testManager(t, now)

	_, _ = mgr.Acquire("task-001", "src/**", "agent-1", "exclusive", 600)

	// Read the state file directly and verify JSON structure.
	data, err := os.ReadFile(mgr.stateFilePath())
	if err != nil {
		t.Fatalf("reading state file: %v", err)
	}

	var doc CoordinationDoc
	if err := json.Unmarshal(data, &doc); err != nil {
		t.Fatalf("parsing state: %v", err)
	}
	if doc.TokenCounter != 1 {
		t.Errorf("TokenCounter = %d, want 1", doc.TokenCounter)
	}
	if len(doc.Claims) != 1 {
		t.Errorf("expected 1 claim, got %d", len(doc.Claims))
	}
}

func TestParseExpiry(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		input   string
		wantErr bool
	}{
		{"Z suffix", "2026-03-01T12:00:00Z", false},
		{"offset suffix", "2026-03-01T12:00:00+00:00", false},
		{"invalid", "not-a-date", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			_, err := parseExpiry(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("parseExpiry(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
			}
		})
	}
}

func TestClassifyConflict(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		claim     *Claim
		ownerID   string
		isExpired bool
		want      string
	}{
		{"own claim", &Claim{OwnerID: "agent-1", Mode: "exclusive"}, "agent-1", false, "ALLOW"},
		{"expired", &Claim{OwnerID: "agent-1", Mode: "exclusive"}, "agent-2", true, "WARN"},
		{"exclusive other", &Claim{OwnerID: "agent-1", Mode: "exclusive"}, "agent-2", false, "BLOCK"},
		{"shared other", &Claim{OwnerID: "agent-1", Mode: "shared"}, "agent-2", false, "WARN"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := classifyConflict(tt.claim, tt.ownerID, tt.isExpired)
			if got != tt.want {
				t.Errorf("classifyConflict() = %q, want %q", got, tt.want)
			}
		})
	}
}
