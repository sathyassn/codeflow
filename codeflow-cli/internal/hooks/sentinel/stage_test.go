package sentinel

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
)

func TestCheckAndCreateStageSentinel(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name           string
		stdin          string
		setupSentinels []string // sentinel names to pre-create
		wantAllow      bool
		wantSentinel   string // expected sentinel file created (empty if none)
		wantReason     string // substring expected in reason when blocked
	}{
		{
			name:      "empty stdin allows",
			stdin:     "",
			wantAllow: true,
		},
		{
			name:      "invalid JSON allows (graceful degradation)",
			stdin:     "not json at all",
			wantAllow: true,
		},
		{
			name:      "non-SendMessage tool ignored",
			stdin:     `{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`,
			wantAllow: true,
		},
		{
			name:      "SendMessage without stage pattern ignored",
			stdin:     `{"tool_name":"SendMessage","tool_input":{"content":"Hello world"}}`,
			wantAllow: true,
		},
		{
			name:      "SendMessage empty content ignored",
			stdin:     `{"tool_name":"SendMessage","tool_input":{"content":""}}`,
			wantAllow: true,
		},
		{
			name:      "SendMessage empty tool_input ignored",
			stdin:     `{"tool_name":"SendMessage","tool_input":{}}`,
			wantAllow: true,
		},
		{
			name:         "STAGE-COMPLETE WS-DEV creates sentinel",
			stdin:        `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}`,
			wantAllow:    true,
			wantSentinel: "pathflow-ws-dev",
		},
		{
			name:           "STAGE-COMPLETE WS-REV with ws-dev creates sentinel",
			stdin:          `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV"}}`,
			setupSentinels: []string{"ws-dev"},
			wantAllow:      true,
			wantSentinel:   "pathflow-ws-rev",
		},
		{
			name:       "STAGE-COMPLETE WS-REV without primary stage blocks",
			stdin:      `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV"}}`,
			wantAllow:  false,
			wantReason: "ws-rev requires prior primary stage",
		},
		{
			name:           "STAGE-COMPLETE WS-REV with ws-plan creates sentinel",
			stdin:          `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV"}}`,
			setupSentinels: []string{"ws-plan"},
			wantAllow:      true,
			wantSentinel:   "pathflow-ws-rev",
		},
		{
			name:           "STAGE-COMPLETE WS-REV with ws-docs creates sentinel",
			stdin:          `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV"}}`,
			setupSentinels: []string{"ws-docs"},
			wantAllow:      true,
			wantSentinel:   "pathflow-ws-rev",
		},
		{
			name:           "STAGE-COMPLETE WS-REV with ws-test creates sentinel",
			stdin:          `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV"}}`,
			setupSentinels: []string{"ws-test"},
			wantAllow:      true,
			wantSentinel:   "pathflow-ws-rev",
		},
		{
			name:           "STAGE-COMPLETE WS-QA with ws-dev creates sentinel",
			stdin:          `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-QA"}}`,
			setupSentinels: []string{"ws-dev"},
			wantAllow:      true,
			wantSentinel:   "pathflow-ws-qa",
		},
		{
			name:           "STAGE-COMPLETE WS-QA with ws-test creates sentinel",
			stdin:          `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-QA"}}`,
			setupSentinels: []string{"ws-test"},
			wantAllow:      true,
			wantSentinel:   "pathflow-ws-qa",
		},
		{
			name:       "STAGE-COMPLETE WS-QA without ws-dev or ws-test blocks",
			stdin:      `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-QA"}}`,
			wantAllow:  false,
			wantReason: "ws-qa requires prior ws-dev or ws-test",
		},
		{
			name:         "STAGE-COMPLETE WS-PLAN creates sentinel",
			stdin:        `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-PLAN"}}`,
			wantAllow:    true,
			wantSentinel: "pathflow-ws-plan",
		},
		{
			name:         "STAGE-COMPLETE WS-DOCS creates sentinel",
			stdin:        `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DOCS"}}`,
			wantAllow:    true,
			wantSentinel: "pathflow-ws-docs",
		},
		{
			name:         "STAGE-COMPLETE WS-TEST creates sentinel",
			stdin:        `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-TEST"}}`,
			wantAllow:    true,
			wantSentinel: "pathflow-ws-test",
		},
		{
			name:         "case insensitive matching",
			stdin:        `{"tool_name":"SendMessage","tool_input":{"content":"stage-complete: ws-dev"}}`,
			wantAllow:    true,
			wantSentinel: "pathflow-ws-dev",
		},
		{
			name:         "extra whitespace in content",
			stdin:        `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE:   WS-DEV   and some more text"}}`,
			wantAllow:    true,
			wantSentinel: "pathflow-ws-dev",
		},
		{
			name:      "partial match does not trigger",
			stdin:     `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-INVALID"}}`,
			wantAllow: true,
		},
		{
			name:      "invalid tool_input JSON allows",
			stdin:     `{"tool_name":"SendMessage","tool_input":"not json"}`,
			wantAllow: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			sentinelDir := t.TempDir()

			// Pre-create sentinels.
			for _, s := range tt.setupSentinels {
				path := filepath.Join(sentinelDir, "pathflow-"+s)
				if err := os.WriteFile(path, []byte("1"), 0o644); err != nil {
					t.Fatalf("setup sentinel %s: %v", s, err)
				}
			}

			reader := strings.NewReader(tt.stdin)
			verdict := CheckAndCreateStageSentinel(reader, sentinelDir)

			if verdict.Allow != tt.wantAllow {
				t.Errorf("Allow = %v, want %v; reason: %s", verdict.Allow, tt.wantAllow, verdict.Reason)
			}

			if tt.wantReason != "" && !strings.Contains(verdict.Reason, tt.wantReason) {
				t.Errorf("Reason = %q, want substring %q", verdict.Reason, tt.wantReason)
			}

			if tt.wantSentinel != "" {
				path := filepath.Join(sentinelDir, tt.wantSentinel)
				if _, err := os.Stat(path); os.IsNotExist(err) {
					t.Errorf("expected sentinel file %q not created", tt.wantSentinel)
				}
			}
		})
	}
}

func TestCheckAndCreateStageSentinel_Idempotent(t *testing.T) {
	t.Parallel()

	sentinelDir := t.TempDir()

	// Create sentinel twice -- second call should succeed without error.
	stdin1 := `{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}`
	v1 := CheckAndCreateStageSentinel(strings.NewReader(stdin1), sentinelDir)
	if !v1.Allow {
		t.Fatalf("first call blocked: %s", v1.Reason)
	}

	v2 := CheckAndCreateStageSentinel(strings.NewReader(stdin1), sentinelDir)
	if !v2.Allow {
		t.Fatalf("second call blocked: %s", v2.Reason)
	}

	// Verify file still exists.
	path := filepath.Join(sentinelDir, "pathflow-ws-dev")
	if _, err := os.Stat(path); os.IsNotExist(err) {
		t.Error("sentinel file missing after idempotent creation")
	}
}

func TestHasSentinelFile(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()

	// File does not exist.
	if hasSentinelFile(dir, "ws-dev") {
		t.Error("hasSentinelFile() = true for non-existent file, want false")
	}

	// Create and verify.
	if err := os.WriteFile(filepath.Join(dir, "pathflow-ws-dev"), []byte("1"), 0o644); err != nil {
		t.Fatal(err)
	}
	if !hasSentinelFile(dir, "ws-dev") {
		t.Error("hasSentinelFile() = false for existing file, want true")
	}
}

func TestCreateSentinelFile(t *testing.T) {
	t.Parallel()

	dir := filepath.Join(t.TempDir(), "nested", "dir")

	// Creates parent directories.
	if err := createSentinelFile(dir, "pf-1"); err != nil {
		t.Fatalf("createSentinelFile() error: %v", err)
	}

	path := filepath.Join(dir, "pathflow-pf-1")
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read sentinel file: %v", err)
	}
	if string(data) != "1" {
		t.Errorf("sentinel content = %q, want %q", string(data), "1")
	}
}

func TestCollapseWhitespace(t *testing.T) {
	t.Parallel()

	tests := []struct {
		input string
		want  string
	}{
		{"hello world", "hello world"},
		{"hello   world", "hello world"},
		{"  hello  world  ", "hello world"},
		{"a\tb\nc", "a b c"},
		{"", ""},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			t.Parallel()
			got := collapseWhitespace(tt.input)
			if got != tt.want {
				t.Errorf("collapseWhitespace(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestCheckAndCreateStageSentinelFromData(t *testing.T) {
	t.Parallel()

	t.Run("creates sentinel from pre-read data", func(t *testing.T) {
		t.Parallel()
		sentinelDir := t.TempDir()
		data := []byte(`{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}`)

		verdict := CheckAndCreateStageSentinelFromData(data, sentinelDir)

		if !verdict.Allow {
			t.Fatalf("Allow = false, want true; reason: %s", verdict.Reason)
		}
		path := filepath.Join(sentinelDir, "pathflow-ws-dev")
		if _, err := os.Stat(path); os.IsNotExist(err) {
			t.Error("expected pathflow-ws-dev sentinel not created")
		}
	})

	t.Run("empty data allows", func(t *testing.T) {
		t.Parallel()
		verdict := CheckAndCreateStageSentinelFromData(nil, t.TempDir())
		if !verdict.Allow {
			t.Errorf("Allow = false for nil data, want true")
		}
	})
}

func TestHandleTeamCreate(t *testing.T) {
	t.Parallel()

	t.Run("creates pathflow-team.json on TeamCreate", func(t *testing.T) {
		t.Parallel()
		sessionDir := filepath.Join(t.TempDir(), "pathflow")
		sessionID := "ses-1234567890123abcdef012345"

		data := []byte(`{"tool_name":"TeamCreate","tool_input":{"team_name":"my-team","description":"A test team"}}`)
		verdict := HandleTeamCreate(data, sessionDir, sessionID)

		if !verdict.Allow {
			t.Fatalf("Allow = false, want true; reason: %s", verdict.Reason)
		}

		teamFilePath := filepath.Join(sessionDir, "pathflow-team.json")
		content, err := os.ReadFile(teamFilePath)
		if err != nil {
			t.Fatalf("read pathflow-team.json: %v", err)
		}

		var team PathflowTeam
		if err := json.Unmarshal(content, &team); err != nil {
			t.Fatalf("unmarshal pathflow-team.json: %v", err)
		}

		if team.TeamName != "my-team" {
			t.Errorf("TeamName = %q, want %q", team.TeamName, "my-team")
		}
		if team.CodeflowSessionID != sessionID {
			t.Errorf("CodeflowSessionID = %q, want %q", team.CodeflowSessionID, sessionID)
		}
		if team.TeammateSpawned {
			t.Error("TeammateSpawned = true, want false")
		}
		if team.LastSpawnName != nil {
			t.Errorf("LastSpawnName = %v, want nil", team.LastSpawnName)
		}
		if team.LeadPID <= 0 {
			t.Errorf("LeadPID = %d, want > 0", team.LeadPID)
		}
		if team.CreatedAt == "" {
			t.Error("CreatedAt is empty, want ISO8601 timestamp")
		}
	})

	t.Run("creates session dir if missing", func(t *testing.T) {
		t.Parallel()
		sessionDir := filepath.Join(t.TempDir(), "nested", "pathflow")
		sessionID := "ses-test"

		data := []byte(`{"tool_name":"TeamCreate","tool_input":{"team_name":"test-team"}}`)
		verdict := HandleTeamCreate(data, sessionDir, sessionID)

		if !verdict.Allow {
			t.Fatalf("Allow = false, want true")
		}

		teamFilePath := filepath.Join(sessionDir, "pathflow-team.json")
		if _, err := os.Stat(teamFilePath); os.IsNotExist(err) {
			t.Error("pathflow-team.json not created in nested dir")
		}
	})

	t.Run("skips non-TeamCreate events", func(t *testing.T) {
		t.Parallel()
		sessionDir := t.TempDir()
		data := []byte(`{"tool_name":"SendMessage","tool_input":{"content":"hello"}}`)

		verdict := HandleTeamCreate(data, sessionDir, "ses-test")

		if !verdict.Allow {
			t.Fatal("Allow = false for non-TeamCreate")
		}
		teamFilePath := filepath.Join(sessionDir, "pathflow-team.json")
		if _, err := os.Stat(teamFilePath); !os.IsNotExist(err) {
			t.Error("pathflow-team.json should not be created for non-TeamCreate")
		}
	})

	t.Run("skips with empty session ID", func(t *testing.T) {
		t.Parallel()
		data := []byte(`{"tool_name":"TeamCreate","tool_input":{"team_name":"my-team"}}`)

		verdict := HandleTeamCreate(data, t.TempDir(), "")

		if !verdict.Allow {
			t.Fatal("Allow = false for empty session ID")
		}
	})

	t.Run("skips with empty session dir", func(t *testing.T) {
		t.Parallel()
		data := []byte(`{"tool_name":"TeamCreate","tool_input":{"team_name":"my-team"}}`)

		verdict := HandleTeamCreate(data, "", "ses-test")

		if !verdict.Allow {
			t.Fatal("Allow = false for empty session dir")
		}
	})

	t.Run("handles invalid JSON gracefully", func(t *testing.T) {
		t.Parallel()
		verdict := HandleTeamCreate([]byte("not json"), t.TempDir(), "ses-test")

		if !verdict.Allow {
			t.Fatal("Allow = false for invalid JSON")
		}
	})

	t.Run("handles invalid tool_input gracefully", func(t *testing.T) {
		t.Parallel()
		data := []byte(`{"tool_name":"TeamCreate","tool_input":"not-object"}`)

		verdict := HandleTeamCreate(data, t.TempDir(), "ses-test")

		if !verdict.Allow {
			t.Fatal("Allow = false for invalid tool_input")
		}
	})
}

func TestHandleTeammateSpawn(t *testing.T) {
	t.Parallel()

	t.Run("updates pathflow-team.json on Task spawn", func(t *testing.T) {
		t.Parallel()
		sessionDir := t.TempDir()

		// Pre-create pathflow-team.json.
		team := PathflowTeam{
			TeamName:          "test-team",
			LeadPID:           12345,
			CodeflowSessionID: "ses-test",
			TeammateSpawned:   false,
			CreatedAt:         "2024-01-01T00:00:00Z",
			LastSpawnName:     nil,
		}
		teamJSON, _ := json.Marshal(team)
		teamFilePath := filepath.Join(sessionDir, "pathflow-team.json")
		if err := os.WriteFile(teamFilePath, teamJSON, 0o644); err != nil {
			t.Fatalf("write initial team json: %v", err)
		}

		data := []byte(`{"tool_name":"Task","tool_input":{"name":"cf-security","description":"spawn sec"}}`)
		verdict := HandleTeammateSpawn(data, sessionDir)

		if !verdict.Allow {
			t.Fatalf("Allow = false, want true; reason: %s", verdict.Reason)
		}

		content, err := os.ReadFile(teamFilePath)
		if err != nil {
			t.Fatalf("read updated team json: %v", err)
		}

		var updated PathflowTeam
		if err := json.Unmarshal(content, &updated); err != nil {
			t.Fatalf("unmarshal updated team json: %v", err)
		}

		if !updated.TeammateSpawned {
			t.Error("TeammateSpawned = false, want true")
		}
		if updated.LastSpawnName == nil || *updated.LastSpawnName != "cf-security" {
			t.Errorf("LastSpawnName = %v, want 'cf-security'", updated.LastSpawnName)
		}
		// Original fields preserved.
		if updated.TeamName != "test-team" {
			t.Errorf("TeamName = %q, want 'test-team'", updated.TeamName)
		}
		if updated.LeadPID != 12345 {
			t.Errorf("LeadPID = %d, want 12345", updated.LeadPID)
		}
	})

	t.Run("skips when pathflow-team.json does not exist", func(t *testing.T) {
		t.Parallel()
		sessionDir := t.TempDir()
		data := []byte(`{"tool_name":"Task","tool_input":{"name":"cf-dev"}}`)

		verdict := HandleTeammateSpawn(data, sessionDir)

		if !verdict.Allow {
			t.Fatal("Allow = false when team json missing")
		}
	})

	t.Run("skips non-Task events", func(t *testing.T) {
		t.Parallel()
		data := []byte(`{"tool_name":"SendMessage","tool_input":{"content":"hello"}}`)

		verdict := HandleTeammateSpawn(data, t.TempDir())

		if !verdict.Allow {
			t.Fatal("Allow = false for non-Task event")
		}
	})

	t.Run("skips with empty session dir", func(t *testing.T) {
		t.Parallel()
		data := []byte(`{"tool_name":"Task","tool_input":{"name":"cf-dev"}}`)

		verdict := HandleTeammateSpawn(data, "")

		if !verdict.Allow {
			t.Fatal("Allow = false for empty session dir")
		}
	})

	t.Run("handles invalid JSON gracefully", func(t *testing.T) {
		t.Parallel()
		verdict := HandleTeammateSpawn([]byte("bad json"), t.TempDir())

		if !verdict.Allow {
			t.Fatal("Allow = false for invalid JSON")
		}
	})

	t.Run("updates last_spawn_name on subsequent spawns", func(t *testing.T) {
		t.Parallel()
		sessionDir := t.TempDir()

		// Pre-create pathflow-team.json with an existing spawn.
		name1 := "cf-security"
		team := PathflowTeam{
			TeamName:          "test-team",
			LeadPID:           12345,
			CodeflowSessionID: "ses-test",
			TeammateSpawned:   true,
			CreatedAt:         "2024-01-01T00:00:00Z",
			LastSpawnName:     &name1,
		}
		teamJSON, _ := json.Marshal(team)
		if err := os.WriteFile(filepath.Join(sessionDir, "pathflow-team.json"), teamJSON, 0o644); err != nil {
			t.Fatalf("write initial team json: %v", err)
		}

		// Spawn second teammate.
		data := []byte(`{"tool_name":"Task","tool_input":{"name":"cf-knowledge-layer"}}`)
		verdict := HandleTeammateSpawn(data, sessionDir)

		if !verdict.Allow {
			t.Fatalf("Allow = false; reason: %s", verdict.Reason)
		}

		content, err := os.ReadFile(filepath.Join(sessionDir, "pathflow-team.json"))
		if err != nil {
			t.Fatalf("read: %v", err)
		}

		var updated PathflowTeam
		if err := json.Unmarshal(content, &updated); err != nil {
			t.Fatalf("unmarshal: %v", err)
		}

		if updated.LastSpawnName == nil || *updated.LastSpawnName != "cf-knowledge-layer" {
			t.Errorf("LastSpawnName = %v, want 'cf-knowledge-layer'", updated.LastSpawnName)
		}
	})
}

func TestAtomicWriteFile(t *testing.T) {
	t.Parallel()

	t.Run("writes file atomically", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "test-file.json")

		if err := atomicWriteFile(path, []byte(`{"key":"value"}`), 0o644); err != nil {
			t.Fatalf("atomicWriteFile() error: %v", err)
		}

		content, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read: %v", err)
		}
		if string(content) != `{"key":"value"}` {
			t.Errorf("content = %q, want %q", string(content), `{"key":"value"}`)
		}

		// Verify no .tmp file remains.
		if _, err := os.Stat(path + ".tmp"); !os.IsNotExist(err) {
			t.Error("tmp file should not remain after successful write")
		}
	})

	t.Run("concurrent writes do not corrupt", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "concurrent.json")

		var wg sync.WaitGroup
		for i := range 10 {
			wg.Add(1)
			go func(n int) {
				defer wg.Done()
				data := []byte(`{"n":` + strings.Repeat("x", n) + `}`)
				_ = atomicWriteFile(path, data, 0o644)
			}(i)
		}
		wg.Wait()

		// File should exist and be readable (not corrupted).
		content, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read after concurrent writes: %v", err)
		}
		if len(content) == 0 {
			t.Error("file is empty after concurrent writes")
		}
	})
}
