package security

import (
	"regexp"
	"strings"
)

// PrivilegeModule detects and blocks privilege escalation attempts.
// Go equivalent of cf-privilege-protection.sh.
type PrivilegeModule struct{}

// Name returns the module name.
func (m *PrivilegeModule) Name() string { return "privilege-protection" }

// privEscCmds are commands that escalate privileges.
var privEscCmds = []string{"sudo", "su", "doas", "pkexec", "runuser"}

// privPipePatterns are pre-compiled regexps for pipe-to-privilege-command detection.
var privPipePatterns []*regexp.Regexp

func init() {
	for _, priv := range privEscCmds {
		privPipePatterns = append(privPipePatterns,
			regexp.MustCompile(`\|\s*`+regexp.QuoteMeta(priv)+`\s`))
	}
}

// Script bypass patterns.
var (
	bashC  = regexp.MustCompile(`bash\s+-c\s+["']`)
	shC    = regexp.MustCompile(`sh\s+-c\s+["']`)
	zshC   = regexp.MustCompile(`zsh\s+-c\s+["']`)
	evalRe = regexp.MustCompile(`eval\s+.*(rm|sudo|su|doas|pkexec|runuser)`)

	// source command at start
	sourceCmd = regexp.MustCompile(`^source\s`)
	// dot source: ". " at start (dot followed by space)
	dotSource = regexp.MustCompile(`^\.\s+`)

	// Environment manipulation
	pathTmp      = regexp.MustCompile(`PATH=.*:/tmp`)
	ldPreload    = regexp.MustCompile(`LD_PRELOAD=`)
	ldLibPath    = regexp.MustCompile(`LD_LIBRARY_PATH=`)
)

// Check evaluates the command for privilege escalation patterns.
func (m *PrivilegeModule) Check(ctx *CheckContext) *Verdict {
	cmd := ctx.Command

	// Shell chaining checks
	if v := checkPrivChaining(cmd); v != nil {
		return v
	}

	// Direct privilege escalation
	if v := checkDirectPrivEsc(cmd); v != nil {
		return v
	}

	// Script execution bypass
	if v := checkScriptBypass(cmd); v != nil {
		return v
	}

	// Environment manipulation
	if v := checkEnvManipulation(cmd); v != nil {
		return v
	}

	return nil
}

func checkPrivChaining(cmd string) *Verdict {
	for i, priv := range privEscCmds {
		// && or || chaining
		if strings.Contains(cmd, "&& "+priv+" ") || strings.Contains(cmd, "|| "+priv+" ") {
			return block("Privilege Escalation", "Chained "+priv+" command", "&& "+priv+" / || "+priv)
		}
		// Semicolon chaining
		if strings.Contains(cmd, "; "+priv) {
			return block("Privilege Escalation", "Semicolon chained "+priv, "; "+priv)
		}
		// Pipe to privilege command (pre-compiled patterns)
		if privPipePatterns[i].MatchString(cmd) {
			return block("Privilege Escalation", "Piped to "+priv+" command", "| "+priv)
		}
	}
	return nil
}

func checkDirectPrivEsc(cmd string) *Verdict {
	for _, priv := range privEscCmds {
		// Command at start of line
		if strings.HasPrefix(cmd, priv+" ") || cmd == priv {
			return block("Privilege Escalation", priv+" command not permitted", priv)
		}
	}
	return nil
}

func checkScriptBypass(cmd string) *Verdict {
	if bashC.MatchString(cmd) {
		return block("Script Bypass", "bash -c execution", "bash -c")
	}
	if shC.MatchString(cmd) {
		return block("Script Bypass", "sh -c execution", "sh -c")
	}
	if zshC.MatchString(cmd) {
		return block("Script Bypass", "zsh -c execution", "zsh -c")
	}
	if evalRe.MatchString(cmd) {
		return block("Script Bypass", "eval with dangerous command", "eval")
	}
	if sourceCmd.MatchString(cmd) {
		return block("Script Bypass", "source command not permitted", "source")
	}
	if dotSource.MatchString(cmd) {
		return block("Script Bypass", "dot source command not permitted", ". (dot source)")
	}
	return nil
}

func checkEnvManipulation(cmd string) *Verdict {
	if pathTmp.MatchString(cmd) {
		return block("Environment Manipulation", "PATH modification with /tmp", "PATH=/tmp")
	}
	if ldPreload.MatchString(cmd) {
		return block("Environment Manipulation", "LD_PRELOAD injection attempt", "LD_PRELOAD")
	}
	if ldLibPath.MatchString(cmd) {
		return block("Environment Manipulation", "LD_LIBRARY_PATH injection attempt", "LD_LIBRARY_PATH")
	}
	return nil
}
