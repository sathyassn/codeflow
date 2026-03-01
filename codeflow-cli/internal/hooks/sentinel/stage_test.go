package sentinel

import (
	"os"
	"path/filepath"
	"strings"
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
