package security

import (
	"regexp"
	"strings"
)

// PathModule protects sensitive paths from dangerous operations.
// Go equivalent of cf-path-protection.sh.
type PathModule struct{}

// Name returns the module name.
func (m *PathModule) Name() string { return "path-protection" }

// Default dangerous and permission command patterns.
var (
	dangerousCmds  = regexp.MustCompile(`(rm|unlink|mv|cp|shred|truncate|touch|sed)\s`)
	permissionCmds = regexp.MustCompile(`(chmod|chown)\s`)
	gitRm          = regexp.MustCompile(`git\s+rm`)
	cpCmd          = regexp.MustCompile(`(^|\s)(cp)\s`)
	otherDangerous = regexp.MustCompile(`(rm|unlink|mv|shred|truncate|touch|sed)\s`)
)

// Pre-compiled directory protection patterns for .claude and .codeflow.
type dirProtectionPatterns struct {
	dirEnd   *regexp.Regexp
	dirSlash *regexp.Regexp
}

var protectedDirPatterns = map[string]*dirProtectionPatterns{
	".claude": {
		dirEnd:   regexp.MustCompile(`\s[^\s]*\.claude($|\s|['"])`),
		dirSlash: regexp.MustCompile(`\s[^\s]*\.claude/($|\s|['"])`),
	},
	".codeflow": {
		dirEnd:   regexp.MustCompile(`\s[^\s]*\.codeflow($|\s|['"])`),
		dirSlash: regexp.MustCompile(`\s[^\s]*\.codeflow/($|\s|['"])`),
	},
}

// Check evaluates the command for protected path violations.
func (m *PathModule) Check(ctx *CheckContext) *Verdict {
	cmd := ctx.Command

	paths := protectedPaths(ctx.Policy)
	if len(paths) == 0 {
		return nil
	}

	segments := splitCommandSegments(cmd)

	// Check protected path operations per segment
	for _, path := range paths {
		for _, seg := range segments {
			if v := checkSegmentDangerousOp(seg, path, ctx); v != nil {
				return v
			}
		}
	}

	// Check variable indirection bypass
	if v := checkVariableIndirection(segments, paths); v != nil {
		return v
	}

	// Check eval bypass
	if v := checkEvalBypass(segments, paths); v != nil {
		return v
	}

	// Check .claude and .codeflow directory-level protection
	if v := checkDirectoryProtection(segments); v != nil {
		return v
	}

	// Check redirect overwrite protection
	if v := checkRedirectProtection(segments, paths); v != nil {
		return v
	}

	return nil
}

func protectedPaths(policy *EnforcementPolicy) []string {
	if policy != nil {
		return policy.AllProtectedPaths()
	}
	return DefaultPolicy().AllProtectedPaths()
}

func checkSegmentDangerousOp(segment, path string, ctx *CheckContext) *Verdict {
	isGlob := isGlobPattern(path)

	// Check if the protected path is in this segment
	pathInSegment := false
	if isGlob {
		pathInSegment = isGlobPathTargeted(segment, path)
	} else {
		pathInSegment = isPathTargeted(segment, path)
	}

	if !pathInSegment {
		return nil
	}

	// cp special handling: only block if protected path is the destination
	if cpCmd.MatchString(segment) {
		if isCPDestination(segment, path, isGlob) {
			return block("Protected Path Deletion", "Dangerous operation on protected path", path)
		}
	}

	// Other dangerous commands
	if otherDangerous.MatchString(segment) {
		return block("Protected Path Deletion", "Dangerous operation on protected path", path)
	}

	// Permission commands
	if permissionCmds.MatchString(segment) {
		return block("Protected Path Manipulation", "Permission change on protected path", path)
	}

	// Git rm
	if gitRm.MatchString(segment) {
		return block("Protected Path Deletion", "Git removal of protected path", path)
	}

	return nil
}

// isCPDestination checks if the protected path is the destination (last non-flag argument) of a cp command.
func isCPDestination(segment, path string, isGlob bool) bool {
	fields := strings.Fields(strings.TrimSpace(segment))
	lastNonFlag := ""
	for _, f := range fields {
		if !strings.HasPrefix(f, "-") {
			lastNonFlag = f
		}
	}
	// Strip surrounding quotes
	lastNonFlag = strings.Trim(lastNonFlag, "\"'")

	if isGlob {
		re, err := regexp.Compile(globToRegex(path))
		if err != nil {
			return false
		}
		return re.MatchString(lastNonFlag)
	}
	return strings.Contains(lastNonFlag, path)
}

func checkDirectoryProtection(segments []string) *Verdict {
	for _, seg := range segments {
		if !dangerousCmds.MatchString(seg) {
			continue
		}

		for dir, patterns := range protectedDirPatterns {
			if patterns.dirEnd.MatchString(seg) || patterns.dirSlash.MatchString(seg) {
				return block("Protected Directory", "Cannot delete "+dir+" directory", dir)
			}
		}
	}

	return nil
}

func checkRedirectProtection(segments []string, paths []string) *Verdict {
	for _, path := range paths {
		for _, seg := range segments {
			if isGlobPattern(path) {
				regex := globToRegex(path)
				// Single redirect
				singleRe, err := regexp.Compile(`([^0-9&>]|^)>[\s]*.*` + regex)
				if err == nil && singleRe.MatchString(seg) {
					return block("Protected File Overwrite", "Redirect overwrite of protected path", "> "+path)
				}
				// Append redirect
				appendRe, err := regexp.Compile(`([^0-9&]|^)>>[\s]*.*` + regex)
				if err == nil && appendRe.MatchString(seg) {
					return block("Protected File Append", "Redirect append to protected path", ">> "+path)
				}
			} else {
				// Single redirect: > protected_path
				// Exclude: 2>path (digit prefix), >&path (& prefix)
				if singleRe, err := regexp.Compile(`([^0-9&>]|^)>\s*` + regexp.QuoteMeta(path) + `($|[\s"'/])`); err == nil && singleRe.MatchString(seg) {
					return block("Protected File Overwrite", "Redirect overwrite of protected path", "> "+path)
				}
				// Append redirect: >> protected_path
				if appendRe, err := regexp.Compile(`([^0-9&]|^)>>\s*` + regexp.QuoteMeta(path) + `($|[\s"'/])`); err == nil && appendRe.MatchString(seg) {
					return block("Protected File Append", "Redirect append to protected path", ">> "+path)
				}
			}
		}
	}

	return nil
}

// checkEvalBypass detects eval commands that contain protected path literals
// anywhere in the compound command. eval can hide arbitrary operations, so
// any mention of a protected path near eval is blocked.
func checkEvalBypass(segments []string, paths []string) *Verdict {
	for _, seg := range segments {
		trimmed := strings.TrimSpace(seg)
		fields := strings.Fields(trimmed)
		if len(fields) == 0 || fields[0] != "eval" {
			continue
		}
		for _, path := range paths {
			if isPathOrGlobTargeted(trimmed, path) {
				return block("Protected Path Indirection",
					"eval command with protected path argument",
					path)
			}
		}
	}
	return nil
}

// checkVariableIndirection detects when a dangerous command uses a shell
// variable that was assigned a protected path value in the same compound
// command. Example: F=".claude/settings.json" && rm $F
func checkVariableIndirection(segments []string, paths []string) *Verdict {
	assignments := extractVariableAssignments(segments)
	if len(assignments) == 0 {
		return nil
	}

	// Find which assigned variables hold protected path values.
	// Also match directory prefixes: if value is ".claude" and a protected
	// path is ".claude/settings.json", the variable targets that path.
	protectedVars := make(map[string]string) // var name -> protected path
	for name, value := range assignments {
		for _, path := range paths {
			if isPathOrGlobTargeted(value, path) || value == path || strings.HasPrefix(path, value+"/") {
				protectedVars[name] = path
				break
			}
		}
	}
	if len(protectedVars) == 0 {
		return nil
	}

	// Check each segment: if it has a dangerous command and variable indirection,
	// and any of the variables refer to a protected path, block it.
	for _, seg := range segments {
		trimmed := strings.TrimSpace(seg)
		if !dangerousCmds.MatchString(trimmed) && !permissionCmds.MatchString(trimmed) && !gitRm.MatchString(trimmed) {
			continue
		}
		if !hasVariableIndirection(trimmed) {
			continue
		}
		for _, protectedPath := range protectedVars {
			return block("Protected Path Indirection",
				"Variable indirection targeting protected path",
				protectedPath)
		}
	}

	return nil
}
