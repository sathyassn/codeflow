package ghpr

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// stubResolver is a test double that returns a fixed target branch.
type stubResolver struct {
	targetBranch string
}

func (s stubResolver) ResolveTargetBranch(_ string) string {
	return s.targetBranch
}

func TestPRChecker_CheckWithResolver(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name         string
		branches     []string
		stdin        string
		targetBranch string // what the resolver returns
		wantAllow    bool
		wantSubstr   string // expected substring in Reason when blocked
	}{
		// --- Non-Bash tools ---
		{
			name:      "non-Bash tool allowed",
			branches:  DefaultProtectedBranches,
			stdin:     `{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`,
			wantAllow: true,
		},
		{
			name:      "Edit tool allowed",
			branches:  DefaultProtectedBranches,
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x","old_string":"a","new_string":"b"}}`,
			wantAllow: true,
		},

		// --- Non-merge commands ---
		{
			name:      "gh pr create allowed",
			branches:  DefaultProtectedBranches,
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"gh pr create --title 'test' --body '## Summary\ntest\n## Testing\ntest'"}}`,
			wantAllow: true,
		},
		{
			name:      "gh pr view allowed",
			branches:  DefaultProtectedBranches,
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"gh pr view 42"}}`,
			wantAllow: true,
		},
		{
			name:      "git push allowed (not gh pr merge)",
			branches:  DefaultProtectedBranches,
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"git push origin feat/my-feature"}}`,
			wantAllow: true,
		},

		// --- Empty / malformed inputs ---
		{
			name:      "empty stdin allowed",
			branches:  DefaultProtectedBranches,
			stdin:     "",
			wantAllow: true,
		},
		{
			name:      "invalid JSON allowed (graceful degradation)",
			branches:  DefaultProtectedBranches,
			stdin:     "not json",
			wantAllow: true,
		},
		{
			name:      "empty command allowed",
			branches:  DefaultProtectedBranches,
			stdin:     `{"tool_name":"Bash","tool_input":{"command":""}}`,
			wantAllow: true,
		},
		{
			name:      "empty tool_input allowed",
			branches:  DefaultProtectedBranches,
			stdin:     `{"tool_name":"Bash","tool_input":{}}`,
			wantAllow: true,
		},
		{
			name:      "null tool_input allowed",
			branches:  DefaultProtectedBranches,
			stdin:     `{"tool_name":"Bash"}`,
			wantAllow: true,
		},

		// --- gh pr merge without PR number ---
		{
			name:      "gh pr merge without PR number allowed (no target branch to resolve)",
			branches:  DefaultProtectedBranches,
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"gh pr merge"}}`,
			wantAllow: true,
		},

		// --- gh pr merge with unresolvable PR ---
		{
			name:         "gh pr merge with unresolvable PR allowed",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 999"}}`,
			targetBranch: "", // resolver returns empty
			wantAllow:    true,
		},

		// --- Protected branch blocking ---
		{
			name:         "merge to main blocked",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "main",
			wantAllow:    false,
			wantSubstr:   "BLOCKED",
		},
		{
			name:         "merge to master blocked",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "master",
			wantAllow:    false,
			wantSubstr:   "BLOCKED",
		},
		{
			name:         "merge to production blocked",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "production",
			wantAllow:    false,
			wantSubstr:   "BLOCKED",
		},
		{
			name:         "merge to release/v1.0 blocked (wildcard match)",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "release/v1.0",
			wantAllow:    false,
			wantSubstr:   "release/*",
		},
		{
			name:         "merge to release/v2.3.1 blocked (wildcard match)",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 123"}}`,
			targetBranch: "release/v2.3.1",
			wantAllow:    false,
			wantSubstr:   "release/*",
		},

		// --- Allowed merges ---
		{
			name:         "merge to feat/my-feature allowed",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "feat/my-feature",
			wantAllow:    true,
		},
		{
			name:         "merge to develop allowed",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "develop",
			wantAllow:    true,
		},
		{
			name:         "merge to integration allowed",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "integration",
			wantAllow:    true,
		},

		// --- Block message content ---
		{
			name:         "block message includes PR number",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 77"}}`,
			targetBranch: "main",
			wantAllow:    false,
			wantSubstr:   "#77",
		},
		{
			name:         "block message includes target branch name",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "main",
			wantAllow:    false,
			wantSubstr:   "'main'",
		},
		{
			name:         "block message includes matched pattern",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "release/v1.0",
			wantAllow:    false,
			wantSubstr:   "Matched protection pattern: release/*",
		},

		// --- Custom protected branches ---
		{
			name:         "custom branch staging blocked",
			branches:     []string{"staging", "main"},
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "staging",
			wantAllow:    false,
			wantSubstr:   "staging",
		},
		{
			name:         "custom branch deploy/* blocked",
			branches:     []string{"deploy/*"},
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "deploy/prod",
			wantAllow:    false,
			wantSubstr:   "deploy/*",
		},

		// --- gh pr merge with flags ---
		{
			name:         "merge with --delete-branch to main blocked",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42 --delete-branch"}}`,
			targetBranch: "main",
			wantAllow:    false,
			wantSubstr:   "BLOCKED",
		},
		{
			name:         "merge with --squash to main blocked",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42 --squash"}}`,
			targetBranch: "main",
			wantAllow:    false,
			wantSubstr:   "BLOCKED",
		},
		{
			name:         "merge with --merge flag to feat allowed",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42 --merge"}}`,
			targetBranch: "feat/test",
			wantAllow:    true,
		},

		// --- Command in pipeline ---
		{
			name:         "gh pr merge in pipeline detected",
			branches:     DefaultProtectedBranches,
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"echo ok && gh pr merge 42"}}`,
			targetBranch: "main",
			wantAllow:    false,
			wantSubstr:   "BLOCKED",
		},

		// --- Empty protected branches list ---
		{
			name:         "empty protected branches list allows all merges",
			branches:     []string{},
			stdin:        `{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`,
			targetBranch: "main",
			wantAllow:    true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			checker := &PRChecker{ProtectedBranches: tt.branches}
			resolver := stubResolver{targetBranch: tt.targetBranch}

			verdict, err := checker.CheckWithResolver(strings.NewReader(tt.stdin), resolver)
			if err != nil {
				t.Fatalf("CheckWithResolver() unexpected error: %v", err)
			}

			if verdict.Allow != tt.wantAllow {
				t.Errorf("Allow = %v, want %v (reason: %s)", verdict.Allow, tt.wantAllow, verdict.Reason)
			}

			if tt.wantSubstr != "" && !strings.Contains(verdict.Reason, tt.wantSubstr) {
				t.Errorf("Reason = %q, want substring %q", verdict.Reason, tt.wantSubstr)
			}
		})
	}
}

func TestMatchBranch(t *testing.T) {
	t.Parallel()

	tests := []struct {
		branch  string
		pattern string
		want    bool
	}{
		// Exact matches.
		{"main", "main", true},
		{"master", "master", true},
		{"production", "production", true},
		{"develop", "main", false},
		{"main-backup", "main", false},

		// Glob wildcard matches.
		{"release/v1.0", "release/*", true},
		{"release/v2.3.1", "release/*", true},
		{"release/hotfix-42", "release/*", true},
		{"releases/v1.0", "release/*", false}, // "releases" != "release"
		{"release", "release/*", false},        // no slash after "release"

		// Other wildcard patterns.
		{"deploy/prod", "deploy/*", true},
		{"deploy/staging", "deploy/*", true},
		{"feature/test", "feature/*", true},
		{"hotfix/urgent", "hotfix/*", true},

		// No wildcard in pattern, no match.
		{"feat/test", "main", false},
	}

	for _, tt := range tests {
		t.Run(tt.branch+"_vs_"+tt.pattern, func(t *testing.T) {
			t.Parallel()

			got := matchBranch(tt.branch, tt.pattern)
			if got != tt.want {
				t.Errorf("matchBranch(%q, %q) = %v, want %v", tt.branch, tt.pattern, got, tt.want)
			}
		})
	}
}

func TestLoadProtectedBranches(t *testing.T) {
	t.Parallel()

	t.Run("reads from real enforcement-policy.json", func(t *testing.T) {
		t.Parallel()

		// Use the actual project root to verify real config loading.
		// Find it by walking up from test location.
		projectDir := findProjectRoot(t)
		if projectDir == "" {
			t.Skip("could not find project root with enforcement-policy.json")
		}

		branches := LoadProtectedBranches(projectDir)
		if len(branches) == 0 {
			t.Fatal("LoadProtectedBranches() returned empty slice from real config")
		}

		// Verify the real config includes "main".
		found := false
		for _, b := range branches {
			if b == "main" {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("LoadProtectedBranches() = %v, want to include 'main'", branches)
		}
	})

	t.Run("returns defaults when config missing", func(t *testing.T) {
		t.Parallel()

		branches := LoadProtectedBranches(t.TempDir())
		if len(branches) != len(DefaultProtectedBranches) {
			t.Errorf("LoadProtectedBranches(missing) = %v, want %v", branches, DefaultProtectedBranches)
		}
		for i, b := range branches {
			if b != DefaultProtectedBranches[i] {
				t.Errorf("branch[%d] = %q, want %q", i, b, DefaultProtectedBranches[i])
			}
		}
	})

	t.Run("returns defaults when config is invalid JSON", func(t *testing.T) {
		t.Parallel()

		dir := t.TempDir()
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		if err := os.MkdirAll(configDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte("invalid json"), 0o644); err != nil {
			t.Fatal(err)
		}

		branches := LoadProtectedBranches(dir)
		if len(branches) != len(DefaultProtectedBranches) {
			t.Errorf("LoadProtectedBranches(invalid JSON) = %v, want defaults %v", branches, DefaultProtectedBranches)
		}
	})

	t.Run("returns defaults when merge_protection.protected_branches is empty", func(t *testing.T) {
		t.Parallel()

		dir := t.TempDir()
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		if err := os.MkdirAll(configDir, 0o755); err != nil {
			t.Fatal(err)
		}
		config := `{"merge_protection": {"protected_branches": []}}`
		if err := os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(config), 0o644); err != nil {
			t.Fatal(err)
		}

		branches := LoadProtectedBranches(dir)
		if len(branches) != len(DefaultProtectedBranches) {
			t.Errorf("LoadProtectedBranches(empty array) = %v, want defaults %v", branches, DefaultProtectedBranches)
		}
	})

	t.Run("loads custom branches from config", func(t *testing.T) {
		t.Parallel()

		dir := t.TempDir()
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		if err := os.MkdirAll(configDir, 0o755); err != nil {
			t.Fatal(err)
		}
		config := `{"merge_protection": {"protected_branches": ["staging", "prod/*"]}}`
		if err := os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(config), 0o644); err != nil {
			t.Fatal(err)
		}

		branches := LoadProtectedBranches(dir)
		want := []string{"staging", "prod/*"}
		if len(branches) != len(want) {
			t.Fatalf("LoadProtectedBranches() = %v, want %v", branches, want)
		}
		for i, b := range branches {
			if b != want[i] {
				t.Errorf("branch[%d] = %q, want %q", i, b, want[i])
			}
		}
	})
}

func TestGHPRMergeRegex(t *testing.T) {
	t.Parallel()

	tests := []struct {
		command string
		want    bool
	}{
		{"gh pr merge 42", true},
		{"gh pr merge 42 --squash", true},
		{"gh pr merge 42 --delete-branch", true},
		{"echo ok && gh pr merge 42", true},
		{"gh pr create --title test", false},
		{"gh pr view 42", false},
		{"git push origin main", false},
		{"echo gh pr merge", true}, // matches the pattern
		{"", false},
	}

	for _, tt := range tests {
		t.Run(tt.command, func(t *testing.T) {
			t.Parallel()

			got := ghPRMergeRe.MatchString(tt.command)
			if got != tt.want {
				t.Errorf("ghPRMergeRe.MatchString(%q) = %v, want %v", tt.command, got, tt.want)
			}
		})
	}
}

func TestPRNumberExtraction(t *testing.T) {
	t.Parallel()

	tests := []struct {
		command string
		want    string
	}{
		{"gh pr merge 42", "42"},
		{"gh pr merge 123 --squash", "123"},
		{"gh pr merge 99 --delete-branch", "99"},
		{"echo ok && gh pr merge 7", "7"},
		{"gh pr merge", ""},         // no PR number
		{"gh pr merge --squash", ""}, // no PR number, just flags
	}

	for _, tt := range tests {
		t.Run(tt.command, func(t *testing.T) {
			t.Parallel()

			matches := prNumberRe.FindStringSubmatch(tt.command)
			var got string
			if len(matches) >= 2 {
				got = matches[1]
			}
			if got != tt.want {
				t.Errorf("prNumberRe.FindStringSubmatch(%q) = %q, want %q", tt.command, got, tt.want)
			}
		})
	}
}

func TestDefaultProtectedBranches(t *testing.T) {
	t.Parallel()

	// Verify the default list matches what enforcement-policy.json typically contains.
	expected := []string{"main", "master", "release/*", "production"}
	if len(DefaultProtectedBranches) != len(expected) {
		t.Fatalf("DefaultProtectedBranches = %v, want %v", DefaultProtectedBranches, expected)
	}
	for i, b := range DefaultProtectedBranches {
		if b != expected[i] {
			t.Errorf("DefaultProtectedBranches[%d] = %q, want %q", i, b, expected[i])
		}
	}
}

func TestCheck_UsesDefaultResolver(t *testing.T) {
	t.Parallel()

	// The default resolver returns empty string, so the check should allow through.
	checker := &PRChecker{ProtectedBranches: DefaultProtectedBranches}
	verdict, err := checker.Check(strings.NewReader(`{"tool_name":"Bash","tool_input":{"command":"gh pr merge 42"}}`))
	if err != nil {
		t.Fatalf("Check() unexpected error: %v", err)
	}
	// Default resolver returns "" -> cannot resolve target branch -> allow.
	if !verdict.Allow {
		t.Errorf("Check() with default resolver blocked, want allow (resolver returns empty)")
	}
}

// findProjectRoot walks up directories to find the project root containing
// .codeflow/config/enforcement/enforcement-policy.json.
func findProjectRoot(t *testing.T) string {
	t.Helper()

	dir, err := os.Getwd()
	if err != nil {
		return ""
	}

	// Walk up to 10 levels.
	for i := 0; i < 10; i++ {
		configPath := filepath.Join(dir, ".codeflow", "config", "enforcement", "enforcement-policy.json")
		if _, err := os.Stat(configPath); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}

	return ""
}
