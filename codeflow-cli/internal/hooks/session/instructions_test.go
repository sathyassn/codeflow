package session

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// setupInstructionsConfig writes a minimal instructions-config.json and
// associated .txt files for testing.
func setupInstructionsConfig(t *testing.T, projectDir string, entries map[string]instructionEntry, txtFiles map[string]string) {
	t.Helper()
	instructionsDir := filepath.Join(projectDir, ".codeflow", "config", "instructions")
	if err := os.MkdirAll(instructionsDir, 0o755); err != nil {
		t.Fatal(err)
	}

	cfg := instructionsConfig{}
	cfg.Hooks.SessionStart = entries

	data, err := json.Marshal(cfg)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(instructionsDir, "instructions-config.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	for name, content := range txtFiles {
		if err := os.WriteFile(filepath.Join(instructionsDir, name), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

// setupActiveTask writes an active-task.json file for testing.
func setupActiveTask(t *testing.T, projectDir string, taskID, title, status string) {
	t.Helper()
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}

	task := map[string]string{
		"task_id":        taskID,
		"task_format_id": taskID,
		"title":          title,
		"status":         status,
	}
	data, err := json.Marshal(task)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(runtimeDir, "active-task.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}
}

// setupPathFlowActive creates the is-pathflow-active flag and optionally
// sentinel files for testing.
func setupPathFlowActive(t *testing.T, projectDir, sessionID string, sentinels []string) {
	t.Helper()

	// Create pathflow-active flag.
	flagDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	flagData := `{"session_id":"` + sessionID + `","tracking_level":"tracked"}`
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte(flagData), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create codeflow-env.sh (sole source of session ID for resolveCodeflowSessionID).
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='" + sessionID + "'\nexport CF_PROJECT_ROOT='testproject'\n"
	if err := os.WriteFile(filepath.Join(runtimeDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create sentinel files.
	if len(sentinels) > 0 {
		sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
		if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
			t.Fatal(err)
		}
		for _, s := range sentinels {
			if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-"+s), []byte("{}"), 0o644); err != nil {
				t.Fatal(err)
			}
		}
	}
}

func TestRunInstructions_HappyPath(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"session-start": {File: "session-start.txt", Enabled: true},
		"memory-load":   {File: "memory-load.txt", Enabled: true},
	}, map[string]string{
		"session-start.txt": "Execute CLAUDE.md Section 2.\n",
		"memory-load.txt":   "Load work context via cf-knowledge-layer.\n",
	})

	setupActiveTask(t, projectDir, "INF-TSK-001-001", "Fix the bug", "in_progress")

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()

	// Verify instruction content appears.
	if !strings.Contains(output, "Execute CLAUDE.md Section 2.") {
		t.Error("output missing session-start instruction content")
	}
	if !strings.Contains(output, "Load work context via cf-knowledge-layer.") {
		t.Error("output missing memory-load instruction content")
	}

	// Verify active task context.
	if !strings.Contains(output, "ACTIVE TASKS DETECTED") {
		t.Error("output missing ACTIVE TASKS DETECTED header")
	}
	if !strings.Contains(output, "INF-TSK-001-001") {
		t.Error("output missing task ID")
	}
	if !strings.Contains(output, "Fix the bug") {
		t.Error("output missing task title")
	}
}

func TestRunInstructions_MissingConfig_Fallback(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	// No config file created.

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()

	// Should contain the fallback instruction.
	if !strings.Contains(output, "SESSION START - EXECUTE CLAUDE.md SECTION 2") {
		t.Error("output missing fallback instruction")
	}
	if !strings.Contains(output, "Check for active work") {
		t.Error("output missing fallback detail text")
	}
}

func TestRunInstructions_InvalidConfig_Fallback(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	instructionsDir := filepath.Join(projectDir, ".codeflow", "config", "instructions")
	if err := os.MkdirAll(instructionsDir, 0o755); err != nil {
		t.Fatal(err)
	}
	// Write invalid JSON.
	if err := os.WriteFile(filepath.Join(instructionsDir, "instructions-config.json"), []byte("{invalid json}"), 0o644); err != nil {
		t.Fatal(err)
	}

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "SESSION START - EXECUTE CLAUDE.md SECTION 2") {
		t.Error("output missing fallback instruction for invalid config")
	}
}

func TestRunInstructions_DisabledEntries_Fallback(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"session-start": {File: "session-start.txt", Enabled: false},
	}, map[string]string{
		"session-start.txt": "Should not appear.\n",
	})

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	if strings.Contains(output, "Should not appear") {
		t.Error("disabled instruction content should not appear in output")
	}
	if !strings.Contains(output, "SESSION START - EXECUTE CLAUDE.md SECTION 2") {
		t.Error("output missing fallback instruction when all entries disabled")
	}
}

func TestRunInstructions_MissingTxtFile(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"session-start": {File: "nonexistent.txt", Enabled: true},
	}, nil)

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	// With no readable .txt files, should fall back.
	if !strings.Contains(output, "SESSION START - EXECUTE CLAUDE.md SECTION 2") {
		t.Error("output missing fallback instruction when .txt file is missing")
	}
}

func TestRunInstructions_ActiveTaskPresent(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"test": {File: "test.txt", Enabled: true},
	}, map[string]string{
		"test.txt": "Test instruction.\n",
	})
	setupActiveTask(t, projectDir, "INF-TSK-021-034", "Build Go CLI hook", "in_progress")

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "ACTIVE TASKS DETECTED") {
		t.Error("output missing ACTIVE TASKS DETECTED header")
	}
	if !strings.Contains(output, "INF-TSK-021-034") {
		t.Error("output missing task ID")
	}
	if !strings.Contains(output, "Build Go CLI hook") {
		t.Error("output missing task title")
	}
	if !strings.Contains(output, "in_progress") {
		t.Error("output missing task status")
	}
	if !strings.Contains(output, "Resume task") {
		t.Error("output missing resume option")
	}
}

func TestRunInstructions_NoActiveTask(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"test": {File: "test.txt", Enabled: true},
	}, map[string]string{
		"test.txt": "Test instruction.\n",
	})
	// No active-task.json created.

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "ACTIVE TASKS DETECTED: None") {
		t.Error("output missing 'ACTIVE TASKS DETECTED: None'")
	}
	if !strings.Contains(output, "IMPORTANT: Register work before making modifications.") {
		t.Error("output missing register-work reminder")
	}
	if !strings.Contains(output, "ensure-work-registered") {
		t.Error("output missing ensure-work-registered delegation text")
	}
}

func TestRunInstructions_PathFlowActive(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sessionID := "ses-01jk0000000000000000000000"

	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"test": {File: "test.txt", Enabled: true},
	}, map[string]string{
		"test.txt": "Test instruction.\n",
	})
	setupPathFlowActive(t, projectDir, sessionID, []string{"pf-1", "pf-2", "pf-3", "ws-dev"})

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"compact"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "PATHFLOW SESSION ACTIVE") {
		t.Error("output missing PATHFLOW SESSION ACTIVE header")
	}
	if !strings.Contains(output, "Completed phases:") {
		t.Error("output missing 'Completed phases:' header")
	}
	if !strings.Contains(output, "pathflow-pf-1") {
		t.Error("output missing pf-1 sentinel")
	}
	if !strings.Contains(output, "pathflow-pf-2") {
		t.Error("output missing pf-2 sentinel")
	}
	if !strings.Contains(output, "pathflow-pf-3") {
		t.Error("output missing pf-3 sentinel")
	}
	// ws-dev should NOT appear in the phase list (filtered to pf-* only).
	if strings.Contains(output, "pathflow-ws-dev") {
		t.Error("output should not list ws-dev as a completed phase")
	}
	if !strings.Contains(output, "Mode: pathflow") {
		t.Error("output missing 'Mode: pathflow'")
	}
	if !strings.Contains(output, "COMPACT RECOVERY: Task tracker registration check required.") {
		t.Error("output missing compact recovery checklist")
	}
	if !strings.Contains(output, "Step 1: Read checkpoint state") {
		t.Error("output missing recovery step 1")
	}
	if !strings.Contains(output, "FORBIDDEN: Skipping task tracker registration") {
		t.Error("output missing FORBIDDEN line")
	}
}

func TestRunInstructions_PathFlowNotActive(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"test": {File: "test.txt", Enabled: true},
	}, map[string]string{
		"test.txt": "Test instruction.\n",
	})
	// No PathFlow flag.

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	if strings.Contains(output, "PATHFLOW SESSION ACTIVE") {
		t.Error("output should not contain PATHFLOW SESSION ACTIVE when not active")
	}
}

func TestRunInstructions_PathFlowNoPhases(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sessionID := "ses-01jk0000000000000000000000"

	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"test": {File: "test.txt", Enabled: true},
	}, map[string]string{
		"test.txt": "Test instruction.\n",
	})
	// PathFlow active but no sentinels at all.
	setupPathFlowActive(t, projectDir, sessionID, nil)

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "PATHFLOW SESSION ACTIVE") {
		t.Error("output missing PATHFLOW SESSION ACTIVE header")
	}
	if !strings.Contains(output, "No completed phases found") {
		t.Error("output missing 'No completed phases found'")
	}
	// No recovery checklist when no phase sentinels.
	if strings.Contains(output, "COMPACT RECOVERY") {
		t.Error("output should not contain COMPACT RECOVERY when no sentinels exist")
	}
}

func TestRunInstructions_InvalidStdinJSON(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"test": {File: "test.txt", Enabled: true},
	}, map[string]string{
		"test.txt": "Test instruction.\n",
	})

	var buf strings.Builder
	stdin := strings.NewReader("this is not JSON")
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error on invalid stdin: %v", err)
	}

	output := buf.String()
	// Should still produce output (instruction loading doesn't depend on stdin).
	if !strings.Contains(output, "Test instruction.") {
		t.Error("output missing instruction content on invalid stdin")
	}
}

func TestRunInstructions_NilStdin(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"test": {File: "test.txt", Enabled: true},
	}, map[string]string{
		"test.txt": "Test instruction.\n",
	})

	var buf strings.Builder
	err := RunInstructions(nil, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error on nil stdin: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "Test instruction.") {
		t.Error("output missing instruction content on nil stdin")
	}
}

func TestRunInstructions_AlwaysReturnsNil(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	// Empty directory — no config, no task, no pathflow.

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() must always return nil, got: %v", err)
	}
}

func TestRunInstructions_EmptyActiveTaskJSON(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	// Write empty JSON object — no task_id.
	if err := os.WriteFile(filepath.Join(runtimeDir, "active-task.json"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"test": {File: "test.txt", Enabled: true},
	}, map[string]string{
		"test.txt": "Test.\n",
	})

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	// Empty task object is valid but has no task_id — should still show as active.
	if !strings.Contains(output, "ACTIVE TASKS DETECTED") {
		t.Error("output missing ACTIVE TASKS DETECTED for empty task object")
	}
}

func TestRunInstructions_PathFlowOnlyStages(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sessionID := "ses-01jk0000000000000000000000"

	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"test": {File: "test.txt", Enabled: true},
	}, map[string]string{
		"test.txt": "Test.\n",
	})
	// Only stage sentinels (ws-dev, ws-rev), no phase sentinels.
	setupPathFlowActive(t, projectDir, sessionID, []string{"ws-dev", "ws-rev"})

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "PATHFLOW SESSION ACTIVE") {
		t.Error("output missing PATHFLOW SESSION ACTIVE header")
	}
	// No pf-* sentinels should mean "No completed phases found"
	if !strings.Contains(output, "No completed phases found") {
		t.Error("output missing 'No completed phases found' when only stage sentinels exist")
	}
	// No COMPACT RECOVERY since no pf-* sentinels.
	if strings.Contains(output, "COMPACT RECOVERY") {
		t.Error("output should not contain COMPACT RECOVERY when only stage sentinels exist")
	}
}

func TestRunInstructions_CorruptActiveTask(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	// Write corrupt JSON.
	if err := os.WriteFile(filepath.Join(runtimeDir, "active-task.json"), []byte("{corrupt"), 0o644); err != nil {
		t.Fatal(err)
	}

	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{
		"test": {File: "test.txt", Enabled: true},
	}, map[string]string{
		"test.txt": "Test.\n",
	})

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error on corrupt active-task: %v", err)
	}

	output := buf.String()
	// Should gracefully show "None" since JSON parse fails.
	if !strings.Contains(output, "ACTIVE TASKS DETECTED: None") {
		t.Error("output missing 'ACTIVE TASKS DETECTED: None' for corrupt active-task.json")
	}
}

func TestRunInstructions_EmptySessionStartSection(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	setupInstructionsConfig(t, projectDir, map[string]instructionEntry{}, nil)

	var buf strings.Builder
	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	err := RunInstructions(stdin, &buf, projectDir)

	if err != nil {
		t.Fatalf("RunInstructions() returned error: %v", err)
	}

	output := buf.String()
	if !strings.Contains(output, "SESSION START - EXECUTE CLAUDE.md SECTION 2") {
		t.Error("output missing fallback instruction when SessionStart map is empty")
	}
}

func TestResolveCodeflowSessionID_EnvVar(t *testing.T) {
	// Cannot use t.Parallel() with t.Setenv.

	projectDir := t.TempDir()

	// Write codeflow-env.sh that should be overridden by env var.
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='ses-from-envfile'\nexport CF_PROJECT_ROOT='testproject'\n"
	if err := os.WriteFile(filepath.Join(runtimeDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	// Set env var -- should take priority over codeflow-env.sh.
	t.Setenv("CODEFLOW_SESSION_ID", "ses-from-env")

	sid := resolveCodeflowSessionID(projectDir)
	if sid != "ses-from-env" {
		t.Errorf("resolveCodeflowSessionID() = %q, want %q", sid, "ses-from-env")
	}
}

func TestResolveCodeflowSessionID_EnvFile(t *testing.T) {
	// Cannot use t.Parallel() with t.Setenv.

	projectDir := t.TempDir()
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='ses-from-envfile'\nexport CF_PROJECT_ROOT='testproject'\n"
	if err := os.WriteFile(filepath.Join(runtimeDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	// Ensure env var is not set.
	t.Setenv("CODEFLOW_SESSION_ID", "")

	sid := resolveCodeflowSessionID(projectDir)
	if sid != "ses-from-envfile" {
		t.Errorf("resolveCodeflowSessionID() = %q, want %q", sid, "ses-from-envfile")
	}
}

func TestResolveCodeflowSessionID_None(t *testing.T) {
	// Cannot use t.Parallel() with t.Setenv.

	projectDir := t.TempDir()
	t.Setenv("CODEFLOW_SESSION_ID", "")

	sid := resolveCodeflowSessionID(projectDir)
	if sid != "" {
		t.Errorf("resolveCodeflowSessionID() = %q, want empty", sid)
	}
}
