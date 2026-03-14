package verification

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/codeflow/codeflow-cli/internal/doctor"
	"github.com/codeflow/codeflow-cli/internal/hooks/sentinel"
)

// TestPostCutoverLifecycle verifies the Go sentinel pipeline handles all
// event types that the old shell hook used to handle: SendMessage
// (STAGE-COMPLETE), TeamCreate (pathflow-team.json creation), and Task
// (teammate spawn tracking).
func TestPostCutoverLifecycle(t *testing.T) {
	t.Parallel()

	root := t.TempDir()
	sessionID := "ses-01jk0000000000000000000000"
	sentinelDir := filepath.Join(root, ".state", "sentinels", "pathflow", sessionID)
	sessionDir := filepath.Join(root, ".state", "session", sessionID, "pathflow")

	// Step 1: TeamCreate creates pathflow-team.json.
	teamCreateData := []byte(`{"tool_name":"TeamCreate","tool_input":{"team_name":"test-team","description":"integration"}}`)
	v := sentinel.HandleTeamCreate(teamCreateData, sessionDir, sessionID)
	if !v.Allow {
		t.Fatalf("TeamCreate blocked: %s", v.Reason)
	}

	teamFilePath := filepath.Join(sessionDir, "pathflow-team.json")
	if _, err := os.Stat(teamFilePath); os.IsNotExist(err) {
		t.Fatal("pathflow-team.json not created after TeamCreate")
	}

	// Step 2: Task (teammate spawn) updates pathflow-team.json.
	taskData := []byte(`{"tool_name":"Task","tool_input":{"name":"cf-security","description":"spawn"}}`)
	v = sentinel.HandleTeammateSpawn(taskData, sessionDir)
	if !v.Allow {
		t.Fatalf("Task blocked: %s", v.Reason)
	}

	content, err := os.ReadFile(teamFilePath)
	if err != nil {
		t.Fatalf("read team json: %v", err)
	}
	var team sentinel.PathflowTeam
	if err := json.Unmarshal(content, &team); err != nil {
		t.Fatalf("unmarshal team json: %v", err)
	}
	if !team.TeammateSpawned {
		t.Error("TeammateSpawned not set after Task")
	}
	if team.LastSpawnName == nil || *team.LastSpawnName != "cf-security" {
		t.Errorf("LastSpawnName = %v, want cf-security", team.LastSpawnName)
	}

	// Step 3: STAGE-COMPLETE: WS-DEV creates sentinel.
	devData := []byte(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-DEV"}}`)
	v = sentinel.CheckAndCreateStageSentinelFromData(devData, sentinelDir)
	if !v.Allow {
		t.Fatalf("WS-DEV blocked: %s", v.Reason)
	}
	assertSentinelExists(t, sentinelDir, "pathflow-ws-dev")

	// Step 4: STAGE-COMPLETE: WS-REV (requires ws-dev).
	revData := []byte(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-REV"}}`)
	v = sentinel.CheckAndCreateStageSentinelFromData(revData, sentinelDir)
	if !v.Allow {
		t.Fatalf("WS-REV blocked: %s", v.Reason)
	}
	assertSentinelExists(t, sentinelDir, "pathflow-ws-rev")

	// Step 5: STAGE-COMPLETE: WS-QA (requires ws-dev).
	qaData := []byte(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-QA"}}`)
	v = sentinel.CheckAndCreateStageSentinelFromData(qaData, sentinelDir)
	if !v.Allow {
		t.Fatalf("WS-QA blocked: %s", v.Reason)
	}
	assertSentinelExists(t, sentinelDir, "pathflow-ws-qa")
}

// TestDoctorClean runs doctor checks programmatically and verifies no
// checks fail. Warn results are acceptable (some checks depend on runtime
// state), but fail results indicate infrastructure problems.
func TestDoctorClean(t *testing.T) {
	t.Parallel()

	projectDir := findProjectDir(t)
	opts := &doctor.Options{
		ProjectDir: projectDir,
		DBPath:     filepath.Join(projectDir, ".state", "db", "codeflow.db"),
		LedgerDir:  filepath.Join(projectDir, ".state", "ledger"),
		StateDir:   filepath.Join(projectDir, ".state"),
	}

	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()

	results := doctor.RunAll(ctx, opts)
	if len(results) == 0 {
		t.Fatal("doctor returned zero results — check registry may be empty")
	}

	// Known runtime-dependent checks that may fail in CI or active dev environments.
	// These depend on external binaries or live database/JSONL state, not migration correctness.
	runtimeChecks := map[string]bool{
		"binary":    true,
		"git-hooks": true,
		"database":  true, // FK violations from active dev sessions
		"jsonl":     true, // Transient JSONL parse errors during writes
		"hooks":     true, // Requires codeflow binary in PATH
		"claude":    true, // Requires Claude CLI in PATH
		"auth":      true, // Requires Claude CLI auth
	}

	for _, r := range results {
		if r.Status == doctor.StatusFail {
			if runtimeChecks[r.Name] {
				t.Logf("WARN (runtime-dependent): %s — %s", r.Name, r.Message)
				continue
			}
			t.Errorf("doctor check %q FAILED: %s", r.Name, r.Message)
		}
	}
}

// TestNoPython3 verifies that no production code paths reference python3.
// Test infrastructure (.codeflow/testing/lib/test-runner.sh) is excluded
// as it has a documented python3 fallback for YAML parsing.
func TestNoPython3(t *testing.T) {
	t.Parallel()

	projectDir := findProjectDir(t)
	productionDirs := []string{
		filepath.Join(projectDir, ".codeflow", "scripts"),
		filepath.Join(projectDir, ".claude", "hooks"),
	}

	for _, dir := range productionDirs {
		if _, err := os.Stat(dir); os.IsNotExist(err) {
			continue
		}

		err := filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
			if err != nil {
				return nil
			}
			if info.IsDir() {
				return nil
			}
			// Only check executable scripts.
			ext := filepath.Ext(path)
			if ext != ".sh" && ext != ".py" && ext != "" {
				return nil
			}

			content, readErr := os.ReadFile(path)
			if readErr != nil {
				return nil
			}

			// Exclude test infrastructure.
			relPath, _ := filepath.Rel(projectDir, path)
			if strings.Contains(relPath, "testing/") {
				return nil
			}

			if strings.Contains(string(content), "python3") {
				t.Errorf("python3 reference found in production file: %s", relPath)
			}
			return nil
		})
		if err != nil {
			t.Logf("walk error in %s: %v", dir, err)
		}
	}
}

// TestSettingsClean verifies settings.json contains no .sh hook command
// references, confirming all hooks have been migrated to the Go CLI.
func TestSettingsClean(t *testing.T) {
	t.Parallel()

	projectDir := findProjectDir(t)
	settingsPath := filepath.Join(projectDir, ".claude", "settings.json")

	content, err := os.ReadFile(settingsPath)
	if err != nil {
		t.Fatalf("read settings.json: %v", err)
	}

	// Parse to find hook command entries.
	var settings map[string]any
	if err := json.Unmarshal(content, &settings); err != nil {
		t.Fatalf("parse settings.json: %v", err)
	}

	hooks, ok := settings["hooks"]
	if !ok {
		// No hooks section — clean by definition.
		return
	}

	hooksMap, ok := hooks.(map[string]any)
	if !ok {
		return
	}

	for eventType, eventHooks := range hooksMap {
		hookEntries, ok := eventHooks.([]any)
		if !ok {
			continue
		}
		for _, entry := range hookEntries {
			entryMap, ok := entry.(map[string]any)
			if !ok {
				continue
			}
			innerHooks, ok := entryMap["hooks"].([]any)
			if !ok {
				continue
			}
			for _, hook := range innerHooks {
				hookMap, ok := hook.(map[string]any)
				if !ok {
					continue
				}
				cmd, ok := hookMap["command"].(string)
				if !ok {
					continue
				}
				if strings.HasSuffix(cmd, ".sh") || strings.Contains(cmd, ".sh ") {
					t.Errorf("settings.json %s hook has .sh reference: %q", eventType, cmd)
				}
			}
		}
	}
}

// findProjectDir locates the project root by walking up from the test binary
// location looking for .claude/settings.json.
func findProjectDir(t *testing.T) string {
	t.Helper()

	// Try common project root detection.
	candidates := []string{
		// When running from codeflow-cli/ subdirectory.
		filepath.Join("..", ".."),
		// When running from project root.
		".",
	}

	cwd, err := os.Getwd()
	if err != nil {
		t.Fatalf("getwd: %v", err)
	}

	for _, c := range candidates {
		dir := filepath.Join(cwd, c)
		if _, err := os.Stat(filepath.Join(dir, ".claude", "settings.json")); err == nil {
			abs, _ := filepath.Abs(dir)
			return abs
		}
	}

	// Walk up from cwd.
	dir := cwd
	for {
		if _, err := os.Stat(filepath.Join(dir, ".claude", "settings.json")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}

	t.Fatalf("could not find project root from %s", cwd)
	return ""
}

// assertSentinelExists checks that a sentinel file exists in the given directory.
func assertSentinelExists(t *testing.T, dir, name string) {
	t.Helper()
	path := filepath.Join(dir, name)
	if _, err := os.Stat(path); os.IsNotExist(err) {
		t.Errorf("expected sentinel %q not found in %s", name, dir)
	}
}
