package gate

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// createSentinel creates a sentinel file in the given directory.
func createSentinel(t *testing.T, dir, name string) {
	t.Helper()
	path := filepath.Join(dir, "pathflow-"+name)
	if err := os.WriteFile(path, []byte("1"), 0o644); err != nil {
		t.Fatalf("create sentinel %s: %v", name, err)
	}
}

func TestGateChecker_Check(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		toolName  string
		toolInput string
		sentinels []string // sentinel names to create
		wantAllow bool
		wantGate  GateType
	}{
		// --- Edit tool ---
		{
			name:      "Edit tool, no pf-3 sentinel -> BLOCK",
			toolName:  "Edit",
			toolInput: `{"file_path":"/tmp/x","old_string":"a","new_string":"b"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateEditWrite,
		},
		{
			name:      "Edit tool, pf-3 sentinel exists -> ALLOW",
			toolName:  "Edit",
			toolInput: `{"file_path":"/tmp/x","old_string":"a","new_string":"b"}`,
			sentinels: []string{"pf-3"},
			wantAllow: true,
			wantGate:  GateEditWrite,
		},

		// --- Write tool ---
		{
			name:      "Write tool, no pf-3 sentinel -> BLOCK",
			toolName:  "Write",
			toolInput: `{"file_path":"/tmp/x","content":"hello"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateEditWrite,
		},
		{
			name:      "Write tool, pf-3 sentinel exists -> ALLOW",
			toolName:  "Write",
			toolInput: `{"file_path":"/tmp/x","content":"hello"}`,
			sentinels: []string{"pf-3"},
			wantAllow: true,
			wantGate:  GateEditWrite,
		},

		// --- Bash git commit ---
		{
			name:      "Bash git commit, no pf-3 -> BLOCK",
			toolName:  "Bash",
			toolInput: `{"command":"git commit -m 'initial'"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateGitCommit,
		},
		{
			name:      "Bash git commit, pf-3 exists -> ALLOW",
			toolName:  "Bash",
			toolInput: `{"command":"git commit -m 'initial'"}`,
			sentinels: []string{"pf-3"},
			wantAllow: true,
			wantGate:  GateGitCommit,
		},

		// --- Bash git push (dual gate) ---
		{
			name:      "Bash git push, no pf-5 -> BLOCK",
			toolName:  "Bash",
			toolInput: `{"command":"git push origin main"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateGitPushPR,
		},
		{
			name:      "Bash git push, pf-5 but no ws-rev -> BLOCK",
			toolName:  "Bash",
			toolInput: `{"command":"git push origin main"}`,
			sentinels: []string{"pf-5"},
			wantAllow: false,
			wantGate:  GateGitPushPR,
		},
		{
			name:      "Bash git push, both pf-5 and ws-rev -> ALLOW",
			toolName:  "Bash",
			toolInput: `{"command":"git push origin main"}`,
			sentinels: []string{"pf-5", "ws-rev"},
			wantAllow: true,
			wantGate:  GateGitPushPR,
		},

		// --- Bash gh pr (dual gate) ---
		{
			name:      "Bash gh pr create, no sentinels -> BLOCK",
			toolName:  "Bash",
			toolInput: `{"command":"gh pr create --title 'fix'"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateGitPushPR,
		},
		{
			name:      "Bash gh pr create, both sentinels -> ALLOW",
			toolName:  "Bash",
			toolInput: `{"command":"gh pr create --title 'fix'"}`,
			sentinels: []string{"pf-5", "ws-rev"},
			wantAllow: true,
			wantGate:  GateGitPushPR,
		},

		// --- Task role teammate spawn ---
		{
			name:      "Task cf-development spawn, no pf-3 -> BLOCK",
			toolName:  "Task",
			toolInput: `{"prompt":"Read .claude/agents/cf-development.md","name":"cf-development","description":"Spawn developer"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateRoleTeammateSpawn,
		},
		{
			name:      "Task cf-development spawn, pf-3 exists -> ALLOW",
			toolName:  "Task",
			toolInput: `{"prompt":"Read .claude/agents/cf-development.md","name":"cf-development","description":"Spawn developer"}`,
			sentinels: []string{"pf-3"},
			wantAllow: true,
			wantGate:  GateRoleTeammateSpawn,
		},
		{
			name:      "Task cf-planning spawn, no pf-3 -> BLOCK",
			toolName:  "Task",
			toolInput: `{"prompt":"plan things","name":"cf-planning","description":"Spawn planner"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateRoleTeammateSpawn,
		},

		// --- Task non-role teammate (not gated) ---
		{
			name:      "Task cf-security (not role teammate), no pf-3 -> ALLOW",
			toolName:  "Task",
			toolInput: `{"prompt":"security check","name":"cf-security","description":"Spawn security"}`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  GateUngated,
		},
		{
			name:      "Task explore sub-agent, no pf-3 -> ALLOW",
			toolName:  "Task",
			toolInput: `{"prompt":"explore codebase","name":"explorer","description":"Quick lookup"}`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  GateUngated,
		},

		// --- Ungated bash commands ---
		{
			name:      "Bash ls command -> ALLOW (ungated)",
			toolName:  "Bash",
			toolInput: `{"command":"ls -la"}`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  GateUngated,
		},

		// --- Quoted string stripping ---
		{
			name:      "Bash git commit with quoted gh pr in message -> ALLOW (not push/PR)",
			toolName:  "Bash",
			toolInput: `{"command":"git commit -m \"gh pr merge completed\""}`,
			sentinels: []string{"pf-3"},
			wantAllow: true,
			wantGate:  GateGitCommit,
		},

		// --- Read tool (ungated) ---
		{
			name:      "Read tool -> ALLOW (ungated)",
			toolName:  "Read",
			toolInput: `{"file_path":"/tmp/x"}`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  GateUngated,
		},

		// --- Empty/nil inputs ---
		{
			name:      "Bash empty command -> ALLOW (ungated)",
			toolName:  "Bash",
			toolInput: `{"command":""}`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  GateUngated,
		},
		{
			name:      "Bash empty tool input -> ALLOW (ungated)",
			toolName:  "Bash",
			toolInput: ``,
			sentinels: nil,
			wantAllow: true,
			wantGate:  GateUngated,
		},
		{
			name:      "Task empty tool input -> ALLOW (ungated)",
			toolName:  "Task",
			toolInput: ``,
			sentinels: nil,
			wantAllow: true,
			wantGate:  GateUngated,
		},

		// --- Chained commands ---
		{
			name:      "Bash chained git push -> BLOCK",
			toolName:  "Bash",
			toolInput: `{"command":"echo done && git push origin main"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateGitPushPR,
		},
		{
			name:      "Bash piped gh pr -> BLOCK",
			toolName:  "Bash",
			toolInput: `{"command":"echo yes | gh pr create"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateGitPushPR,
		},

		// --- All role teammates ---
		{
			name:      "Task cf-documentation spawn, no pf-3 -> BLOCK",
			toolName:  "Task",
			toolInput: `{"prompt":"docs","name":"cf-documentation","description":"Write docs"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateRoleTeammateSpawn,
		},
		{
			name:      "Task cf-review spawn, no pf-3 -> BLOCK",
			toolName:  "Task",
			toolInput: `{"prompt":"review","name":"cf-review","description":"Code review"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateRoleTeammateSpawn,
		},
		{
			name:      "Task cf-quality-assurance spawn, no pf-3 -> BLOCK",
			toolName:  "Task",
			toolInput: `{"prompt":"qa","name":"cf-quality-assurance","description":"Run QA"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  GateRoleTeammateSpawn,
		},

		// --- Invalid JSON inputs ---
		{
			name:      "Bash invalid JSON tool input -> ALLOW (ungated)",
			toolName:  "Bash",
			toolInput: `not json`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  GateUngated,
		},
		{
			name:      "Task invalid JSON tool input -> ALLOW (ungated)",
			toolName:  "Task",
			toolInput: `not json`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  GateUngated,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			sentinelDir := t.TempDir()
			for _, s := range tt.sentinels {
				createSentinel(t, sentinelDir, s)
			}

			checker := &GateChecker{
				SentinelDir: sentinelDir,
				SessionID:   "ses-test-123",
			}

			var input json.RawMessage
			if tt.toolInput != "" {
				input = json.RawMessage(tt.toolInput)
			}

			verdict := checker.Check(tt.toolName, input)

			if verdict.Allow != tt.wantAllow {
				t.Errorf("Check() Allow = %v, want %v; reason: %s", verdict.Allow, tt.wantAllow, verdict.Reason)
			}
			if verdict.GateType != tt.wantGate {
				t.Errorf("Check() GateType = %q, want %q", verdict.GateType, tt.wantGate)
			}
			if !tt.wantAllow && verdict.Reason == "" {
				t.Error("Check() blocked but Reason is empty")
			}
		})
	}
}

func TestClassifyGateType(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		toolName  string
		toolInput string
		want      GateType
	}{
		{"Edit", "Edit", `{}`, GateEditWrite},
		{"Write", "Write", `{}`, GateEditWrite},
		{"Read", "Read", `{}`, GateUngated},
		{"Glob", "Glob", `{}`, GateUngated},
		{"Grep", "Grep", `{}`, GateUngated},
		{"Bash ls", "Bash", `{"command":"ls"}`, GateUngated},
		{"Bash git status", "Bash", `{"command":"git status"}`, GateUngated},
		{"Bash git commit", "Bash", `{"command":"git commit -m 'fix'"}`, GateGitCommit},
		{"Bash git push", "Bash", `{"command":"git push origin feat/x"}`, GateGitPushPR},
		{"Bash gh pr", "Bash", `{"command":"gh pr create"}`, GateGitPushPR},
		{"Task role", "Task", `{"name":"cf-development"}`, GateRoleTeammateSpawn},
		{"Task function", "Task", `{"name":"cf-git-operations"}`, GateUngated},
		{"Task explore", "Task", `{"name":"explore-agent"}`, GateUngated},
		{"Empty tool name", "", `{}`, GateUngated},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			var input json.RawMessage
			if tt.toolInput != "" {
				input = json.RawMessage(tt.toolInput)
			}

			got := ClassifyGateType(tt.toolName, input)
			if got != tt.want {
				t.Errorf("ClassifyGateType(%q, ...) = %q, want %q", tt.toolName, got, tt.want)
			}
		})
	}
}

func TestClassifyBashGate_QuotedStrings(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		command string
		want    GateType
	}{
		{
			name:    "git push in double quotes is stripped",
			command: `echo "git push origin main"`,
			want:    GateUngated,
		},
		{
			name:    "gh pr in single quotes is stripped",
			command: `echo 'gh pr create'`,
			want:    GateUngated,
		},
		{
			name:    "git commit message with gh pr text",
			command: `git commit -m "gh pr merge completed"`,
			want:    GateGitCommit,
		},
		{
			name:    "actual git push after quoted string",
			command: `echo "done" && git push origin main`,
			want:    GateGitPushPR,
		},
		{
			name:    "escaped quotes in double-quoted string",
			command: `echo "say \"git push\"" && ls`,
			want:    GateUngated,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			input := json.RawMessage(`{"command":` + mustMarshalString(tt.command) + `}`)
			got := classifyBashGate(input)
			if got != tt.want {
				t.Errorf("classifyBashGate(%q) = %q, want %q", tt.command, got, tt.want)
			}
		})
	}
}

// mustMarshalString marshals a string to JSON (with proper escaping).
func mustMarshalString(s string) string {
	b, err := json.Marshal(s)
	if err != nil {
		panic(err)
	}
	return string(b)
}

func TestClassifyTaskGate_RoleTeammates(t *testing.T) {
	t.Parallel()

	// Verify all 5 role teammates are detected.
	roles := []string{
		"cf-development",
		"cf-planning",
		"cf-documentation",
		"cf-review",
		"cf-quality-assurance",
	}

	for _, role := range roles {
		t.Run("name="+role, func(t *testing.T) {
			t.Parallel()

			input := json.RawMessage(`{"name":"` + role + `","prompt":"spawn","description":"test"}`)
			got := classifyTaskGate(input)
			if got != GateRoleTeammateSpawn {
				t.Errorf("classifyTaskGate(name=%q) = %q, want %q", role, got, GateRoleTeammateSpawn)
			}
		})

		t.Run("prompt_mentions="+role, func(t *testing.T) {
			t.Parallel()

			input := json.RawMessage(`{"name":"agent","prompt":"Read .claude/agents/` + role + `.md","description":"test"}`)
			got := classifyTaskGate(input)
			if got != GateRoleTeammateSpawn {
				t.Errorf("classifyTaskGate(prompt mentions %q) = %q, want %q", role, got, GateRoleTeammateSpawn)
			}
		})
	}

	// Function teammates should NOT be gated.
	funcTeammates := []string{"cf-security", "cf-knowledge-layer", "cf-git-operations"}
	for _, ft := range funcTeammates {
		t.Run("function="+ft, func(t *testing.T) {
			t.Parallel()

			input := json.RawMessage(`{"name":"` + ft + `","prompt":"spawn","description":"test"}`)
			got := classifyTaskGate(input)
			if got != GateUngated {
				t.Errorf("classifyTaskGate(name=%q) = %q, want %q (function teammates are ungated)", ft, got, GateUngated)
			}
		})
	}
}

func TestGateChecker_hasSentinel(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	createSentinel(t, dir, "pf-3")

	checker := &GateChecker{SentinelDir: dir, SessionID: "test"}

	if !checker.hasSentinel("pf-3") {
		t.Error("hasSentinel(pf-3) = false, want true")
	}
	if checker.hasSentinel("pf-5") {
		t.Error("hasSentinel(pf-5) = true, want false")
	}
	if checker.hasSentinel("ws-rev") {
		t.Error("hasSentinel(ws-rev) = true, want false")
	}

	// Verify filepath.Join handles sentinel dir with trailing separator.
	checkerTrailing := &GateChecker{SentinelDir: dir + "/", SessionID: "test"}
	if !checkerTrailing.hasSentinel("pf-3") {
		t.Error("hasSentinel(pf-3) with trailing slash = false, want true")
	}
}

func TestVerdict_BlockedHasReason(t *testing.T) {
	t.Parallel()

	dir := t.TempDir() // Empty sentinel dir.
	checker := &GateChecker{SentinelDir: dir, SessionID: "test"}

	// Edit without pf-3 should block with a reason.
	v := checker.Check("Edit", json.RawMessage(`{"file_path":"/tmp/x"}`))
	if v.Allow {
		t.Fatal("expected block for Edit without pf-3")
	}
	if v.Reason == "" {
		t.Error("blocked verdict has empty Reason")
	}
	if v.GateType != GateEditWrite {
		t.Errorf("GateType = %q, want %q", v.GateType, GateEditWrite)
	}
}

func TestGateChecker_DualGate_OnlyWSRev(t *testing.T) {
	t.Parallel()

	// ws-rev exists but pf-5 does not -> should block on pf-5.
	dir := t.TempDir()
	createSentinel(t, dir, "ws-rev")

	checker := &GateChecker{SentinelDir: dir, SessionID: "test"}
	v := checker.Check("Bash", json.RawMessage(`{"command":"git push origin main"}`))

	if v.Allow {
		t.Fatal("expected block: ws-rev exists but pf-5 missing")
	}
	if v.GateType != GateGitPushPR {
		t.Errorf("GateType = %q, want %q", v.GateType, GateGitPushPR)
	}
	if !strings.Contains(v.Reason, "pf-5") {
		t.Errorf("Reason should mention pf-5, got: %s", v.Reason)
	}
}
