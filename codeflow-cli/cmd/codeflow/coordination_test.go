package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCoordinationCmd_Help(t *testing.T) {
	t.Parallel()
	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{"coordination", "--help"})

	if err := root.Execute(); err != nil {
		t.Fatalf("coordination --help error: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "claim-acquire") {
		t.Error("help should mention claim-acquire")
	}
	if !strings.Contains(output, "crdt-rebuild") {
		t.Error("help should mention crdt-rebuild")
	}
}

func TestClaimAcquireCmd(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := dir
	ledgerDir := filepath.Join(dir, "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatal(err)
	}

	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{
		"coordination", "claim-acquire",
		"--state", stateDir,
		"--ledger", ledgerDir,
		"--work-id", "work_001",
		"--pattern", "src/*.go",
		"--owner", "agent-1",
		"--mode", "exclusive",
		"--ttl", "300",
	})

	if err := root.Execute(); err != nil {
		t.Fatalf("claim-acquire error: %v", err)
	}

	var result map[string]any
	if err := json.Unmarshal(buf.Bytes(), &result); err != nil {
		t.Fatalf("parsing JSON: %v; output: %s", err, buf.String())
	}

	if result["status"] != "active" {
		t.Errorf("status = %v, want active", result["status"])
	}
	if result["pattern"] != "src/*.go" {
		t.Errorf("pattern = %v, want src/*.go", result["pattern"])
	}
	if result["mode"] != "exclusive" {
		t.Errorf("mode = %v, want exclusive", result["mode"])
	}

	// Verify state file was written.
	statePath := filepath.Join(stateDir, "coordination", "state.json")
	if _, err := os.Stat(statePath); err != nil {
		t.Errorf("state file not created: %v", err)
	}

	// Verify ledger event was written.
	ledgerPath := filepath.Join(ledgerDir, "sessions.jsonl")
	data, err := os.ReadFile(ledgerPath)
	if err != nil {
		t.Fatalf("reading ledger: %v", err)
	}
	if !strings.Contains(string(data), "claim_created") {
		t.Error("ledger should contain claim_created event")
	}
}

func TestClaimAcquireCmd_Conflict(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := dir
	ledgerDir := filepath.Join(dir, "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// First acquire succeeds.
	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{
		"coordination", "claim-acquire",
		"--state", stateDir,
		"--ledger", ledgerDir,
		"--work-id", "work_001",
		"--pattern", "src/*.go",
		"--owner", "agent-1",
	})
	if err := root.Execute(); err != nil {
		t.Fatalf("first acquire error: %v", err)
	}

	// Second acquire on same pattern should fail.
	root2 := newRootCmd()
	buf2 := new(bytes.Buffer)
	root2.SetOut(buf2)
	root2.SetErr(buf2)
	root2.SetArgs([]string{
		"coordination", "claim-acquire",
		"--state", stateDir,
		"--ledger", ledgerDir,
		"--work-id", "work_002",
		"--pattern", "src/*.go",
		"--owner", "agent-2",
	})
	err := root2.Execute()
	if err == nil {
		t.Fatal("expected conflict error on second acquire")
	}
	if !strings.Contains(err.Error(), "conflict") {
		t.Errorf("error = %q, want to contain 'conflict'", err.Error())
	}
}

func TestClaimCheckCmd(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := dir
	ledgerDir := filepath.Join(dir, "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create a claim first.
	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{
		"coordination", "claim-acquire",
		"--state", stateDir,
		"--ledger", ledgerDir,
		"--work-id", "work_001",
		"--pattern", "src/*.go",
		"--owner", "agent-1",
	})
	if err := root.Execute(); err != nil {
		t.Fatalf("acquire error: %v", err)
	}

	// Check for conflict.
	root2 := newRootCmd()
	buf2 := new(bytes.Buffer)
	root2.SetOut(buf2)
	root2.SetErr(buf2)
	root2.SetArgs([]string{
		"coordination", "claim-check",
		"--state", stateDir,
		"--value", "src/*.go",
		"--owner", "agent-2",
	})
	err := root2.Execute()
	if err == nil {
		t.Fatal("expected blocked error")
	}

	var result map[string]any
	if err := json.Unmarshal(buf2.Bytes(), &result); err != nil {
		t.Fatalf("parsing JSON: %v; output: %s", err, buf2.String())
	}
	if result["can_proceed"] != false {
		t.Errorf("can_proceed = %v, want false", result["can_proceed"])
	}
}

func TestClaimListCmd_Empty(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{
		"coordination", "claim-list",
		"--state", dir,
	})

	if err := root.Execute(); err != nil {
		t.Fatalf("claim-list error: %v", err)
	}

	// Should output null or empty array.
	output := strings.TrimSpace(buf.String())
	if output != "null" && output != "[]" {
		t.Errorf("expected null or empty array, got: %s", output)
	}
}

func TestClaimReleaseCmd(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := dir
	ledgerDir := filepath.Join(dir, "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Acquire a claim.
	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{
		"coordination", "claim-acquire",
		"--state", stateDir,
		"--ledger", ledgerDir,
		"--work-id", "work_001",
		"--pattern", "src/*.go",
		"--owner", "agent-1",
	})
	if err := root.Execute(); err != nil {
		t.Fatalf("acquire error: %v", err)
	}

	var acquired map[string]any
	if err := json.Unmarshal(buf.Bytes(), &acquired); err != nil {
		t.Fatalf("parsing acquired: %v", err)
	}
	claimID := acquired["id"].(string)

	// Release by ID.
	root2 := newRootCmd()
	buf2 := new(bytes.Buffer)
	root2.SetOut(buf2)
	root2.SetErr(buf2)
	root2.SetArgs([]string{
		"coordination", "claim-release",
		"--state", stateDir,
		"--ledger", ledgerDir,
		"--claim-id", claimID,
		"--reason", "work complete",
	})
	if err := root2.Execute(); err != nil {
		t.Fatalf("release error: %v", err)
	}

	var released map[string]any
	if err := json.Unmarshal(buf2.Bytes(), &released); err != nil {
		t.Fatalf("parsing released: %v", err)
	}
	if released["status"] != "released" {
		t.Errorf("status = %v, want released", released["status"])
	}
}

func TestClaimRenewCmd(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := dir
	ledgerDir := filepath.Join(dir, "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Acquire a claim.
	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{
		"coordination", "claim-acquire",
		"--state", stateDir,
		"--ledger", ledgerDir,
		"--work-id", "work_001",
		"--pattern", "src/*.go",
		"--owner", "agent-1",
		"--ttl", "60",
	})
	if err := root.Execute(); err != nil {
		t.Fatalf("acquire error: %v", err)
	}

	var acquired map[string]any
	if err := json.Unmarshal(buf.Bytes(), &acquired); err != nil {
		t.Fatalf("parsing acquired: %v", err)
	}
	claimID := acquired["id"].(string)

	// Renew the claim.
	root2 := newRootCmd()
	buf2 := new(bytes.Buffer)
	root2.SetOut(buf2)
	root2.SetErr(buf2)
	root2.SetArgs([]string{
		"coordination", "claim-renew",
		"--state", stateDir,
		"--ledger", ledgerDir,
		"--claim-id", claimID,
		"--ttl", "900",
	})
	if err := root2.Execute(); err != nil {
		t.Fatalf("renew error: %v", err)
	}

	var renewed map[string]any
	if err := json.Unmarshal(buf2.Bytes(), &renewed); err != nil {
		t.Fatalf("parsing renewed: %v", err)
	}
	if renewed["claim_id"] != claimID {
		t.Errorf("claim_id = %v, want %s", renewed["claim_id"], claimID)
	}
}

func TestClaimRenewAllCmd(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := dir
	ledgerDir := filepath.Join(dir, "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Acquire a claim.
	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{
		"coordination", "claim-acquire",
		"--state", stateDir,
		"--ledger", ledgerDir,
		"--work-id", "work_001",
		"--pattern", "src/*.go",
		"--owner", "agent-1",
	})
	if err := root.Execute(); err != nil {
		t.Fatalf("acquire error: %v", err)
	}

	// Renew all.
	root2 := newRootCmd()
	buf2 := new(bytes.Buffer)
	root2.SetOut(buf2)
	root2.SetErr(buf2)
	root2.SetArgs([]string{
		"coordination", "claim-renew",
		"--state", stateDir,
		"--ledger", ledgerDir,
		"--all",
		"--ttl", "900",
	})
	if err := root2.Execute(); err != nil {
		t.Fatalf("renew-all error: %v", err)
	}

	var result map[string]any
	if err := json.Unmarshal(buf2.Bytes(), &result); err != nil {
		t.Fatalf("parsing result: %v", err)
	}
	if result["renewed"] != float64(1) {
		t.Errorf("renewed = %v, want 1", result["renewed"])
	}
}

func TestCRDTRebuildCmd_DryRun(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	// Create a sessions.jsonl with claim events.
	ledgerDir := dir
	events := []string{
		`{"event":"claim_created","timestamp":"2026-01-15T10:00:00Z","id":"claim_test","work_id":"w1","pattern":"*.go","mode":"exclusive","owner_id":"a1","fencing_token":1,"expires_at":"2026-01-15T10:10:00Z"}`,
	}
	source := filepath.Join(ledgerDir, "sessions.jsonl")
	if err := os.WriteFile(source, []byte(strings.Join(events, "\n")+"\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{
		"coordination", "crdt-rebuild",
		"--source", source,
		"--state", dir,
		"--dry-run",
	})

	if err := root.Execute(); err != nil {
		t.Fatalf("crdt-rebuild error: %v", err)
	}

	var result map[string]any
	if err := json.Unmarshal(buf.Bytes(), &result); err != nil {
		t.Fatalf("parsing JSON: %v; output: %s", err, buf.String())
	}

	if result["success"] != true {
		t.Error("success should be true")
	}
	if result["dry_run"] != true {
		t.Error("dry_run should be true")
	}
	if result["events_replayed"] != float64(1) {
		t.Errorf("events_replayed = %v, want 1", result["events_replayed"])
	}
}

func TestCRDTRebuildCmd_Verify(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	stateDir := dir

	// Create matching state and JSONL.
	coordDir := filepath.Join(stateDir, "coordination")
	if err := os.MkdirAll(coordDir, 0o755); err != nil {
		t.Fatal(err)
	}
	existingState := `{"claims":{"claim_v":{"id":"claim_v","status":"active","pattern":"*.go","mode":"exclusive","owner_id":"a1","fencing_token":1}},"token_counter":1}`
	if err := os.WriteFile(filepath.Join(coordDir, "state.json"), []byte(existingState), 0o644); err != nil {
		t.Fatal(err)
	}

	source := filepath.Join(dir, "sessions.jsonl")
	events := []string{
		`{"event":"claim_created","timestamp":"2026-01-15T10:00:00Z","id":"claim_v","work_id":"w1","pattern":"*.go","mode":"exclusive","owner_id":"a1","fencing_token":1,"expires_at":"2026-01-15T10:10:00Z"}`,
	}
	if err := os.WriteFile(source, []byte(strings.Join(events, "\n")+"\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{
		"coordination", "crdt-rebuild",
		"--source", source,
		"--state", stateDir,
		"--verify",
	})

	if err := root.Execute(); err != nil {
		t.Fatalf("crdt-rebuild --verify error: %v", err)
	}

	var result map[string]any
	if err := json.Unmarshal(buf.Bytes(), &result); err != nil {
		t.Fatalf("parsing JSON: %v; output: %s", err, buf.String())
	}

	if result["verified_ok"] != true {
		t.Errorf("verified_ok = %v, want true", result["verified_ok"])
	}
}

func TestClaimRenewCmd_NoArgs(t *testing.T) {
	t.Parallel()
	root := newRootCmd()
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs([]string{
		"coordination", "claim-renew",
		"--state", t.TempDir(),
		"--ledger", t.TempDir(),
	})

	err := root.Execute()
	if err == nil {
		t.Fatal("expected error when neither --claim-id nor --all is set")
	}
	if !strings.Contains(err.Error(), "--claim-id is required") {
		t.Errorf("error = %q, want to contain '--claim-id is required'", err.Error())
	}
}
