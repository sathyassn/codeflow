package autorun

import (
	"testing"
)

// TestRealTmuxRunnerImplementsInterface verifies that RealTmuxRunner
// satisfies the TmuxRunner interface at compile time.
func TestRealTmuxRunnerImplementsInterface(t *testing.T) {
	t.Parallel()
	var _ TmuxRunner = (*RealTmuxRunner)(nil)
}

// TestRealClaudeInvokerImplementsInterface verifies that RealClaudeInvoker
// satisfies the ClaudeInvoker interface at compile time.
func TestRealClaudeInvokerImplementsInterface(t *testing.T) {
	t.Parallel()
	var _ ClaudeInvoker = (*RealClaudeInvoker)(nil)
}
