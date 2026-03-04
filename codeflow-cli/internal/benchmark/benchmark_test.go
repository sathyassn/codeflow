// Package benchmark provides performance benchmarks for Go CLI hook invocations.
//
// These benchmarks measure the core logic latency of each hook subcommand,
// establishing a baseline for comparison against the historical shell hook
// overhead (~300ms per tool call).
//
// Run benchmarks:
//
//	go test -bench=. -benchmem ./internal/benchmark/...
package benchmark

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/hooks/edit"
	"github.com/codeflow/codeflow-cli/internal/hooks/gate"
	"github.com/codeflow/codeflow-cli/internal/hooks/security"
	"github.com/codeflow/codeflow-cli/internal/hooks/sentinel"
)

// --- PreToolUse hook benchmarks ---

// BenchmarkSecurityCheck measures the security PreToolUse validation latency
// for a typical Bash tool call (the most common tool type).
func BenchmarkSecurityCheck(b *testing.B) {
	checker := security.NewChecker()
	ctx := &security.CheckContext{
		ToolName:      "Bash",
		Command:       "ls -la /tmp",
		SandboxBypass: false,
	}

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		checker.Check(ctx)
	}
}

// BenchmarkSecurityCheck_DangerousCommand measures security check latency for
// a command that triggers multiple pattern checks (worst case).
func BenchmarkSecurityCheck_DangerousCommand(b *testing.B) {
	checker := security.NewChecker()
	ctx := &security.CheckContext{
		ToolName:      "Bash",
		Command:       "rm -rf /tmp/test && git push --force origin main",
		SandboxBypass: true,
	}

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		checker.Check(ctx)
	}
}

// BenchmarkGateCheck measures the gate-check PreToolUse validation latency
// for an Edit tool call (the gated tool type).
func BenchmarkGateCheck(b *testing.B) {
	sentinelDir := b.TempDir()
	checker := &gate.GateChecker{
		SentinelDir: sentinelDir,
	}
	toolInput := json.RawMessage(`{"file_path":"/tmp/test.go","old_string":"a","new_string":"b"}`)

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		checker.Check("Edit", toolInput)
	}
}

// BenchmarkGateCheck_WithSentinel measures gate-check when the pf-3 sentinel
// exists (normal operating state during PF4-EXECUTE).
func BenchmarkGateCheck_WithSentinel(b *testing.B) {
	sentinelDir := b.TempDir()
	// Create the pf-3 sentinel so the gate passes.
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-3"), []byte("1"), 0o644); err != nil {
		b.Fatal(err)
	}
	checker := &gate.GateChecker{
		SentinelDir: sentinelDir,
	}
	toolInput := json.RawMessage(`{"file_path":"/tmp/test.go","old_string":"a","new_string":"b"}`)

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		checker.Check("Edit", toolInput)
	}
}

// BenchmarkEditWriteGuard measures the edit-write-guard scope validation latency.
func BenchmarkEditWriteGuard(b *testing.B) {
	checker := &edit.ScopeChecker{
		ProjectDir: "/tmp/testproject",
		BlockedDirs: edit.DefaultBlockedDirs(),
		AllowedTmpPrefixes: edit.DefaultAllowedTmpPrefixes(),
		DangerousExts: edit.DefaultDangerousExtensions(),
	}
	stdin := strings.NewReader(`{"tool_name":"Edit","tool_input":{"file_path":"/tmp/testproject/src/main.go"}}`)

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		stdin.Reset(`{"tool_name":"Edit","tool_input":{"file_path":"/tmp/testproject/src/main.go"}}`)
		checker.Check(stdin)
	}
}

// --- PostToolUse hook benchmarks ---

// BenchmarkSentinelWrite measures the stage sentinel creation latency from a
// STAGE-COMPLETE SendMessage event.
func BenchmarkSentinelWrite(b *testing.B) {
	sentinelDir := b.TempDir()
	data := []byte(`{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}`)

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		sentinel.CheckAndCreateStageSentinelFromData(data, sentinelDir)
	}
}

// BenchmarkSentinelWrite_FromReader measures sentinel creation from an io.Reader
// (the actual stdin path).
func BenchmarkSentinelWrite_FromReader(b *testing.B) {
	sentinelDir := b.TempDir()
	input := `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}`

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		sentinel.CheckAndCreateStageSentinel(strings.NewReader(input), sentinelDir)
	}
}

// BenchmarkCheckpointRegister measures the checkpoint task registration latency
// (PostToolUse on TaskCreate).
func BenchmarkCheckpointRegister(b *testing.B) {
	sessionDir := b.TempDir()
	sentinelDir := b.TempDir()
	data := `{"tool_name":"TaskCreate","tool_input":{"subject":"PF1-TSK-01 Init"}}`

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		sentinel.RegisterCheckpointTask(strings.NewReader(data), sessionDir, sentinelDir)
	}
}

// BenchmarkCheckpointComplete measures the checkpoint task completion latency
// (TaskCompleted event).
func BenchmarkCheckpointComplete(b *testing.B) {
	sessionDir := b.TempDir()
	sentinelDir := b.TempDir()
	data := `{"task_id":"1","subject":"PF1-TSK-01 Init","status":"completed"}`

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		sentinel.CompleteCheckpointTask(strings.NewReader(data), sessionDir, sentinelDir)
	}
}

// BenchmarkTeamCreate measures the pathflow-team.json creation latency.
func BenchmarkTeamCreate(b *testing.B) {
	sessionDir := b.TempDir()
	data := []byte(`{"tool_name":"TeamCreate","tool_input":{"team_name":"bench-team"}}`)

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		sentinel.HandleTeamCreate(data, sessionDir, "ses-bench")
	}
}

// BenchmarkTeammateSpawn measures the pathflow-team.json update latency on
// teammate spawn.
func BenchmarkTeammateSpawn(b *testing.B) {
	sessionDir := b.TempDir()

	// Pre-create pathflow-team.json.
	createData := []byte(`{"tool_name":"TeamCreate","tool_input":{"team_name":"bench-team"}}`)
	sentinel.HandleTeamCreate(createData, sessionDir, "ses-bench")

	spawnData := []byte(`{"tool_name":"Task","tool_input":{"name":"cf-dev"}}`)

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		sentinel.HandleTeammateSpawn(spawnData, sessionDir)
	}
}

// --- Combined overhead benchmarks ---

// BenchmarkTotalPerToolCall_PreToolUse measures the combined overhead of all
// PreToolUse hooks that fire on a typical Bash tool call:
// security + gate-check + edit-write-guard.
//
// This represents the minimum overhead added to every tool call.
func BenchmarkTotalPerToolCall_PreToolUse(b *testing.B) {
	// Setup security checker.
	secChecker := security.NewChecker()
	secCtx := &security.CheckContext{
		ToolName:      "Bash",
		Command:       "ls -la /tmp",
		SandboxBypass: false,
	}

	// Setup gate checker.
	sentinelDir := b.TempDir()
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-3"), []byte("1"), 0o644); err != nil {
		b.Fatal(err)
	}
	gateChecker := &gate.GateChecker{
		SentinelDir: sentinelDir,
	}
	toolInput := json.RawMessage(`{"command":"ls -la /tmp"}`)

	// Setup scope checker.
	scopeChecker := &edit.ScopeChecker{
		ProjectDir: "/tmp/testproject",
		BlockedDirs: edit.DefaultBlockedDirs(),
		AllowedTmpPrefixes: edit.DefaultAllowedTmpPrefixes(),
		DangerousExts: edit.DefaultDangerousExtensions(),
	}

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		// 1. Security check.
		secChecker.Check(secCtx)
		// 2. Gate check.
		gateChecker.Check("Bash", toolInput)
		// 3. Scope check.
		stdin := strings.NewReader(`{"tool_name":"Bash","tool_input":{"command":"ls -la /tmp"}}`)
		scopeChecker.Check(stdin)
	}
}

// BenchmarkTotalPerToolCall_PostToolUse measures the combined overhead of all
// PostToolUse hooks that fire on a typical tool call:
// sentinel-write + checkpoint-register.
func BenchmarkTotalPerToolCall_PostToolUse(b *testing.B) {
	sentinelDir := b.TempDir()
	sessionDir := b.TempDir()
	sentinelData := []byte(`{"tool_name":"Bash","tool_input":{"command":"ls"}}`)
	checkpointData := `{"tool_name":"Bash","tool_input":{"command":"ls"}}`

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		// 1. Sentinel write (no-op for non-matching tools).
		sentinel.CheckAndCreateStageSentinelFromData(sentinelData, sentinelDir)
		// 2. Checkpoint register (no-op for non-TaskCreate tools).
		sentinel.RegisterCheckpointTask(strings.NewReader(checkpointData), sessionDir, sentinelDir)
	}
}

// BenchmarkTotalPerToolCall_Combined measures the TOTAL overhead of all hooks
// that fire on a typical tool call (pre + post).
func BenchmarkTotalPerToolCall_Combined(b *testing.B) {
	// Setup pre-tool-use checkers.
	secChecker := security.NewChecker()
	secCtx := &security.CheckContext{
		ToolName:      "Bash",
		Command:       "ls -la /tmp",
		SandboxBypass: false,
	}
	sentinelDir := b.TempDir()
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-3"), []byte("1"), 0o644); err != nil {
		b.Fatal(err)
	}
	gateChecker := &gate.GateChecker{SentinelDir: sentinelDir}
	toolInput := json.RawMessage(`{"command":"ls -la /tmp"}`)
	scopeChecker := &edit.ScopeChecker{
		ProjectDir: "/tmp/testproject",
		BlockedDirs: edit.DefaultBlockedDirs(),
		AllowedTmpPrefixes: edit.DefaultAllowedTmpPrefixes(),
		DangerousExts: edit.DefaultDangerousExtensions(),
	}

	// Setup post-tool-use data.
	sessionDir := b.TempDir()
	sentinelData := []byte(`{"tool_name":"Bash","tool_input":{"command":"ls"}}`)
	checkpointData := `{"tool_name":"Bash","tool_input":{"command":"ls"}}`

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		// PreToolUse hooks.
		secChecker.Check(secCtx)
		gateChecker.Check("Bash", toolInput)
		stdin := strings.NewReader(`{"tool_name":"Bash","tool_input":{"command":"ls -la /tmp"}}`)
		scopeChecker.Check(stdin)

		// PostToolUse hooks.
		sentinel.CheckAndCreateStageSentinelFromData(sentinelData, sentinelDir)
		sentinel.RegisterCheckpointTask(strings.NewReader(checkpointData), sessionDir, sentinelDir)
	}
}

// --- Binary startup benchmark ---

// BenchmarkBinaryStartup measures the cold start time of the codeflow binary.
// This uses exec.Command to measure real subprocess overhead.
//
// Note: This benchmark requires the codeflow binary to be on PATH. If not
// available, it will skip gracefully. The measured time includes process
// fork + Go runtime init + command dispatch + exit.
func BenchmarkBinaryStartup(b *testing.B) {
	// Check if codeflow binary is available.
	binPath, err := exec.LookPath("codeflow")
	if err != nil {
		b.Skip("codeflow binary not on PATH; skipping startup benchmark")
	}

	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		cmd := exec.Command(binPath, "version")
		_ = cmd.Run()
	}
}
