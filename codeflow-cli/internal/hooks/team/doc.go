// Package team provides TeamDelete guard enforcement for CodeFlow hooks.
//
// It blocks TeamDelete operations when a PathFlow session is active,
// preventing accidental team dissolution that would destroy the PathFlow
// task graph. The PF7-END gate allows TeamDelete when the pf-6 sentinel
// exists, indicating PF6-COMPLETE is done.
//
// It also provides PostToolUse logic for removing the pathflow-active flag
// after a successful TeamDelete.
//
// Usage:
//
//	verdict, err := team.CheckTeamDelete(os.Stdin, sessionDir, sentinelDir)
//	if err != nil {
//	    // parse error — allow through
//	}
//	if !verdict.Allow {
//	    fmt.Fprintf(os.Stderr, "%s", verdict.Reason)
//	    os.Exit(2)
//	}
package team
