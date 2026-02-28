package welcome

import (
	"bytes"
	"os"
	"strings"
	"testing"
	"time"
)

// renderToString renders the welcome screen with the given options and returns
// the output as a string.
func renderToString(t *testing.T, opts ...Option) string {
	t.Helper()
	var buf bytes.Buffer
	// Default: no-color and fixed width for deterministic tests.
	defaults := []Option{
		WithNoColor(true),
		WithTermWidth(80),
		WithOutputFd(0), // fd 0 is not a terminal in tests
	}
	Show(&buf, append(defaults, opts...)...)
	return buf.String()
}

func TestShowFullDisplay(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithVersion("0.6.0"),
		WithClaudeCodeVersion("v1.0.25"),
		WithProject("codeflow", "Active"),
		WithActiveWork("INF-TSK-015-007", "Welcome screen package", "feat/welcome-screen",
			time.Now().Add(-2*time.Hour)),
	)

	// Verify box borders present.
	for _, ch := range []string{"╔", "╗", "╚", "╝", "║", "╠", "╣"} {
		if !strings.Contains(output, ch) {
			t.Errorf("output should contain box character %q", ch)
		}
	}

	// Verify block logo present (6 lines).
	if !strings.Contains(output, "██████╗") {
		t.Error("output should contain block logo characters")
	}

	// Verify tagline.
	if !strings.Contains(output, "AI-Powered Software Development Workflow") {
		t.Error("output should contain tagline")
	}

	// Verify version.
	if !strings.Contains(output, "v0.6.0") {
		t.Error("output should contain version")
	}

	// Verify Claude Code version.
	if !strings.Contains(output, "v1.0.25") {
		t.Error("output should contain Claude Code version")
	}

	// Verify project info.
	if !strings.Contains(output, "PROJECT:") {
		t.Error("output should contain PROJECT label")
	}
	if !strings.Contains(output, "codeflow") {
		t.Error("output should contain project name")
	}
	if !strings.Contains(output, "STATUS:") {
		t.Error("output should contain STATUS label")
	}

	// Verify active work.
	if !strings.Contains(output, "ACTIVE WORK:") {
		t.Error("output should contain ACTIVE WORK section")
	}
	if !strings.Contains(output, "INF-TSK-015-007") {
		t.Error("output should contain task ID")
	}
	if !strings.Contains(output, "Welcome screen package") {
		t.Error("output should contain task title")
	}
	if !strings.Contains(output, "feat/welcome-screen") {
		t.Error("output should contain branch name")
	}

	// Verify quick start.
	if !strings.Contains(output, "QUICK START:") {
		t.Error("output should contain QUICK START section")
	}
	if !strings.Contains(output, "/cf-resume") {
		t.Error("output should contain /cf-resume when active work exists")
	}

	// Verify prompt.
	if !strings.Contains(output, "Press Enter to continue") {
		t.Error("output should contain interactive prompt")
	}
}

func TestShowNoActiveWork(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithVersion("0.6.0"),
	)

	if !strings.Contains(output, "NO ACTIVE WORK") {
		t.Error("output should show 'NO ACTIVE WORK' when no active work")
	}
	if !strings.Contains(output, "/cf-plan") {
		t.Error("output should show /cf-plan when no active work")
	}
	if !strings.Contains(output, "/cf-develop") {
		t.Error("output should show /cf-develop when no active work")
	}
	if strings.Contains(output, "/cf-resume") {
		t.Error("output should NOT show /cf-resume when no active work")
	}
}

func TestShowMissingConfig(t *testing.T) {
	t.Parallel()
	// Minimal call with no options — should render gracefully with defaults.
	output := renderToString(t)

	if !strings.Contains(output, "PROJECT:") {
		t.Error("output should still show PROJECT section with defaults")
	}
	if !strings.Contains(output, "codeflow") {
		t.Error("default project name should be 'codeflow'")
	}
	if !strings.Contains(output, "dev") {
		t.Error("default version should be 'dev'")
	}
}

func TestShowVersionRendering(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithVersion("1.2.3"),
		WithClaudeCodeVersion("v2.0.0"),
	)

	if !strings.Contains(output, "v1.2.3") {
		t.Error("output should contain CodeFlow version v1.2.3")
	}
	if !strings.Contains(output, "v2.0.0") {
		t.Error("output should contain Claude Code version v2.0.0")
	}
}

func TestShowBoxDrawingCorrectness(t *testing.T) {
	t.Parallel()
	output := renderToString(t)
	lines := strings.Split(output, "\n")

	// First non-empty line should start with ╔ and end with ╗.
	firstLine := lines[0]
	if !strings.HasPrefix(firstLine, "╔") {
		t.Errorf("first line should start with ╔, got %q", firstLine[:4])
	}
	if !strings.HasSuffix(strings.TrimSpace(firstLine), "╗") {
		t.Errorf("first line should end with ╗, got %q", firstLine)
	}

	// Last non-empty line should start with ╚ and end with ╝.
	var lastLine string
	for i := len(lines) - 1; i >= 0; i-- {
		if strings.TrimSpace(lines[i]) != "" {
			lastLine = lines[i]
			break
		}
	}
	if !strings.HasPrefix(lastLine, "╚") {
		t.Errorf("last line should start with ╚, got %q", lastLine[:4])
	}
	if !strings.HasSuffix(strings.TrimSpace(lastLine), "╝") {
		t.Errorf("last line should end with ╝, got %q", lastLine)
	}

	// Content lines should have ║ borders.
	for i, line := range lines {
		if i == 0 || strings.TrimSpace(line) == "" {
			continue
		}
		trimmed := strings.TrimSpace(line)
		if trimmed == "" {
			continue
		}
		// Skip top, bottom, and separator lines.
		if strings.HasPrefix(trimmed, "╔") || strings.HasPrefix(trimmed, "╚") ||
			strings.HasPrefix(trimmed, "╠") {
			continue
		}
		if !strings.HasPrefix(trimmed, "║") {
			t.Errorf("line %d should start with ║: %q", i, trimmed[:min(10, len(trimmed))])
		}
	}
}

func TestShowBlockLogoPresence(t *testing.T) {
	t.Parallel()
	output := renderToString(t, WithTermWidth(80))
	lines := strings.Split(output, "\n")

	// The 6-line block logo: 5 lines contain "██" and the 6th line uses
	// "╚═════╝" characters. Count lines that match either pattern.
	logoLineCount := 0
	for _, line := range lines {
		if strings.Contains(line, "██") || strings.Contains(line, "╚═════╝") {
			logoLineCount++
		}
	}

	if logoLineCount < 6 {
		t.Errorf("block logo should have 6 lines, found %d", logoLineCount)
	}
}

func TestShowLogoFallbackNarrowTerminal(t *testing.T) {
	t.Parallel()
	output := renderToString(t, WithTermWidth(60), WithVersion("0.6.0"))

	// Should NOT contain the full block logo.
	if strings.Contains(output, "██████╗") {
		t.Error("narrow terminal should NOT show full block logo")
	}

	// Should contain the compact header instead.
	if !strings.Contains(output, "CODEFLOW v0.6.0") {
		t.Error("narrow terminal should show compact header 'CODEFLOW v0.6.0'")
	}
}

func TestShowColorOutput(t *testing.T) {
	t.Parallel()
	// Show() disables color for non-terminal fds, so we test the renderer
	// directly to verify color logic works when NoColor=false.
	r := &renderer{
		w: &bytes.Buffer{},
		cfg: &Config{
			CodeflowVersion:   "0.6.0",
			ClaudeCodeVersion: "v1.0.25",
			NoColor:           false,
			TermWidth:         80,
		},
		bc: unicodeBox,
		tw: 80,
	}

	// Verify the color function produces ANSI codes when NoColor is false.
	result := r.color(ansiCyan, "hello")
	if !strings.Contains(result, "\033[36m") {
		t.Error("color output should contain ANSI cyan code when NoColor=false")
	}
	if !strings.Contains(result, "\033[0m") {
		t.Error("color output should contain ANSI reset code when NoColor=false")
	}
}

func TestShowNoColorMode(t *testing.T) {
	t.Parallel()
	output := renderToString(t, WithNoColor(true))

	// Output should NOT contain any ANSI escape codes.
	if strings.Contains(output, "\033[") {
		t.Error("no-color output should not contain ANSI escape codes")
	}
}

func TestShowNoColorEnvVar(t *testing.T) {
	// NOTE: no t.Parallel() -- t.Setenv is not compatible with parallel tests
	t.Setenv("NO_COLOR", "1")

	var buf bytes.Buffer
	Show(&buf,
		WithVersion("0.6.0"),
		WithTermWidth(80),
		WithOutputFd(0),
	)
	output := buf.String()

	if strings.Contains(output, "\033[") {
		t.Error("output with NO_COLOR env var should not contain ANSI codes")
	}
}

func TestShowUpdateNotification(t *testing.T) {
	t.Parallel()
	output := renderToString(t, WithUpdate("v1.1.0"))

	if !strings.Contains(output, "UPDATE AVAILABLE") {
		t.Error("output should show UPDATE AVAILABLE when update is available")
	}
	if !strings.Contains(output, "v1.1.0") {
		t.Error("output should show the new version number")
	}
}

func TestShowNoUpdateNotification(t *testing.T) {
	t.Parallel()
	output := renderToString(t)

	if strings.Contains(output, "UPDATE AVAILABLE") {
		t.Error("output should NOT show UPDATE AVAILABLE when no update")
	}
}

func TestShowQuietMode(t *testing.T) {
	t.Parallel()
	var buf bytes.Buffer
	Show(&buf,
		WithQuiet(true),
		WithVersion("0.6.0"),
		WithTermWidth(80),
		WithOutputFd(0),
	)

	if buf.Len() != 0 {
		t.Errorf("quiet mode should produce no output, got %d bytes: %q", buf.Len(), buf.String())
	}
}

func TestShowASCIIFallback(t *testing.T) {
	t.Parallel()
	output := renderToString(t, WithASCII(true))

	// Should use ASCII box characters for the frame.
	if !strings.Contains(output, "+") {
		t.Error("ASCII mode should use '+' for corners")
	}
	if !strings.Contains(output, "|") {
		t.Error("ASCII mode should use '|' for vertical borders")
	}
	// Should NOT contain Unicode box-drawing frame characters (╔ ╗ ╚ ╝ ╠ ╣).
	// Note: the block logo still uses Unicode block chars (██), which is separate.
	for _, ch := range []string{"╔", "╗", "╚", "╝", "╠", "╣"} {
		// Only check at the start/end of lines where frame chars appear.
		lines := strings.Split(output, "\n")
		for _, line := range lines {
			trimmed := strings.TrimSpace(line)
			if trimmed == "" {
				continue
			}
			if strings.HasPrefix(trimmed, ch) || strings.HasSuffix(trimmed, ch) {
				t.Errorf("ASCII mode box frame should NOT use %q at line boundaries", ch)
			}
		}
	}
}

func TestShowCustomQuickStart(t *testing.T) {
	t.Parallel()
	cmds := []QuickStartCmd{
		{Command: "/custom-cmd", Description: "Custom command"},
		{Command: "/another", Description: "Another one"},
	}
	output := renderToString(t, WithQuickStart(cmds))

	if !strings.Contains(output, "/custom-cmd") {
		t.Error("output should show custom quick start commands")
	}
	if !strings.Contains(output, "/another") {
		t.Error("output should show all custom commands")
	}
}

func TestShowContextAwareQuickStart(t *testing.T) {
	t.Parallel()
	t.Run("with active work", func(t *testing.T) {
		output := renderToString(t,
			WithActiveWork("TSK-001", "Test", "feat/test", time.Now()),
		)
		if !strings.Contains(output, "/cf-resume") {
			t.Error("should show /cf-resume when active work exists")
		}
	})

	t.Run("without active work", func(t *testing.T) {
		output := renderToString(t)
		if strings.Contains(output, "/cf-resume") {
			t.Error("should NOT show /cf-resume when no active work")
		}
		if !strings.Contains(output, "/cf-develop") {
			t.Error("should show /cf-develop when no active work")
		}
	})
}

func TestFormatTimeAgo(t *testing.T) {
	t.Parallel()
	tests := []struct {
		name     string
		duration time.Duration
		want     string
	}{
		{"zero time", 0, "unknown"},
		{"just now", 30 * time.Second, "just now"},
		{"1 minute", 1 * time.Minute, "1 minute ago"},
		{"5 minutes", 5 * time.Minute, "5 minutes ago"},
		{"1 hour", 1 * time.Hour, "1 hour ago"},
		{"3 hours", 3 * time.Hour, "3 hours ago"},
		{"1 day", 25 * time.Hour, "1 day ago"},
		{"5 days", 5 * 24 * time.Hour, "5 days ago"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var input time.Time
			if tt.name != "zero time" {
				input = time.Now().Add(-tt.duration)
			}
			got := formatTimeAgo(input)
			if got != tt.want {
				t.Errorf("formatTimeAgo() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestVisibleLength(t *testing.T) {
	t.Parallel()
	tests := []struct {
		name  string
		input string
		want  int
	}{
		{"plain text", "hello", 5},
		{"with ANSI", "\033[36mhello\033[0m", 5},
		{"multiple ANSI", "\033[1;36mhi\033[0m \033[32mworld\033[0m", 8},
		{"empty", "", 0},
		{"only ANSI", "\033[36m\033[0m", 0},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := visibleLength(tt.input)
			if got != tt.want {
				t.Errorf("visibleLength(%q) = %d, want %d", tt.input, got, tt.want)
			}
		})
	}
}

func TestIsUTF8Terminal(t *testing.T) {
	// NOTE: no t.Parallel() -- subtests use t.Setenv which is not compatible with parallel tests
	t.Run("UTF-8 LANG", func(t *testing.T) {
		t.Setenv("LANG", "en_US.UTF-8")
		t.Setenv("LC_ALL", "")
		if !isUTF8Terminal() {
			t.Error("should detect UTF-8 from LANG")
		}
	})

	t.Run("UTF-8 LC_ALL", func(t *testing.T) {
		t.Setenv("LANG", "")
		t.Setenv("LC_ALL", "en_US.utf8")
		if !isUTF8Terminal() {
			t.Error("should detect UTF-8 from LC_ALL")
		}
	})

	t.Run("no locale vars defaults to true", func(t *testing.T) {
		t.Setenv("LANG", "")
		t.Setenv("LC_ALL", "")
		if !isUTF8Terminal() {
			t.Error("should default to true when no locale vars set")
		}
	})
}

func TestShowProjectStatus(t *testing.T) {
	t.Parallel()
	tests := []struct {
		status string
		word   string
	}{
		{"Active", "Active"},
		{"Warning", "Warning"},
		{"Error", "Error"},
	}

	for _, tt := range tests {
		t.Run(tt.status, func(t *testing.T) {
			output := renderToString(t, WithProject("myproject", tt.status))
			if !strings.Contains(output, tt.word) {
				t.Errorf("output should contain status %q", tt.word)
			}
			if !strings.Contains(output, "myproject") {
				t.Error("output should contain project name")
			}
		})
	}
}

func TestShowStartedTimestamp(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithActiveWork("TSK-001", "Test task", "feat/test",
			time.Now().Add(-2*time.Hour)),
	)
	if !strings.Contains(output, "Started:") {
		t.Error("output should contain 'Started:' timestamp")
	}
	if !strings.Contains(output, "hours ago") {
		t.Error("output should show 'hours ago' for 2-hour-old work")
	}
}

func TestShowEmojiInActiveWork(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithActiveWork("TSK-001", "Test", "feat/test", time.Now()),
	)
	// The hammer emoji U+1F528.
	if !strings.Contains(output, "\U0001F528") {
		t.Error("active work section should contain hammer emoji")
	}
}

func TestDefaultQuickStart(t *testing.T) {
	t.Parallel()
	t.Run("with active work", func(t *testing.T) {
		cmds := defaultQuickStart(&ActiveWork{TaskID: "TSK-001"})
		if len(cmds) < 3 {
			t.Fatalf("expected at least 3 commands, got %d", len(cmds))
		}
		if cmds[0].Command != "/cf-resume" {
			t.Errorf("first command should be /cf-resume, got %q", cmds[0].Command)
		}
	})

	t.Run("without active work", func(t *testing.T) {
		cmds := defaultQuickStart(nil)
		if len(cmds) < 3 {
			t.Fatalf("expected at least 3 commands, got %d", len(cmds))
		}
		if cmds[0].Command != `/cf-plan "feature"` {
			t.Errorf("first command should be /cf-plan, got %q", cmds[0].Command)
		}
	})
}

func TestShowNonInteractiveDisablesColor(t *testing.T) {
	t.Parallel()
	// When OutputFd points to a non-terminal, color should be disabled
	// regardless of WithNoColor setting.
	var buf bytes.Buffer

	// Use a pipe fd (not a terminal).
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatalf("creating pipe: %v", err)
	}
	defer r.Close()
	defer w.Close()

	Show(&buf,
		WithVersion("0.6.0"),
		WithTermWidth(80),
		WithOutputFd(w.Fd()), // pipe fd, not a terminal
	)
	output := buf.String()

	if strings.Contains(output, "\033[") {
		t.Error("non-interactive output (pipe) should not contain ANSI codes")
	}
}

func TestShowLineCount(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithVersion("0.6.0"),
		WithActiveWork("TSK-001", "Test", "feat/test", time.Now()),
	)
	lines := strings.Split(strings.TrimRight(output, "\n"), "\n")

	// The welcome screen should have a reasonable number of lines.
	// With all sections: top border + empty + 6 logo + empty + tagline +
	// claude version + empty + separator + empty + project (2) + empty +
	// active work (5) + separator + empty + quick start (4) + empty +
	// prompt + bottom border = ~28 lines minimum.
	if len(lines) < 20 {
		t.Errorf("expected at least 20 lines, got %d", len(lines))
	}
}

func TestRendererColorDisabled(t *testing.T) {
	t.Parallel()
	r := &renderer{
		cfg: &Config{NoColor: true},
		bc:  unicodeBox,
		tw:  80,
	}

	got := r.color(ansiCyan, "test")
	if got != "test" {
		t.Errorf("color with NoColor=true should return plain text, got %q", got)
	}
}

func TestRendererColorEnabled(t *testing.T) {
	t.Parallel()
	r := &renderer{
		cfg: &Config{NoColor: false},
		bc:  unicodeBox,
		tw:  80,
	}

	got := r.color(ansiCyan, "test")
	want := ansiCyan + "test" + ansiReset
	if got != want {
		t.Errorf("color with NoColor=false: got %q, want %q", got, want)
	}
}

func TestShowAllBoxCharsASCII(t *testing.T) {
	t.Parallel()
	output := renderToString(t, WithASCII(true))

	// Verify all ASCII box characters are present.
	for _, ch := range []string{"+", "=", "|"} {
		if !strings.Contains(output, ch) {
			t.Errorf("ASCII output should contain %q", ch)
		}
	}
}

func TestShowWithPathFlow(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithVersion("0.6.0"),
		WithPathFlow("PF4-EXECUTE", "WS-DEV", "0"),
	)

	if !strings.Contains(output, "PATHFLOW:") {
		t.Error("output should contain PATHFLOW section label")
	}
	if !strings.Contains(output, "Phase:") {
		t.Error("output should contain Phase label")
	}
	if !strings.Contains(output, "PF4-EXECUTE") {
		t.Error("output should contain phase value")
	}
	if !strings.Contains(output, "Stage:") {
		t.Error("output should contain Stage label")
	}
	if !strings.Contains(output, "WS-DEV") {
		t.Error("output should contain stage value")
	}
	if !strings.Contains(output, "Rework:") {
		t.Error("output should contain Rework label")
	}
	if !strings.Contains(output, "0") {
		t.Error("output should contain rework count value")
	}
}

func TestShowWithTeam(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithVersion("0.6.0"),
		WithTeam("inf-tsk-015-014", []string{"cf-security", "cf-knowledge-layer", "cf-git-operations"}),
	)

	if !strings.Contains(output, "TEAM:") {
		t.Error("output should contain TEAM section label")
	}
	if !strings.Contains(output, "inf-tsk-015-014") {
		t.Error("output should contain team name")
	}
	if !strings.Contains(output, "cf-security") {
		t.Error("output should contain teammate cf-security")
	}
	if !strings.Contains(output, "cf-knowledge-layer") {
		t.Error("output should contain teammate cf-knowledge-layer")
	}
	if !strings.Contains(output, "cf-git-operations") {
		t.Error("output should contain teammate cf-git-operations")
	}
}

func TestShowWithPathFlowAndTeam(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithVersion("0.6.0"),
		WithActiveWork("INF-TSK-015-014", "Welcome V4", "feat/welcome-v4", time.Now()),
		WithPathFlow("PF4-EXECUTE", "WS-DEV", "1"),
		WithTeam("inf-tsk-015-014", []string{"cf-development", "cf-review"}),
	)

	// Both sections should be present.
	if !strings.Contains(output, "PATHFLOW:") {
		t.Error("output should contain PATHFLOW section when both options set")
	}
	if !strings.Contains(output, "TEAM:") {
		t.Error("output should contain TEAM section when both options set")
	}
	if !strings.Contains(output, "ACTIVE WORK:") {
		t.Error("output should still contain ACTIVE WORK section")
	}

	// Verify rework count.
	if !strings.Contains(output, "Rework:") {
		t.Error("output should contain Rework label")
	}

	// Verify ordering: PATHFLOW appears after ACTIVE WORK, TEAM after PATHFLOW.
	pathflowIdx := strings.Index(output, "PATHFLOW:")
	teamIdx := strings.Index(output, "TEAM:")
	activeWorkIdx := strings.Index(output, "ACTIVE WORK:")
	if activeWorkIdx >= pathflowIdx {
		t.Error("PATHFLOW section should appear after ACTIVE WORK section")
	}
	if pathflowIdx >= teamIdx {
		t.Error("TEAM section should appear after PATHFLOW section")
	}
}

func TestShowWithoutPathFlowOrTeam(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithVersion("0.6.0"),
		WithActiveWork("TSK-001", "Test task", "feat/test", time.Now()),
	)

	// Standard V3 layout: no PATHFLOW or TEAM sections.
	if strings.Contains(output, "PATHFLOW:") {
		t.Error("output should NOT contain PATHFLOW section when option not set")
	}
	if strings.Contains(output, "TEAM:") {
		t.Error("output should NOT contain TEAM section when option not set")
	}

	// V3 sections should still be present.
	if !strings.Contains(output, "ACTIVE WORK:") {
		t.Error("output should still contain ACTIVE WORK section")
	}
	if !strings.Contains(output, "PROJECT:") {
		t.Error("output should still contain PROJECT section")
	}
	if !strings.Contains(output, "QUICK START:") {
		t.Error("output should still contain QUICK START section")
	}
}

func TestShowWithTeamEmptyTeammates(t *testing.T) {
	t.Parallel()
	output := renderToString(t,
		WithTeam("solo-team", nil),
	)

	if !strings.Contains(output, "TEAM:") {
		t.Error("output should contain TEAM section even with empty teammates")
	}
	if !strings.Contains(output, "solo-team") {
		t.Error("output should contain team name")
	}
}

func TestShowWithPathFlowReworkCount(t *testing.T) {
	t.Parallel()
	tests := []struct {
		name        string
		reworkCount string
		wantInOutput string
	}{
		{"zero rework", "0", "Rework: 0"},
		{"one rework", "1", "Rework: 1"},
		{"high rework", "3", "Rework: 3"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			output := renderToString(t,
				WithPathFlow("PF4-EXECUTE", "WS-REV", tt.reworkCount),
			)
			if !strings.Contains(output, tt.wantInOutput) {
				t.Errorf("output should contain %q", tt.wantInOutput)
			}
		})
	}
}

func TestShowBranchColorInActiveWork(t *testing.T) {
	t.Parallel()
	// Test that branch name appears when active work is set.
	output := renderToString(t,
		WithActiveWork("TSK-001", "Test", "fix/bug-123", time.Now()),
	)
	if !strings.Contains(output, "fix/bug-123") {
		t.Error("output should contain branch name")
	}
	if !strings.Contains(output, "Branch:") {
		t.Error("output should contain 'Branch:' label")
	}
}
