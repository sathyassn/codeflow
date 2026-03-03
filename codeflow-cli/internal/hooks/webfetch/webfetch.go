package webfetch

import (
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/url"
	"strings"
)

// Verdict represents the result of a URL validation check.
type Verdict struct {
	// Allow is true if the URL is permitted.
	Allow bool

	// Reason is a human-readable explanation when the URL is blocked.
	Reason string

	// AlwaysBlocked is true when the domain is on the hard-block list
	// (localhost, private IPs, etc.) and should cause exit 2. When false
	// and Allow is false, the domain is untrusted and the caller should
	// ask for user approval rather than hard-blocking.
	AlwaysBlocked bool

	// Domain is the extracted domain from the URL, used by callers to
	// construct permission-decision messages.
	Domain string
}

// URLChecker validates URLs against trusted domain allowlists and
// blocked patterns. It implements the same logic as
// cf-pre-tool-use-webfetch.sh.
type URLChecker struct {
	// TrustedDomains are domains that are always allowed.
	// Subdomains are matched automatically (e.g., "github.com" matches
	// "api.github.com").
	TrustedDomains []string

	// BlockedPatterns are domain patterns that are always blocked
	// (e.g., "localhost", "*.local", "internal.*").
	BlockedPatterns []string

	// PrivateIPRanges are IP prefix patterns that are always blocked
	// (e.g., "10.*", "192.168.*", "172.16.*").
	PrivateIPRanges []string
}

// hookInput represents the JSON structure sent by Claude Code on stdin.
type hookInput struct {
	ToolName  string          `json:"tool_name"`
	ToolInput json.RawMessage `json:"tool_input"`
}

// webFetchInput represents the tool_input for WebFetch/WebSearch tool calls.
type webFetchInput struct {
	URL string `json:"url"`
}

// bashInput represents the tool_input for Bash tool calls.
type bashInput struct {
	Command string `json:"command"`
}

// Check reads Claude Code hook JSON from stdin, extracts URLs, and validates
// them against the trusted/blocked domain configuration.
//
// For WebFetch/WebSearch tools, the URL is extracted from tool_input.url.
// For Bash tools, URLs are extracted from curl/wget commands.
// For non-network tools, the check returns Allow.
func (c *URLChecker) Check(stdin io.Reader) (*Verdict, error) {
	data, err := io.ReadAll(stdin)
	if err != nil {
		return &Verdict{Allow: true}, fmt.Errorf("read stdin: %w", err)
	}

	if len(data) == 0 {
		return &Verdict{Allow: true}, nil
	}

	var input hookInput
	if err := json.Unmarshal(data, &input); err != nil {
		return &Verdict{Allow: true}, fmt.Errorf("parse hook input: %w", err)
	}

	// Only check WebFetch, WebSearch, and Bash tools.
	switch input.ToolName {
	case "WebFetch", "WebSearch":
		return c.checkWebFetchURL(input.ToolInput)
	case "Bash":
		return c.checkBashURLs(input.ToolInput)
	default:
		return &Verdict{Allow: true}, nil
	}
}

// checkWebFetchURL validates the URL from a WebFetch/WebSearch tool input.
func (c *URLChecker) checkWebFetchURL(toolInput json.RawMessage) (*Verdict, error) {
	if len(toolInput) == 0 {
		return &Verdict{Allow: true}, nil
	}

	var wfi webFetchInput
	if err := json.Unmarshal(toolInput, &wfi); err != nil {
		return &Verdict{Allow: true}, fmt.Errorf("parse webfetch input: %w", err)
	}

	if wfi.URL == "" {
		return &Verdict{Allow: true}, nil
	}

	return c.validateURL(wfi.URL), nil
}

// checkBashURLs extracts and validates URLs from Bash commands that use
// network tools (curl, wget, etc.).
func (c *URLChecker) checkBashURLs(toolInput json.RawMessage) (*Verdict, error) {
	if len(toolInput) == 0 {
		return &Verdict{Allow: true}, nil
	}

	var bi bashInput
	if err := json.Unmarshal(toolInput, &bi); err != nil {
		return &Verdict{Allow: true}, fmt.Errorf("parse bash input: %w", err)
	}

	if bi.Command == "" {
		return &Verdict{Allow: true}, nil
	}

	// Only check commands that use network tools.
	if !containsNetworkTool(bi.Command) {
		return &Verdict{Allow: true}, nil
	}

	// Extract HTTP(S) URLs from the command.
	urls := extractURLs(bi.Command)
	if len(urls) == 0 {
		return &Verdict{Allow: true}, nil
	}

	// Validate each URL — block on first failure.
	for _, u := range urls {
		v := c.validateURL(u)
		if !v.Allow {
			return v, nil
		}
	}

	return &Verdict{Allow: true}, nil
}

// networkTools lists command names that indicate network operations.
var networkTools = []string{
	"curl", "wget", "fetch", "nc", "netcat",
	"ssh", "scp", "rsync", "ftp",
}

// containsNetworkTool checks whether a command string contains any
// network tool invocation.
func containsNetworkTool(command string) bool {
	for _, tool := range networkTools {
		if strings.Contains(command, tool) {
			return true
		}
	}
	return false
}

// extractURLs finds all HTTP(S) URLs in a command string.
func extractURLs(command string) []string {
	var urls []string
	words := strings.Fields(command)
	for _, w := range words {
		// Strip surrounding quotes.
		w = strings.Trim(w, `"'`)
		if strings.HasPrefix(w, "http://") || strings.HasPrefix(w, "https://") {
			urls = append(urls, w)
		}
	}
	return urls
}

// validateURL checks a single URL against blocked patterns and trusted domains.
func (c *URLChecker) validateURL(rawURL string) *Verdict {
	// Block file:// and data: URLs.
	lower := strings.ToLower(rawURL)
	if strings.HasPrefix(lower, "file://") {
		return &Verdict{
			Allow:         false,
			AlwaysBlocked: true,
			Reason:        fmt.Sprintf("BLOCKED: file:// URLs are not allowed\nURL: %s\n", rawURL),
		}
	}
	if strings.HasPrefix(lower, "data:") {
		return &Verdict{
			Allow:         false,
			AlwaysBlocked: true,
			Reason:        fmt.Sprintf("BLOCKED: data: URLs are not allowed\nURL: %s\n", rawURL),
		}
	}

	domain := extractDomain(rawURL)
	if domain == "" {
		return &Verdict{Allow: true}
	}

	// Trusted domains bypass block checks (matching shell behavior).
	if c.isTrustedDomain(domain) {
		return &Verdict{Allow: true, Domain: domain}
	}

	// Check always-blocked patterns.
	if c.isBlockedDomain(domain) {
		return &Verdict{
			Allow:         false,
			AlwaysBlocked: true,
			Domain:        domain,
			Reason:        fmt.Sprintf("BLOCKED: Internal/local network access forbidden\nURL: %s\nDomain: %s\nAccess to localhost, internal networks, and private IPs is not allowed.\n", rawURL, domain),
		}
	}

	// Check private IP ranges.
	if c.isPrivateIP(domain) {
		return &Verdict{
			Allow:         false,
			AlwaysBlocked: true,
			Domain:        domain,
			Reason:        fmt.Sprintf("BLOCKED: Internal/local network access forbidden\nURL: %s\nDomain: %s\nAccess to private IP ranges is not allowed.\n", rawURL, domain),
		}
	}

	// Not trusted and not hard-blocked — untrusted domain, ask for approval.
	return &Verdict{
		Allow:         false,
		AlwaysBlocked: false,
		Domain:        domain,
		Reason:        fmt.Sprintf("BLOCKED: Untrusted domain\nURL: %s\nDomain: %s\nThis domain is not in the trusted allowlist.\n", rawURL, domain),
	}
}

// extractDomain extracts the domain from a URL string, stripping the
// protocol, path, port, and user info.
func extractDomain(rawURL string) string {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return ""
	}

	host := parsed.Hostname()
	if host == "" {
		return ""
	}

	return strings.ToLower(host)
}

// isTrustedDomain checks if a domain matches any trusted domain.
// Subdomain matching: "api.github.com" matches "github.com".
// Case-insensitive.
func (c *URLChecker) isTrustedDomain(domain string) bool {
	d := strings.ToLower(domain)
	for _, trusted := range c.TrustedDomains {
		t := strings.ToLower(trusted)
		if d == t {
			return true
		}
		if strings.HasSuffix(d, "."+t) {
			return true
		}
	}
	return false
}

// isBlockedDomain checks if a domain matches any blocked pattern.
// Supports wildcard patterns: "*.local", "internal.*", exact matches.
func (c *URLChecker) isBlockedDomain(domain string) bool {
	for _, pattern := range c.BlockedPatterns {
		if matchDomainPattern(domain, pattern) {
			return true
		}
	}
	return false
}

// isPrivateIP checks if a domain looks like a private/internal IP address.
func (c *URLChecker) isPrivateIP(domain string) bool {
	// First try matching against configured ranges.
	for _, pattern := range c.PrivateIPRanges {
		if matchIPPattern(domain, pattern) {
			return true
		}
	}

	// Also check via net.ParseIP for RFC 1918 ranges not covered by patterns.
	ip := net.ParseIP(domain)
	if ip == nil {
		return false
	}

	return ip.IsLoopback() || ip.IsPrivate() || ip.IsLinkLocalUnicast() || ip.IsLinkLocalMulticast()
}

// matchDomainPattern matches a domain against a wildcard pattern.
// Supported patterns:
//   - "*.local" — matches any domain ending in ".local"
//   - "internal.*" — matches any domain starting with "internal."
//   - "localhost" — exact match
func matchDomainPattern(domain, pattern string) bool {
	domain = strings.ToLower(domain)
	pattern = strings.ToLower(pattern)

	// Exact match.
	if domain == pattern {
		return true
	}

	// *.suffix pattern (e.g., "*.local").
	if strings.HasPrefix(pattern, "*.") {
		suffix := pattern[1:] // includes the leading dot
		if strings.HasSuffix(domain, suffix) {
			return true
		}
		// Also match the bare suffix without the dot.
		if domain == pattern[2:] {
			return true
		}
	}

	// prefix.* pattern (e.g., "internal.*").
	if strings.HasSuffix(pattern, ".*") {
		prefix := pattern[:len(pattern)-2]
		if strings.HasPrefix(domain, prefix+".") {
			return true
		}
		if domain == prefix {
			return true
		}
	}

	return false
}

// matchIPPattern matches a domain against an IP prefix pattern.
// Patterns like "10.*", "192.168.*" match IP addresses with that prefix.
func matchIPPattern(domain, pattern string) bool {
	if !strings.HasSuffix(pattern, "*") {
		return domain == pattern
	}
	prefix := pattern[:len(pattern)-1] // strip trailing *
	return strings.HasPrefix(domain, prefix)
}
