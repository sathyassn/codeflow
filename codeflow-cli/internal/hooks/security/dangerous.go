package security

import (
	"regexp"
	"strings"
)

// DangerousModule detects and blocks destructive commands.
// Go equivalent of cf-dangerous-commands.sh.
type DangerousModule struct{}

// Name returns the module name.
func (m *DangerousModule) Name() string { return "dangerous-commands" }

// dangerousPatterns are substring patterns that always block.
var dangerousPatterns = []string{
	"dd if=/dev/zero",
	"dd if=/dev/random",
	"mkfs.",
	"> /dev/sd",
}

// Regex patterns for dangerous commands.
var (
	// rm -rf / (with r before optional f)
	rmRfRoot = regexp.MustCompile(`rm\s+-[a-zA-Z]*r[a-zA-Z]*f?\s+/(\s|$|\*)`)
	// rm -fr / (with f before r)
	rmFrRoot = regexp.MustCompile(`rm\s+-[a-zA-Z]*f[a-zA-Z]*r[a-zA-Z]*\s+/(\s|$|\*)`)
	// rm -r ~ (home directory)
	rmHome = regexp.MustCompile(`rm\s+-[a-zA-Z]*r[a-zA-Z]*\s+~(\s|$|/\*)`)
	// rm -r /system-dir
	rmSystemDir = regexp.MustCompile(`rm\s+-[a-zA-Z]*r[a-zA-Z]*\s+/(etc|var|usr|bin|sbin|boot|lib|lib64|opt|root|sys|proc)(\s|$|/)`)

	// dd to disk devices
	ddDisk = regexp.MustCompile(`dd\s.*of=/dev/(sd|hd|nvme|vd)[a-z]`)
	// Format commands
	formatCmd = regexp.MustCompile(`(mkfs|mke2fs|mkswap)\s`)

	// chmod 777 on system paths
	chmod777 = regexp.MustCompile(`chmod\s.*777\s+/(etc|var|usr|bin|sbin|boot|lib|opt|root)(\s|$|/)|chmod\s.*777\s+/(\s|$)`)
	// chmod -R /
	chmodR = regexp.MustCompile(`chmod\s+-R\s.*\s/($|\s)`)
	// chown -R /
	chownR = regexp.MustCompile(`chown\s+-R\s.*\s/($|\s)`)

	// Fork bomb: :(){ :|:& };:
	forkBomb = regexp.MustCompile(`:\(\)\s*\{\s*.*\|.*:\s*&\s*\}\s*;\s*:`)
	// Dot variant: .(){.|.&};.
	forkBombDot = regexp.MustCompile(`\.\(\)\s*\{\s*.*\|.*\.\s*&\s*\}\s*;\s*\.`)
)

// Check evaluates the command for dangerous patterns.
func (m *DangerousModule) Check(ctx *CheckContext) *Verdict {
	cmd := ctx.Command

	// Substring pattern checks
	for _, pattern := range dangerousPatterns {
		if strings.Contains(cmd, pattern) {
			return block("Dangerous Command", "Destructive operation detected", pattern)
		}
	}

	// Recursive deletion checks
	if v := checkRecursiveDelete(cmd); v != nil {
		return v
	}

	// Disk operation checks
	if v := checkDiskOperations(cmd); v != nil {
		return v
	}

	// Permission change checks
	if v := checkPermissionChanges(cmd); v != nil {
		return v
	}

	// Fork bomb checks
	if v := checkForkBomb(cmd); v != nil {
		return v
	}

	return nil
}

func checkRecursiveDelete(cmd string) *Verdict {
	if rmRfRoot.MatchString(cmd) {
		return block("Dangerous Command", "Recursive deletion of root directory", "rm -r[f] /")
	}
	if rmFrRoot.MatchString(cmd) {
		return block("Dangerous Command", "Recursive forced deletion of root", "rm -fr /")
	}
	if rmHome.MatchString(cmd) {
		return block("Dangerous Command", "Recursive deletion of home directory", "rm -r ~")
	}
	if rmSystemDir.MatchString(cmd) {
		return block("Dangerous Command", "Recursive deletion of system directory", "rm -r /system-dir")
	}
	return nil
}

func checkDiskOperations(cmd string) *Verdict {
	if ddDisk.MatchString(cmd) {
		return block("Dangerous Command", "Direct disk write operation", "dd of=/dev/*")
	}
	if formatCmd.MatchString(cmd) {
		return block("Dangerous Command", "Disk format operation", "mkfs/mke2fs/mkswap")
	}
	return nil
}

func checkPermissionChanges(cmd string) *Verdict {
	if chmod777.MatchString(cmd) {
		return block("Dangerous Command", "Dangerous permission change on system path", "chmod 777 /system-path")
	}
	if chmodR.MatchString(cmd) {
		return block("Dangerous Command", "Recursive permission change on root", "chmod -R /")
	}
	if chownR.MatchString(cmd) {
		return block("Dangerous Command", "Recursive ownership change on root", "chown -R /")
	}
	return nil
}

func checkForkBomb(cmd string) *Verdict {
	if forkBomb.MatchString(cmd) {
		return block("Dangerous Command", "Fork bomb detected", ":(){:|:&};:")
	}
	if forkBombDot.MatchString(cmd) {
		return block("Dangerous Command", "Fork bomb variation detected", ".(){.|.&};.")
	}
	return nil
}
