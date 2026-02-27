package welcome

import (
	"io"
	"testing"
	"time"
)

// BenchmarkRender benchmarks the welcome screen rendering with all options
// (including PathFlow + Team). Uses pre-injected data only — no filesystem I/O.
func BenchmarkRender(b *testing.B) {
	opts := []Option{
		WithVersion("0.6.0"),
		WithClaudeCodeVersion("v1.0.25"),
		WithProject("codeflow", "Active"),
		WithActiveWork("INF-TSK-015-014", "Welcome V4 PathFlow", "feat/welcome-v4",
			time.Now().Add(-2*time.Hour)),
		WithPathFlow("PF4-EXECUTE", "WS-DEV", "1"),
		WithTeam("inf-tsk-015-014", []string{
			"cf-security", "cf-knowledge-layer", "cf-git-operations", "cf-development",
		}),
		WithQuickStart([]QuickStartCmd{
			{Command: "/cf-resume", Description: "Resume active work"},
			{Command: "/cf-help", Description: "Show all commands"},
		}),
		WithUpdate("v0.7.0"),
		WithNoColor(true),
		WithTermWidth(80),
		WithOutputFd(0),
	}

	b.ResetTimer()
	for b.Loop() {
		Show(io.Discard, opts...)
	}
}

// BenchmarkRenderMinimal benchmarks with minimal options (V3 layout, no
// PathFlow/Team) for comparison.
func BenchmarkRenderMinimal(b *testing.B) {
	opts := []Option{
		WithVersion("0.6.0"),
		WithNoColor(true),
		WithTermWidth(80),
		WithOutputFd(0),
	}

	b.ResetTimer()
	for b.Loop() {
		Show(io.Discard, opts...)
	}
}
