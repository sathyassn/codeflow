package resource

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/hooks/security"
)

func TestNewProtectionGuard(t *testing.T) {
	t.Parallel()

	// With no policy file, guard should use defaults.
	guard := NewProtectionGuard(t.TempDir())
	if guard.Policy == nil {
		t.Fatal("Policy should not be nil (should fall back to default)")
	}
	if len(guard.Policy.ProtectedResources.Critical) == 0 {
		t.Error("DefaultPolicy should have critical paths")
	}
}

func newTestGuard(t *testing.T) *ProtectionGuard {
	t.Helper()
	policy := security.DefaultPolicy()
	return &ProtectionGuard{
		Policy:      policy,
		ProjectDir:  "/project",
		ProjectRoot: "project",
	}
}

func TestCheck(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		stdin     string
		wantAllow bool
		wantTier  ProtectionTier
	}{
		{
			name:      "empty stdin allows",
			stdin:     "",
			wantAllow: true,
			wantTier:  TierNone,
		},
		{
			name:      "invalid JSON allows",
			stdin:     "not json",
			wantAllow: true,
			wantTier:  TierNone,
		},
		{
			name:      "non Edit/Write tool allows",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"ls"}}`,
			wantAllow: true,
			wantTier:  TierNone,
		},
		{
			name:      "Edit with no file_path allows",
			stdin:     `{"tool_name":"Edit","tool_input":{}}`,
			wantAllow: true,
			wantTier:  TierNone,
		},
		{
			name:      "Edit unprotected file allows",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/project/src/main.go"}}`,
			wantAllow: true,
			wantTier:  TierNone,
		},
		{
			name:      "Edit critical file blocks",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/project/.claude/settings.json"}}`,
			wantAllow: false,
			wantTier:  TierCritical,
		},
		{
			name:      "Write critical file blocks",
			stdin:     `{"tool_name":"Write","tool_input":{"file_path":"/project/.claude/settings.local.json"}}`,
			wantAllow: false,
			wantTier:  TierCritical,
		},
		{
			name:      "Edit high tier file blocks",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/project/.claude/hooks/codeflow/pre-tool-use/test.sh"}}`,
			wantAllow: false,
			wantTier:  TierHigh,
		},
		{
			name:      "Edit moderate file allows with warning",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/project/project/mission.md"}}`,
			wantAllow: true,
			wantTier:  TierModerate,
		},
		{
			name:      "staging path always allowed",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/project/managed/protected-edits/.claude/settings.json"}}`,
			wantAllow: true,
			wantTier:  TierNone,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			guard := newTestGuard(t)

			verdict, err := guard.Check(strings.NewReader(tt.stdin))
			if err != nil {
				t.Fatalf("Check() error = %v", err)
			}

			if verdict.Allow != tt.wantAllow {
				t.Errorf("Check() Allow = %v, want %v; message: %s", verdict.Allow, tt.wantAllow, verdict.Message)
			}
			if verdict.Tier != tt.wantTier {
				t.Errorf("Check() Tier = %q, want %q", verdict.Tier, tt.wantTier)
			}
		})
	}
}

func TestCheckPath(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		path      string
		wantAllow bool
		wantTier  ProtectionTier
	}{
		{
			name:      "unprotected path",
			path:      "/project/src/main.go",
			wantAllow: true,
			wantTier:  TierNone,
		},
		{
			name:      "critical path",
			path:      "/project/.claude/settings.json",
			wantAllow: false,
			wantTier:  TierCritical,
		},
		{
			name:      "high tier path",
			path:      "/project/.codeflow/config/enforcement/policy.json",
			wantAllow: false,
			wantTier:  TierHigh,
		},
		{
			name:      "moderate path",
			path:      "/project/project/mission.md",
			wantAllow: true,
			wantTier:  TierModerate,
		},
		{
			name:      "staging path bypasses protection",
			path:      "/tmp/claude/project/managed/protected-edits/.claude/settings.json",
			wantAllow: true,
			wantTier:  TierNone,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			guard := newTestGuard(t)

			verdict := guard.CheckPath(tt.path)
			if verdict.Allow != tt.wantAllow {
				t.Errorf("CheckPath() Allow = %v, want %v; message: %s", verdict.Allow, tt.wantAllow, verdict.Message)
			}
			if verdict.Tier != tt.wantTier {
				t.Errorf("CheckPath() Tier = %q, want %q", verdict.Tier, tt.wantTier)
			}
		})
	}
}

func TestBlockVerdict(t *testing.T) {
	t.Parallel()

	guard := newTestGuard(t)
	verdict := guard.blockVerdict(TierCritical, ".claude/settings.json")

	if verdict.Allow {
		t.Error("blockVerdict should not allow")
	}
	if verdict.Tier != TierCritical {
		t.Errorf("Tier = %q, want critical", verdict.Tier)
	}

	// Verify the message contains staging workflow instructions.
	expectedPhrases := []string{
		"BLOCKED",
		"Critical",
		"AUTO-STAGING",
		"mkdir -p",
		"cp",
		".claude/settings.json",
	}
	for _, phrase := range expectedPhrases {
		if !strings.Contains(verdict.Message, phrase) {
			t.Errorf("Message missing phrase %q; got: %s", phrase, verdict.Message)
		}
	}
}

func TestFormatBlockJSON(t *testing.T) {
	t.Parallel()

	verdict := &Verdict{
		Allow:   false,
		Tier:    TierCritical,
		Path:    ".claude/settings.json",
		Message: "BLOCKED: Critical\nPath: .claude/settings.json",
	}

	got := FormatBlockJSON(verdict)

	if !strings.Contains(got, `"tier":"critical"`) {
		t.Errorf("JSON should contain tier; got: %s", got)
	}
	if !strings.Contains(got, `"path":".claude/settings.json"`) {
		t.Errorf("JSON should contain path; got: %s", got)
	}
	// Newlines should be escaped.
	if strings.Contains(got, "\n") {
		t.Error("FormatBlockJSON should escape newlines")
	}
	// Output must be valid JSON.
	var parsed map[string]any
	if err := json.Unmarshal([]byte(got), &parsed); err != nil {
		t.Errorf("FormatBlockJSON should produce valid JSON; got error: %v, output: %s", err, got)
	}
}

func TestFormatBlockJSON_SpecialChars(t *testing.T) {
	t.Parallel()

	verdict := &Verdict{
		Allow:   false,
		Tier:    TierHigh,
		Path:    "path/with\"quotes",
		Message: "tab:\there\rnewline:\nbackslash:\\end",
	}

	got := FormatBlockJSON(verdict)

	// Must be valid JSON despite special characters.
	var parsed map[string]any
	if err := json.Unmarshal([]byte(got), &parsed); err != nil {
		t.Errorf("FormatBlockJSON should handle special chars; got error: %v, output: %s", err, got)
	}

	pg, ok := parsed["protectionGuard"].(map[string]any)
	if !ok {
		t.Fatalf("expected protectionGuard object; got: %s", got)
	}
	if pg["tier"] != "high" {
		t.Errorf("expected tier 'high'; got: %v", pg["tier"])
	}
	if pg["path"] != "path/with\"quotes" {
		t.Errorf("expected path with quotes; got: %v", pg["path"])
	}
	if msg, ok := pg["message"].(string); !ok || !strings.Contains(msg, "tab:\t") {
		t.Errorf("expected message with tab; got: %v", pg["message"])
	}
}

func TestMatchesPattern(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		path    string
		pattern string
		want    bool
	}{
		{
			name:    "exact match",
			path:    ".claude/settings.json",
			pattern: ".claude/settings.json",
			want:    true,
		},
		{
			name:    "no match",
			path:    "src/main.go",
			pattern: ".claude/settings.json",
			want:    false,
		},
		{
			name:    "double star recursive",
			path:    ".claude/hooks/codeflow/pre-tool-use/test.sh",
			pattern: ".claude/hooks/codeflow/**",
			want:    true,
		},
		{
			name:    "double star no match",
			path:    "src/main.go",
			pattern: ".claude/hooks/**",
			want:    false,
		},
		{
			name:    "single star",
			path:    "release/v1.0",
			pattern: "release/*",
			want:    true,
		},
		{
			name:    "single star no match",
			path:    "other/v1.0",
			pattern: "release/*",
			want:    false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := matchesPattern(tt.path, tt.pattern)
			if got != tt.want {
				t.Errorf("matchesPattern(%q, %q) = %v, want %v", tt.path, tt.pattern, got, tt.want)
			}
		})
	}
}

func TestNormalizePath(t *testing.T) {
	t.Parallel()

	guard := &ProtectionGuard{ProjectDir: "/home/user/project"}

	tests := []struct {
		name string
		path string
		want string
	}{
		{
			name: "absolute in project",
			path: "/home/user/project/src/main.go",
			want: "src/main.go",
		},
		{
			name: "already relative",
			path: "src/main.go",
			want: "src/main.go",
		},
		{
			name: "different project",
			path: "/home/user/other/src/main.go",
			want: "/home/user/other/src/main.go",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := guard.normalizePath(tt.path)
			if got != tt.want {
				t.Errorf("normalizePath(%q) = %q, want %q", tt.path, got, tt.want)
			}
		})
	}
}

func TestIsStagingPath(t *testing.T) {
	t.Parallel()

	guard := &ProtectionGuard{ProjectRoot: "myproject"}

	tests := []struct {
		name string
		path string
		want bool
	}{
		{
			name: "staging path exact base",
			path: "/tmp/claude/myproject/managed/protected-edits/file.txt",
			want: true,
		},
		{
			name: "staging path other project",
			path: "/tmp/claude/otherproject/managed/protected-edits/file.txt",
			want: true, // matches the generic /tmp/claude/ + managed/protected-edits pattern
		},
		{
			name: "non-staging tmp path",
			path: "/tmp/claude/myproject/other/file.txt",
			want: false,
		},
		{
			name: "project source path",
			path: "/project/src/main.go",
			want: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := guard.isStagingPath(tt.path)
			if got != tt.want {
				t.Errorf("isStagingPath(%q) = %v, want %v", tt.path, got, tt.want)
			}
		})
	}
}

func TestStagingBase(t *testing.T) {
	t.Parallel()

	t.Run("default", func(t *testing.T) {
		t.Parallel()
		guard := &ProtectionGuard{ProjectRoot: "myproject"}
		want := "/tmp/claude/myproject/managed/protected-edits"
		if got := guard.stagingBase(); got != want {
			t.Errorf("stagingBase() = %q, want %q", got, want)
		}
	})

	t.Run("override", func(t *testing.T) {
		t.Parallel()
		guard := &ProtectionGuard{
			ProjectRoot: "myproject",
			StagingBase: "/custom/staging",
		}
		want := "/custom/staging"
		if got := guard.stagingBase(); got != want {
			t.Errorf("stagingBase() = %q, want %q", got, want)
		}
	})
}

func TestCapitalizeFirst(t *testing.T) {
	t.Parallel()

	tests := []struct {
		input string
		want  string
	}{
		{"critical", "Critical"},
		{"high", "High"},
		{"moderate", "Moderate"},
		{"", ""},
		{"A", "A"},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			t.Parallel()
			if got := capitalizeFirst(tt.input); got != tt.want {
				t.Errorf("capitalizeFirst(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}
