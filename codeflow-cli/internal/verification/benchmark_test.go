package verification

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/hooks/gate"
	"github.com/codeflow/codeflow-cli/internal/hooks/security"
	"github.com/codeflow/codeflow-cli/internal/hooks/sentinel"
)

// BenchmarkSentinelWrite measures the latency of stage sentinel creation
// from a STAGE-COMPLETE SendMessage event.
func BenchmarkSentinelWrite(b *testing.B) {
	sentinelDir := b.TempDir()
	data := []byte(`{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}`)

	b.ResetTimer()
	for b.Loop() {
		sentinel.CheckAndCreateStageSentinelFromData(data, sentinelDir)
	}
}

// BenchmarkSentinelWriteFromReader measures sentinel creation from an io.Reader.
func BenchmarkSentinelWriteFromReader(b *testing.B) {
	sentinelDir := b.TempDir()
	stdin := `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}`

	b.ResetTimer()
	for b.Loop() {
		sentinel.CheckAndCreateStageSentinel(strings.NewReader(stdin), sentinelDir)
	}
}

// BenchmarkTeamCreate measures the latency of pathflow-team.json creation.
func BenchmarkTeamCreate(b *testing.B) {
	sessionDir := b.TempDir()
	data := []byte(`{"tool_name":"TeamCreate","tool_input":{"team_name":"bench-team"}}`)

	b.ResetTimer()
	for b.Loop() {
		sentinel.HandleTeamCreate(data, sessionDir, "ses-bench")
	}
}

// BenchmarkTeammateSpawn measures the latency of pathflow-team.json update
// on teammate spawn.
func BenchmarkTeammateSpawn(b *testing.B) {
	sessionDir := b.TempDir()

	// Pre-create pathflow-team.json.
	createData := []byte(`{"tool_name":"TeamCreate","tool_input":{"team_name":"bench-team"}}`)
	sentinel.HandleTeamCreate(createData, sessionDir, "ses-bench")

	spawnData := []byte(`{"tool_name":"Task","tool_input":{"name":"cf-dev"}}`)

	b.ResetTimer()
	for b.Loop() {
		sentinel.HandleTeammateSpawn(spawnData, sessionDir)
	}
}

// BenchmarkGateCheck measures the latency of gate-check PreToolUse validation.
func BenchmarkGateCheck(b *testing.B) {
	sentinelDir := b.TempDir()
	checker := &gate.GateChecker{
		SentinelDir: sentinelDir,
	}
	toolInput := json.RawMessage(`{"file_path":"/tmp/test.go","old_string":"a","new_string":"b"}`)

	b.ResetTimer()
	for b.Loop() {
		checker.Check("Edit", toolInput)
	}
}

// BenchmarkSecurityCheck measures the latency of security PreToolUse validation.
func BenchmarkSecurityCheck(b *testing.B) {
	checker := security.NewChecker()
	ctx := &security.CheckContext{
		ToolName:      "Bash",
		Command:       "ls -la",
		SandboxBypass: false,
	}

	b.ResetTimer()
	for b.Loop() {
		checker.Check(ctx)
	}
}

// BenchmarkCheckpointRegister measures the latency of checkpoint task registration.
func BenchmarkCheckpointRegister(b *testing.B) {
	sessionDir := b.TempDir()
	sentinelDir := b.TempDir()
	data := `{"tool_name":"TaskCreate","tool_input":{"subject":"PF1-TSK-01 Init"}}`

	b.ResetTimer()
	for b.Loop() {
		sentinel.RegisterCheckpointTask(strings.NewReader(data), sessionDir, sentinelDir)
	}
}
