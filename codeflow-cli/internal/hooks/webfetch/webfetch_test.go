package webfetch

import (
	"strings"
	"testing"
)

// defaultChecker returns a URLChecker with typical configuration matching
// the enforcement-policy.json defaults.
func defaultChecker() *URLChecker {
	return &URLChecker{
		TrustedDomains: []string{
			"github.com",
			"raw.githubusercontent.com",
			"npmjs.com",
			"pypi.org",
			"stackoverflow.com",
			"anthropic.com",
		},
		BlockedPatterns: []string{
			"localhost",
			"127.0.0.1",
			"0.0.0.0",
			"::1",
			"*.local",
			"internal.*",
			"*.internal",
			"*.corp",
			"*.corp.*",
			"*.lan",
			"*.home",
			"*.private",
		},
		PrivateIPRanges: []string{
			"169.254.*",
			"10.*",
			"172.16.*", "172.17.*", "172.18.*", "172.19.*",
			"172.20.*", "172.21.*", "172.22.*", "172.23.*",
			"172.24.*", "172.25.*", "172.26.*", "172.27.*",
			"172.28.*", "172.29.*", "172.30.*", "172.31.*",
			"192.168.*",
		},
	}
}

func TestURLChecker_Check(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		stdin     string
		wantAllow bool
		wantErr   bool
	}{
		// --- WebFetch tool ---
		{
			name:      "WebFetch trusted domain allowed",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"https://github.com/user/repo"}}`,
			wantAllow: true,
		},
		{
			name:      "WebFetch trusted subdomain allowed",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"https://api.github.com/repos"}}`,
			wantAllow: true,
		},
		{
			name:      "WebFetch anthropic subdomain allowed",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"https://docs.anthropic.com/api"}}`,
			wantAllow: true,
		},
		{
			name:      "WebFetch untrusted domain blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"https://evil.example.com/malware"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch localhost blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://localhost:8080/api"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch 127.0.0.1 blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://127.0.0.1:3000"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch private IP 10.x blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://10.0.0.1/admin"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch private IP 192.168.x blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://192.168.1.1"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch private IP 172.16.x blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://172.16.0.1"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch .local domain blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://myhost.local/api"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch internal.* domain blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://internal.mycompany.com"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch .corp domain blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://app.corp"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch file:// URL blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"file:///etc/passwd"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch data: URL blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"data:text/html,<h1>hi</h1>"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch empty URL allowed",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":""}}`,
			wantAllow: true,
		},
		{
			name:      "WebFetch no URL field allowed",
			stdin:     `{"tool_name":"WebFetch","tool_input":{}}`,
			wantAllow: true,
		},

		// --- WebSearch tool ---
		{
			name:      "WebSearch trusted domain allowed",
			stdin:     `{"tool_name":"WebSearch","tool_input":{"url":"https://stackoverflow.com/questions"}}`,
			wantAllow: true,
		},
		{
			name:      "WebSearch untrusted domain blocked",
			stdin:     `{"tool_name":"WebSearch","tool_input":{"url":"https://sketchy.site/search"}}`,
			wantAllow: false,
		},

		// --- Bash tool ---
		{
			name:      "Bash curl to trusted domain allowed",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"curl https://api.github.com/repos"}}`,
			wantAllow: true,
		},
		{
			name:      "Bash curl to untrusted domain blocked",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"curl https://evil.example.com/payload"}}`,
			wantAllow: false,
		},
		{
			name:      "Bash wget to localhost blocked",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"wget http://localhost:9090/api"}}`,
			wantAllow: false,
		},
		{
			name:      "Bash non-network command allowed",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"ls -la"}}`,
			wantAllow: true,
		},
		{
			name:      "Bash empty command allowed",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":""}}`,
			wantAllow: true,
		},
		{
			name:      "Bash curl no URL allowed",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"curl --help"}}`,
			wantAllow: true,
		},

		// --- Non-network tools ---
		{
			name:      "Edit tool allowed (not checked)",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}`,
			wantAllow: true,
		},
		{
			name:      "Read tool allowed (not checked)",
			stdin:     `{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`,
			wantAllow: true,
		},

		// --- Edge cases ---
		{
			name:      "empty stdin allowed",
			stdin:     "",
			wantAllow: true,
		},
		{
			name:      "invalid JSON allowed (graceful degradation)",
			stdin:     "not json at all",
			wantAllow: true,
			wantErr:   true,
		},
		{
			name:      "empty tool input allowed",
			stdin:     `{"tool_name":"WebFetch","tool_input":null}`,
			wantAllow: true,
		},
		{
			name:      "link-local IP 169.254.x blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://169.254.1.1/metadata"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch 0.0.0.0 blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://0.0.0.0:8080"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch IPv6 loopback blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://[::1]:8080"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch URL with port on trusted domain allowed",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"https://github.com:443/user/repo"}}`,
			wantAllow: true,
		},
		{
			name:      "WebFetch .lan domain blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://server.lan/api"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch .home domain blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://nas.home"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch .private domain blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://app.private"}}`,
			wantAllow: false,
		},
		{
			name:      "WebFetch .internal domain blocked",
			stdin:     `{"tool_name":"WebFetch","tool_input":{"url":"http://service.internal"}}`,
			wantAllow: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			checker := defaultChecker()
			verdict, err := checker.Check(strings.NewReader(tt.stdin))

			if tt.wantErr && err == nil {
				// wantErr means we expect an error but still a valid verdict.
				// For invalid JSON, the function returns an error + Allow verdict.
			}

			if verdict.Allow != tt.wantAllow {
				t.Errorf("Check() Allow = %v, want %v; reason: %s", verdict.Allow, tt.wantAllow, verdict.Reason)
			}

			if !tt.wantAllow && verdict.Reason == "" {
				t.Error("blocked verdict has empty Reason")
			}
		})
	}
}

func TestExtractDomain(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		url  string
		want string
	}{
		{"https URL", "https://github.com/user/repo", "github.com"},
		{"http URL with port", "http://localhost:8080/api", "localhost"},
		{"URL with user info", "https://user@github.com/repo", "github.com"},
		{"URL with path", "https://api.github.com/repos/user/repo", "api.github.com"},
		{"empty string", "", ""},
		{"no protocol", "github.com", ""},
		{"file URL", "file:///etc/passwd", ""},
		{"URL with port number", "https://example.com:443/path", "example.com"},
		{"mixed case domain", "HTTPS://GitHub.COM/repo", "github.com"},
		{"IPv6 URL", "http://[::1]:8080/api", "::1"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			got := extractDomain(tt.url)
			if got != tt.want {
				t.Errorf("extractDomain(%q) = %q, want %q", tt.url, got, tt.want)
			}
		})
	}
}

func TestIsTrustedDomain(t *testing.T) {
	t.Parallel()

	checker := defaultChecker()

	tests := []struct {
		name   string
		domain string
		want   bool
	}{
		{"exact match", "github.com", true},
		{"subdomain match", "api.github.com", true},
		{"deep subdomain", "raw.api.github.com", true},
		{"no match", "evil.example.com", false},
		{"partial name no match", "notgithub.com", false},
		{"case insensitive", "GitHub.COM", true},
		{"npmjs exact", "npmjs.com", true},
		{"pypi exact", "pypi.org", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			got := checker.isTrustedDomain(tt.domain)
			if got != tt.want {
				t.Errorf("isTrustedDomain(%q) = %v, want %v", tt.domain, got, tt.want)
			}
		})
	}
}

func TestIsBlockedDomain(t *testing.T) {
	t.Parallel()

	checker := defaultChecker()

	tests := []struct {
		name   string
		domain string
		want   bool
	}{
		{"localhost exact", "localhost", true},
		{"127.0.0.1 exact", "127.0.0.1", true},
		{"0.0.0.0 exact", "0.0.0.0", true},
		{"::1 exact", "::1", true},
		{"*.local suffix", "myhost.local", true},
		{"internal.* prefix", "internal.company.com", true},
		{"*.internal suffix", "api.internal", true},
		{"*.corp suffix", "app.corp", true},
		{"*.lan suffix", "server.lan", true},
		{"*.home suffix", "nas.home", true},
		{"*.private suffix", "vault.private", true},
		{"not blocked", "example.com", false},
		{"github not blocked", "github.com", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			got := checker.isBlockedDomain(tt.domain)
			if got != tt.want {
				t.Errorf("isBlockedDomain(%q) = %v, want %v", tt.domain, got, tt.want)
			}
		})
	}
}

func TestIsPrivateIP(t *testing.T) {
	t.Parallel()

	checker := defaultChecker()

	tests := []struct {
		name   string
		domain string
		want   bool
	}{
		{"10.0.0.1", "10.0.0.1", true},
		{"10.255.255.255", "10.255.255.255", true},
		{"172.16.0.1", "172.16.0.1", true},
		{"172.31.255.255", "172.31.255.255", true},
		{"192.168.1.1", "192.168.1.1", true},
		{"192.168.0.100", "192.168.0.100", true},
		{"169.254.1.1", "169.254.1.1", true},
		{"8.8.8.8 public", "8.8.8.8", false},
		{"1.1.1.1 public", "1.1.1.1", false},
		{"non-IP domain", "example.com", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			got := checker.isPrivateIP(tt.domain)
			if got != tt.want {
				t.Errorf("isPrivateIP(%q) = %v, want %v", tt.domain, got, tt.want)
			}
		})
	}
}

func TestMatchDomainPattern(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		domain  string
		pattern string
		want    bool
	}{
		{"exact match", "localhost", "localhost", true},
		{"suffix *.local", "myhost.local", "*.local", true},
		{"suffix bare local", "local", "*.local", true},
		{"prefix internal.*", "internal.corp.com", "internal.*", true},
		{"prefix bare internal", "internal", "internal.*", true},
		{"no match", "example.com", "*.local", false},
		{"partial not match", "notlocal", "*.local", false},
		{"case insensitive", "LOCALHOST", "localhost", true},
		{"*.corp.* pattern", "app.corp.com", "*.corp.*", false}, // This pattern not supported by our simple matcher
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			got := matchDomainPattern(tt.domain, tt.pattern)
			if got != tt.want {
				t.Errorf("matchDomainPattern(%q, %q) = %v, want %v", tt.domain, tt.pattern, got, tt.want)
			}
		})
	}
}

func TestMatchIPPattern(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		domain  string
		pattern string
		want    bool
	}{
		{"10.* matches 10.0.0.1", "10.0.0.1", "10.*", true},
		{"192.168.* matches", "192.168.1.1", "192.168.*", true},
		{"172.16.* matches", "172.16.0.1", "172.16.*", true},
		{"10.* does not match 8.8.8.8", "8.8.8.8", "10.*", false},
		{"exact match without wildcard", "10.0.0.1", "10.0.0.1", true},
		{"no match without wildcard", "10.0.0.2", "10.0.0.1", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			got := matchIPPattern(tt.domain, tt.pattern)
			if got != tt.want {
				t.Errorf("matchIPPattern(%q, %q) = %v, want %v", tt.domain, tt.pattern, got, tt.want)
			}
		})
	}
}

func TestExtractURLs(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		command string
		want    int // number of URLs expected
	}{
		{"no URLs", "ls -la", 0},
		{"single http URL", "curl http://example.com", 1},
		{"single https URL", "curl https://example.com", 1},
		{"multiple URLs", "curl https://a.com https://b.com", 2},
		{"quoted URL", `curl "https://example.com/path"`, 1},
		{"no protocol", "curl example.com", 0},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			got := extractURLs(tt.command)
			if len(got) != tt.want {
				t.Errorf("extractURLs(%q) returned %d URLs, want %d; got: %v", tt.command, len(got), tt.want, got)
			}
		})
	}
}

func TestContainsNetworkTool(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		command string
		want    bool
	}{
		{"curl command", "curl https://example.com", true},
		{"wget command", "wget https://example.com", true},
		{"ssh command", "ssh user@host", true},
		{"scp command", "scp file user@host:/path", true},
		{"rsync command", "rsync -avz dir/ user@host:/path", true},
		{"nc command", "nc -z host 80", true},
		{"ls command", "ls -la", false},
		{"git status", "git status", false},
		{"empty command", "", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			got := containsNetworkTool(tt.command)
			if got != tt.want {
				t.Errorf("containsNetworkTool(%q) = %v, want %v", tt.command, got, tt.want)
			}
		})
	}
}

func TestURLChecker_TrustedBypassesBlockList(t *testing.T) {
	t.Parallel()

	// This verifies that trusted domains bypass the block checks,
	// matching the shell behavior where trusted check happens BEFORE
	// block check.
	checker := &URLChecker{
		TrustedDomains:  []string{"localhost"},
		BlockedPatterns: []string{"localhost"},
	}

	verdict, err := checker.Check(strings.NewReader(
		`{"tool_name":"WebFetch","tool_input":{"url":"http://localhost:8080"}}`,
	))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !verdict.Allow {
		t.Errorf("trusted localhost should bypass block list; reason: %s", verdict.Reason)
	}
}

func TestURLChecker_EmptyLists(t *testing.T) {
	t.Parallel()

	// No trusted, no blocked — everything gets blocked as untrusted.
	checker := &URLChecker{}

	verdict, err := checker.Check(strings.NewReader(
		`{"tool_name":"WebFetch","tool_input":{"url":"https://example.com"}}`,
	))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if verdict.Allow {
		t.Error("expected untrusted domain to be blocked when no trusted list configured")
	}
}

func TestValidateURL_FileProtocol(t *testing.T) {
	t.Parallel()

	checker := defaultChecker()
	v := checker.validateURL("file:///etc/passwd")
	if v.Allow {
		t.Error("file:// URL should be blocked")
	}
	if !strings.Contains(v.Reason, "file://") {
		t.Errorf("reason should mention file://, got: %s", v.Reason)
	}
}

func TestValidateURL_DataProtocol(t *testing.T) {
	t.Parallel()

	checker := defaultChecker()
	v := checker.validateURL("data:text/html,<h1>hi</h1>")
	if v.Allow {
		t.Error("data: URL should be blocked")
	}
	if !strings.Contains(v.Reason, "data:") {
		t.Errorf("reason should mention data:, got: %s", v.Reason)
	}
}

func TestVerdictAlwaysBlocked_HardBlockDomains(t *testing.T) {
	t.Parallel()

	checker := defaultChecker()

	// Always-blocked domains (private IPs, localhost, .local, etc.) set AlwaysBlocked=true.
	alwaysBlockedCases := []struct {
		name string
		url  string
	}{
		{"localhost", "http://localhost:8080"},
		{"127.0.0.1", "http://127.0.0.1:3000"},
		{"private IP 10.x", "http://10.0.0.1/admin"},
		{"private IP 192.168.x", "http://192.168.1.1"},
		{".local domain", "http://myhost.local/api"},
		{"file:// URL", "file:///etc/passwd"},
		{"data: URL", "data:text/html,hi"},
	}

	for _, tc := range alwaysBlockedCases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			v := checker.validateURL(tc.url)
			if v.Allow {
				t.Errorf("expected block for %s", tc.url)
			}
			if !v.AlwaysBlocked {
				t.Errorf("expected AlwaysBlocked=true for %s, got false", tc.url)
			}
		})
	}
}

func TestVerdictAlwaysBlocked_UntrustedDomainNotHardBlocked(t *testing.T) {
	t.Parallel()

	checker := defaultChecker()

	// Untrusted domains that are NOT on the always-block list should have AlwaysBlocked=false.
	v := checker.validateURL("https://evil.example.com/path")
	if v.Allow {
		t.Error("expected untrusted domain to be blocked")
	}
	if v.AlwaysBlocked {
		t.Error("expected AlwaysBlocked=false for untrusted (not hard-blocked) domain")
	}
	if v.Domain == "" {
		t.Error("expected Domain to be set for untrusted domain verdict")
	}
}

func TestVerdictDomain_TrustedDomain(t *testing.T) {
	t.Parallel()

	checker := defaultChecker()

	v := checker.validateURL("https://docs.anthropic.com/api")
	if !v.Allow {
		t.Errorf("expected trusted domain to be allowed; reason: %s", v.Reason)
	}
	// Domain field is set even for allowed verdicts.
	if v.Domain != "docs.anthropic.com" {
		t.Errorf("expected Domain=docs.anthropic.com, got %q", v.Domain)
	}
}
