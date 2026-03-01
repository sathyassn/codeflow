// Package webfetch provides URL validation for WebFetch/WebSearch hook enforcement.
//
// It validates URLs against trusted domain allowlists and blocks access to
// internal/private network addresses. The domain lists and block patterns
// are loaded from enforcement-policy.json.
//
// Usage:
//
//	checker := &webfetch.URLChecker{
//	    TrustedDomains:     []string{"github.com", "npmjs.com"},
//	    BlockedPatterns:    []string{"localhost", "127.0.0.1", "*.local"},
//	    PrivateIPRanges:    []string{"10.*", "192.168.*"},
//	}
//	verdict, err := checker.Check(os.Stdin)
//	if err != nil {
//	    // parse error — allow through
//	}
//	if !verdict.Allow {
//	    fmt.Fprintf(os.Stderr, "%s", verdict.Reason)
//	    os.Exit(2)
//	}
package webfetch
