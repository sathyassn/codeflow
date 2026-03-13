package security

import (
	"testing"
)

func TestPathModule(t *testing.T) {
	t.Parallel()

	mod := &PathModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"rm settings", "rm .claude/settings.json"},
		{"mv settings", "mv .claude/settings.json /tmp/backup"},
		{"rm hooks", "rm -rf .claude/hooks/codeflow/pre-tool-use/script.sh"},
		{"chmod settings", "chmod 777 .claude/settings.json"},
		{"git rm config", "git rm .codeflow/config/enforcement/policy.json"},
		{"rm .claude dir", "rm -rf .claude"},
		{"rm .codeflow dir", "rm -rf .codeflow"},
		{"redirect to settings", "echo x > .claude/settings.json"},
		{"append to settings", "echo x >> .claude/settings.json"},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
			v := mod.Check(ctx)
			if v == nil || v.Allow {
				t.Errorf("expected block for %q, got allow", tt.cmd)
			}
		})
	}

	allowed := []struct {
		name string
		cmd  string
	}{
		{"read settings", "cat .claude/settings.json"},
		{"rm unprotected", "rm some-file.txt"},
		{"bash hook", "bash .claude/hooks/codeflow/pre-tool-use/script.sh"},
		{"ls config", "ls .codeflow/config/"},
		{"cp to tmp", "cp .claude/settings.json /tmp/claude/backup.json"},
	}

	for _, tt := range allowed {
		t.Run("allow_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
			v := mod.Check(ctx)
			if v != nil && !v.Allow {
				t.Errorf("expected allow for %q, got block: %s", tt.cmd, v.Reason)
			}
		})
	}
}

// TestPathModuleVariableIndirection exercises the checkVariableIndirection
// function which detects shell variable indirection targeting protected paths.
func TestPathModuleVariableIndirection(t *testing.T) {
	t.Parallel()

	mod := &PathModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"rm via dollar var", `F=".claude/settings.json" && rm $F`},
		{"rm via quoted dollar var", `F=".claude/settings.json"; rm "$F"`},
		{"rm -rf via brace var", `DIR=".claude" && rm -rf ${DIR}`},
		{"eval with protected path", `eval "rm .claude/settings.json"`},
		{"glob path via var", `P=".claude/hooks/codeflow/**" && rm $P`},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
			v := mod.Check(ctx)
			if v == nil || v.Allow {
				t.Errorf("expected block for %q, got allow", tt.cmd)
			}
		})
	}

	allowed := []struct {
		name string
		cmd  string
	}{
		{"safe path via var", `X="/tmp/safe" && rm $X`},
		{"echo PATH no danger", "echo $PATH"},
		{"cat via var not dangerous", `F=".claude/settings.json" && cat $F`},
	}

	for _, tt := range allowed {
		t.Run("allow_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
			v := mod.Check(ctx)
			if v != nil && !v.Allow {
				t.Errorf("expected allow for %q, got block: %s", tt.cmd, v.Reason)
			}
		})
	}
}

// TestCheckEvalBypass exercises the checkEvalBypass function directly,
// covering all branches including the inner path-matching loop.
func TestCheckEvalBypass(t *testing.T) {
	t.Parallel()

	paths := DefaultPolicy().AllProtectedPaths()

	tests := []struct {
		name    string
		segs    []string
		blocked bool
	}{
		{"eval with protected path", []string{`eval "something .claude/settings.json"`}, true},
		{"eval with glob protected path", []string{`eval ".claude/hooks/codeflow/test.sh"`}, true},
		{"eval with safe path", []string{`eval "ls /tmp/safe"`}, false},
		{"no eval keyword", []string{`rm .claude/settings.json`}, false},
		{"eval in second segment", []string{`echo hello`, `eval ".claude/settings.json"`}, true},
		{"empty segments", []string{}, false},
		{"eval with no protected arg", []string{`eval "ls -la"`}, false},
		{"gh pr with eval in body", []string{`gh pr create --body "checkEvalBypass detects eval"`}, false},
		{"echo with eval mention", []string{`echo "the eval command is dangerous"`}, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			v := checkEvalBypass(tt.segs, paths)
			if tt.blocked && (v == nil || v.Allow) {
				t.Errorf("expected block, got allow")
			}
			if !tt.blocked && v != nil && !v.Allow {
				t.Errorf("expected allow, got block: %s", v.Reason)
			}
		})
	}
}

// TestPathModuleEvalBypassViaCheck exercises the checkEvalBypass return path
// through Check(). The eval command must not contain a literal dangerous
// command keyword visible to checkSegmentDangerousOp, so that only
// checkEvalBypass catches it.
func TestPathModuleEvalBypassViaCheck(t *testing.T) {
	t.Parallel()

	mod := &PathModule{}
	policy := DefaultPolicy()

	// "eval" with protected path but no dangerous cmd keyword (rm/mv/etc)
	// visible to the otherDangerous regex. This bypasses checkSegmentDangerousOp
	// and must be caught by checkEvalBypass.
	blocked := []struct {
		name string
		cmd  string
	}{
		{"eval cat redirect", `eval "cat .codeflow/config/enforcement/policy.json"`},
		{"eval with config path", `eval "test .claude/settings.json"`},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
			v := mod.Check(ctx)
			if v == nil || v.Allow {
				t.Errorf("expected block for %q via checkEvalBypass, got allow", tt.cmd)
			}
		})
	}
}

// TestPathModuleDirectoryProtection exercises the pre-compiled
// protectedDirPatterns for .claude and .codeflow directory-level detection.
func TestPathModuleDirectoryProtection(t *testing.T) {
	t.Parallel()

	mod := &PathModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"rm .claude with quotes", `rm ".claude"`},
		{"rm .codeflow trailing slash", "rm -rf .codeflow/"},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
			v := mod.Check(ctx)
			if v == nil || v.Allow {
				t.Errorf("expected block for %q, got allow", tt.cmd)
			}
		})
	}
}
