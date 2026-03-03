package shadowtest

import (
	"strings"
	"testing"
)

// TestAllShadowTests verifies that AllShadowTests returns a non-empty list
// and all tests have required fields.
func TestAllShadowTests(t *testing.T) {
	t.Parallel()

	tests := AllShadowTests("/tmp/project")
	if len(tests) == 0 {
		t.Fatal("AllShadowTests returned empty list")
	}

	for i, st := range tests {
		if st.Name == "" {
			t.Errorf("test[%d]: Name is empty", i)
		}
		if len(st.GoCommand) == 0 {
			t.Errorf("test[%d] %q: GoCommand is empty", i, st.Name)
		}
		if len(st.ShellCommand) == 0 {
			t.Errorf("test[%d] %q: ShellCommand is empty", i, st.Name)
		}
	}
}

// TestAllShadowTests_NoKnownDivergenceForFixedBugs verifies that the 2 bugs
// fixed in Category B (phase-transition/invalid-phase, session-register/invalid-mode)
// no longer carry a KnownDivergence annotation.
// If KnownDivergence is re-added to any of these entries, the fix was reverted.
//
// Note: hook/webfetch-guard/allow-safe-url is intentionally excluded here — it
// carries a KnownDivergence for a runtime environment difference (shell trusted
// domain loading via REPO_ROOT vs Go using detectProjectDir), not a code bug.
func TestAllShadowTests_NoKnownDivergenceForFixedBugs(t *testing.T) {
	t.Parallel()

	fixed := map[string]bool{
		"pathflow/phase-transition/invalid-phase": true,
		"pathflow/session-register/invalid-mode":  true,
	}

	tests := AllShadowTests("/tmp/project")
	for _, st := range tests {
		if fixed[st.Name] && st.KnownDivergence != "" {
			t.Errorf("test %q has KnownDivergence set — bug should be fixed, not documented: %s",
				st.Name, st.KnownDivergence)
		}
	}
}

// TestAllShadowTests_CoverageCount verifies coverage of all required hook and
// pathflow categories.
func TestAllShadowTests_CoverageCount(t *testing.T) {
	t.Parallel()

	tests := AllShadowTests("/tmp/project")

	hookCount := 0
	pathflowCount := 0
	validationCount := 0

	for _, st := range tests {
		switch {
		case strings.HasPrefix(st.Name, "hook/"):
			hookCount++
		case strings.HasPrefix(st.Name, "pathflow/"):
			pathflowCount++
		case strings.HasPrefix(st.Name, "validate/"):
			validationCount++
		}
	}

	if hookCount < 12 {
		t.Errorf("expected >= 12 hook shadow tests (all 12 critical hooks), got %d", hookCount)
	}
	if pathflowCount < 5 {
		t.Errorf("expected >= 5 pathflow shadow tests (all 5 T1 scripts), got %d", pathflowCount)
	}
	if validationCount < 2 {
		t.Errorf("expected >= 2 validation shadow tests (task + epic), got %d", validationCount)
	}
}

