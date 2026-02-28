// Package gate provides PathFlow phase gate enforcement for CodeFlow hooks.
//
// It checks sentinel files to enforce PathFlow phase ordering:
//   - Edit/Write tools require pf-3 sentinel (PF3-CLASSIFY complete)
//   - Bash git commit requires pf-3 sentinel
//   - Bash git push / gh pr require BOTH pf-5 AND ws-rev sentinels (dual gate)
//   - Task tool spawning role teammates requires pf-3 sentinel
//
// When no PathFlow session is active, all operations are allowed.
//
// Usage:
//
//	checker := &gate.GateChecker{
//	    SentinelDir: "/path/to/.state/sentinels/pathflow/session-id/",
//	    SessionID:   "ses-123",
//	}
//	verdict := checker.Check("Edit", json.RawMessage(`{"file_path":"/tmp/x"}`))
//	if !verdict.Allow {
//	    fmt.Fprintf(os.Stderr, "%s", verdict.Reason)
//	    os.Exit(2)
//	}
package gate
