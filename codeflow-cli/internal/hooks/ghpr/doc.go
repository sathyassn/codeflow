// Package ghpr provides PR merge protection enforcement for CodeFlow hooks.
//
// It checks whether a `gh pr merge` command targets a protected branch
// (main, master, release/*, production) and blocks the operation if so.
// Protected branches are loaded from enforcement-policy.json.
//
// Usage:
//
//	checker := &ghpr.PRChecker{
//	    ProtectedBranches: []string{"main", "master", "release/*", "production"},
//	}
//	verdict, err := checker.Check(os.Stdin)
//	if err != nil {
//	    // handle error
//	}
//	if !verdict.Allow {
//	    fmt.Fprintf(os.Stderr, "%s", verdict.Reason)
//	    os.Exit(2)
//	}
package ghpr
