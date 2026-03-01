package team

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// createPathflowActive creates the is-pathflow-active flag file in the given dir.
func createPathflowActive(t *testing.T, sessionDir string) {
	t.Helper()
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatalf("create session dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(sessionDir, "is-pathflow-active"), []byte("1"), 0o644); err != nil {
		t.Fatalf("create pathflow-active flag: %v", err)
	}
}

// createSentinel creates a sentinel file in the given directory.
func createSentinel(t *testing.T, sentinelDir, name string) {
	t.Helper()
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatalf("create sentinel dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-"+name), []byte("1"), 0o644); err != nil {
		t.Fatalf("create sentinel %s: %v", name, err)
	}
}

func TestCheckTeamDelete(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name             string
		stdin            string
		pathflowActive   bool
		pf6Exists        bool
		wantAllow        bool
		wantReasonSubstr string
	}{
		// --- Non-target tools ---
		{
			name:      "Edit tool allowed (not TeamDelete/Teammate)",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}`,
			wantAllow: true,
		},
		{
			name:      "Bash tool allowed (not TeamDelete/Teammate)",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"ls"}}`,
			wantAllow: true,
		},

		// --- Teammate non-cleanup ---
		{
			name:      "Teammate spawn operation allowed",
			stdin:     `{"tool_name":"Teammate","tool_input":{"operation":"spawn","name":"cf-dev"}}`,
			wantAllow: true,
		},
		{
			name:      "Teammate message operation allowed",
			stdin:     `{"tool_name":"Teammate","tool_input":{"operation":"message","name":"cf-dev"}}`,
			wantAllow: true,
		},
		{
			name:      "Teammate empty input allowed",
			stdin:     `{"tool_name":"Teammate","tool_input":{}}`,
			wantAllow: true,
		},
		{
			name:      "Teammate null input allowed",
			stdin:     `{"tool_name":"Teammate"}`,
			wantAllow: true,
		},

		// --- No PathFlow active ---
		{
			name:           "TeamDelete allowed when no PathFlow active",
			stdin:          `{"tool_name":"TeamDelete","tool_input":{}}`,
			pathflowActive: false,
			wantAllow:      true,
		},
		{
			name:           "Teammate cleanup allowed when no PathFlow active",
			stdin:          `{"tool_name":"Teammate","tool_input":{"operation":"cleanup"}}`,
			pathflowActive: false,
			wantAllow:      true,
		},

		// --- PathFlow active, no pf-6 sentinel ---
		{
			name:             "TeamDelete blocked when PathFlow active without pf-6",
			stdin:            `{"tool_name":"TeamDelete","tool_input":{}}`,
			pathflowActive:   true,
			pf6Exists:        false,
			wantAllow:        false,
			wantReasonSubstr: "TeamDelete",
		},
		{
			name:             "Teammate cleanup blocked when PathFlow active",
			stdin:            `{"tool_name":"Teammate","tool_input":{"operation":"cleanup"}}`,
			pathflowActive:   true,
			pf6Exists:        false,
			wantAllow:        false,
			wantReasonSubstr: "cleanup",
		},

		// --- PathFlow active, pf-6 sentinel exists (PF7-END gate) ---
		{
			name:           "TeamDelete allowed when pf-6 sentinel exists",
			stdin:          `{"tool_name":"TeamDelete","tool_input":{}}`,
			pathflowActive: true,
			pf6Exists:      true,
			wantAllow:      true,
		},
		{
			name:             "Teammate cleanup blocked even with pf-6 (only TeamDelete gets gate exception)",
			stdin:            `{"tool_name":"Teammate","tool_input":{"operation":"cleanup"}}`,
			pathflowActive:   true,
			pf6Exists:        true,
			wantAllow:        false,
			wantReasonSubstr: "cleanup",
		},

		// --- Edge cases ---
		{
			name:      "empty stdin allowed",
			stdin:     "",
			wantAllow: true,
		},
		{
			name:      "invalid JSON allowed (graceful degradation)",
			stdin:     "not json",
			wantAllow: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			tmpDir := t.TempDir()
			sessionDir := filepath.Join(tmpDir, "session", "pathflow")
			sentinelDir := filepath.Join(tmpDir, "sentinels")

			if tt.pathflowActive {
				createPathflowActive(t, sessionDir)
			}
			if tt.pf6Exists {
				createSentinel(t, sentinelDir, "pf-6")
			}

			verdict, _ := CheckTeamDelete(
				strings.NewReader(tt.stdin),
				sessionDir,
				sentinelDir,
			)

			if verdict.Allow != tt.wantAllow {
				t.Errorf("CheckTeamDelete() Allow = %v, want %v; reason: %s",
					verdict.Allow, tt.wantAllow, verdict.Reason)
			}

			if !tt.wantAllow && verdict.Reason == "" {
				t.Error("blocked verdict has empty Reason")
			}

			if tt.wantReasonSubstr != "" && !strings.Contains(verdict.Reason, tt.wantReasonSubstr) {
				t.Errorf("Reason = %q, want substring %q", verdict.Reason, tt.wantReasonSubstr)
			}
		})
	}
}

func TestHandlePostTeamDelete(t *testing.T) {
	t.Parallel()

	t.Run("removes existing flag", func(t *testing.T) {
		t.Parallel()

		tmpDir := t.TempDir()
		sessionDir := filepath.Join(tmpDir, "pathflow")
		createPathflowActive(t, sessionDir)

		// Verify flag exists.
		flagPath := filepath.Join(sessionDir, "is-pathflow-active")
		if _, err := os.Stat(flagPath); err != nil {
			t.Fatalf("flag should exist before removal: %v", err)
		}

		err := HandlePostTeamDelete(sessionDir)
		if err != nil {
			t.Fatalf("HandlePostTeamDelete() error = %v", err)
		}

		// Verify flag was removed.
		if _, err := os.Stat(flagPath); err == nil {
			t.Error("flag should not exist after HandlePostTeamDelete")
		}
	})

	t.Run("no error when flag does not exist", func(t *testing.T) {
		t.Parallel()

		tmpDir := t.TempDir()
		sessionDir := filepath.Join(tmpDir, "pathflow")
		// Do NOT create the flag.

		err := HandlePostTeamDelete(sessionDir)
		if err != nil {
			t.Errorf("HandlePostTeamDelete() error = %v, want nil (flag does not exist)", err)
		}
	})

	t.Run("no error when session dir does not exist", func(t *testing.T) {
		t.Parallel()

		err := HandlePostTeamDelete("/nonexistent/path/to/session/pathflow")
		if err != nil {
			t.Errorf("HandlePostTeamDelete() error = %v, want nil (dir does not exist)", err)
		}
	})
}

func TestCheckTeamDelete_BlockedReasonFormat(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	sessionDir := filepath.Join(tmpDir, "session", "pathflow")
	sentinelDir := filepath.Join(tmpDir, "sentinels")
	createPathflowActive(t, sessionDir)

	verdict, _ := CheckTeamDelete(
		strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`),
		sessionDir,
		sentinelDir,
	)

	if verdict.Allow {
		t.Fatal("expected block")
	}

	// Verify the block message contains key information.
	expectedPhrases := []string{
		"BLOCKED",
		"PathFlow is active",
		"TeamDelete",
		"PF7-END",
	}
	for _, phrase := range expectedPhrases {
		if !strings.Contains(verdict.Reason, phrase) {
			t.Errorf("Reason missing phrase %q; got: %s", phrase, verdict.Reason)
		}
	}
}
