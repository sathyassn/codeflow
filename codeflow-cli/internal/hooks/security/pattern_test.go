package security

import (
	"testing"
)

func TestGlobToRegex(t *testing.T) {
	t.Parallel()

	tests := []struct {
		glob    string
		input   string
		matches bool
	}{
		{".claude/hooks/**", ".claude/hooks/pre-tool-use/script.sh", true},
		{".claude/hooks/**", ".claude/hooks/", true},
		{".codeflow/config/**", ".codeflow/config/enforcement/policy.json", true},
		{"*.json", "settings.json", true},
		{"*.json", "settings.txt", false},
		{".claude/settings.json", ".claude/settings.json", true},
		{".claude/settings.json", ".claude/settings.local.json", false},
	}

	for _, tt := range tests {
		t.Run(tt.glob+"_"+tt.input, func(t *testing.T) {
			t.Parallel()
			got := matchesExtendedGlob(tt.input, tt.glob)
			if got != tt.matches {
				t.Errorf("matchesExtendedGlob(%q, %q) = %v, want %v", tt.input, tt.glob, got, tt.matches)
			}
		})
	}
}

func TestIsPathTargeted(t *testing.T) {
	t.Parallel()

	tests := []struct {
		cmd      string
		path     string
		targeted bool
	}{
		{"rm -rf .claude/settings.json", ".claude/settings.json", true},
		{"cat .claude/settings.json", ".claude/settings.json", true},
		{"rm .claude-notes.md", ".claude", false}, // substring not at boundary
		{"cp /tmp/claude/x /tmp/claude/y", ".claude", false}, // only in /tmp/claude
		{"rm .claude/hooks/script.sh", ".claude/hooks", true},
		{"echo hello", ".claude", false},
	}

	for _, tt := range tests {
		t.Run(tt.cmd, func(t *testing.T) {
			t.Parallel()
			got := isPathTargeted(tt.cmd, tt.path)
			if got != tt.targeted {
				t.Errorf("isPathTargeted(%q, %q) = %v, want %v", tt.cmd, tt.path, got, tt.targeted)
			}
		})
	}
}

func TestSplitCommandSegments(t *testing.T) {
	t.Parallel()

	tests := []struct {
		cmd  string
		want int
	}{
		{"ls", 1},
		{"ls && pwd", 2},
		{"a || b", 2},
		{"a; b; c", 3},
		{"a | b", 2},
		{`echo "a && b"`, 1}, // inside quotes
		{`echo 'a || b'`, 1},
		{"a && b; c || d", 4},
	}

	for _, tt := range tests {
		t.Run(tt.cmd, func(t *testing.T) {
			t.Parallel()
			got := splitCommandSegments(tt.cmd)
			if len(got) != tt.want {
				t.Errorf("splitCommandSegments(%q) = %d segments, want %d: %v", tt.cmd, len(got), tt.want, got)
			}
		})
	}
}

func TestGetFlagsPortion(t *testing.T) {
	t.Parallel()

	tests := []struct {
		cmd  string
		want string
	}{
		{"git commit -m 'message'", "git commit"},
		{"git commit --message='msg'", "git commit"},
		{`git commit -m"msg"`, "git commit"},
		{"git commit -n", "git commit -n"},
	}

	for _, tt := range tests {
		t.Run(tt.cmd, func(t *testing.T) {
			t.Parallel()
			got := getFlagsPortion(tt.cmd)
			if got != tt.want {
				t.Errorf("getFlagsPortion(%q) = %q, want %q", tt.cmd, got, tt.want)
			}
		})
	}
}

func TestExtractVariableAssignments(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		segments []string
		want     map[string]string
	}{
		{
			"simple assignment",
			[]string{`F=".claude/settings.json"`},
			map[string]string{"F": ".claude/settings.json"},
		},
		{
			"single-quoted value",
			[]string{`F='.claude/settings.json'`},
			map[string]string{"F": ".claude/settings.json"},
		},
		{
			"unquoted value",
			[]string{"DIR=.claude"},
			map[string]string{"DIR": ".claude"},
		},
		{
			"multiple segments",
			[]string{`F=".claude/settings.json"`, "rm $F"},
			map[string]string{"F": ".claude/settings.json"},
		},
		{
			"no assignments",
			[]string{"rm -rf /tmp/safe"},
			map[string]string{},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := extractVariableAssignments(tt.segments)
			if len(got) != len(tt.want) {
				t.Fatalf("extractVariableAssignments(%v) returned %d entries, want %d: %v", tt.segments, len(got), len(tt.want), got)
			}
			for k, v := range tt.want {
				if got[k] != v {
					t.Errorf("extractVariableAssignments(%v)[%q] = %q, want %q", tt.segments, k, got[k], v)
				}
			}
		})
	}
}

func TestHasVariableIndirection(t *testing.T) {
	t.Parallel()

	tests := []struct {
		cmd  string
		want bool
	}{
		{"rm $F", true},
		{`rm "$F"`, true},
		{"rm ${DIR}", true},
		{"echo $PATH", true},
		{"echo $?", false},       // special variable
		{"echo $$", false},       // special variable
		{"echo $!", false},       // special variable
		{"echo $0", false},       // special variable
		{"echo $@", false},       // special variable
		{"echo $*", false},       // special variable
		{"echo $#", false},       // special variable
		{"rm some-file.txt", false},
		{"echo hello world", false},
	}

	for _, tt := range tests {
		t.Run(tt.cmd, func(t *testing.T) {
			t.Parallel()
			got := hasVariableIndirection(tt.cmd)
			if got != tt.want {
				t.Errorf("hasVariableIndirection(%q) = %v, want %v", tt.cmd, got, tt.want)
			}
		})
	}
}

func TestNormalizePath(t *testing.T) {
	t.Parallel()

	tests := []struct {
		input string
		want  string
	}{
		{"./src/main.go", "src/main.go"},
		{"src/", "src"},
		{"a//b//c", "a/b/c"},
		{"../../../etc/passwd", "etc/passwd"},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			t.Parallel()
			got := normalizePath(tt.input)
			if got != tt.want {
				t.Errorf("normalizePath(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}
