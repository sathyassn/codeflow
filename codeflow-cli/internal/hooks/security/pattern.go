package security

import (
	"regexp"
	"strings"
)

// globToRegex converts a glob pattern to a Go regexp pattern.
// Handles * (single path segment), ** (recursive), and ? (single char).
func globToRegex(pattern string) string {
	escaped := regexp.QuoteMeta(pattern)
	// Order matters: handle ** before * since QuoteMeta escapes * to \*
	// ** was escaped to \*\*, convert to .* (recursive match)
	escaped = strings.ReplaceAll(escaped, `\*\*`, `.*`)
	// * was escaped to \*, convert to [^/]* (single segment match)
	escaped = strings.ReplaceAll(escaped, `\*`, `[^/]*`)
	// ? was escaped to \?, convert to . (single char match)
	escaped = strings.ReplaceAll(escaped, `\?`, `.`)
	return escaped
}

// stripTmpClaudePaths removes /tmp/claude... path segments from a command
// to prevent false positives when operations target safe scratch space.
var tmpClaudeRe = regexp.MustCompile(`/tmp/claude\S*`)

func stripTmpClaudePaths(cmd string) string {
	return tmpClaudeRe.ReplaceAllString(cmd, "")
}

// isPathTargeted checks if a path appears in a command with proper boundaries.
// This prevents ".claude" from matching ".claude-notes.md".
// It also strips /tmp/claude paths to avoid false positives.
func isPathTargeted(cmd, path string) bool {
	if !strings.Contains(cmd, path) {
		return false
	}

	// Strip /tmp/claude paths
	cleaned := stripTmpClaudePaths(cmd)
	if !strings.Contains(cleaned, path) {
		return false
	}

	// Check for valid path boundaries
	idx := 0
	for {
		pos := strings.Index(cmd[idx:], path)
		if pos < 0 {
			break
		}
		pos += idx
		end := pos + len(path)

		// At end of string
		if end >= len(cmd) {
			return true
		}

		next := cmd[end]
		// Path followed by /, space, quote, or end
		if next == '/' || next == ' ' || next == '"' || next == '\'' {
			return true
		}

		idx = pos + 1
	}

	return false
}

// isGlobPathTargeted checks if a command targets a path matching a glob pattern.
func isGlobPathTargeted(cmd, pattern string) bool {
	cleaned := stripTmpClaudePaths(cmd)
	re, err := regexp.Compile(globToRegex(pattern))
	if err != nil {
		return false
	}
	if re.MatchString(cleaned) {
		return true
	}

	// For /** patterns, also match the directory itself
	if strings.HasSuffix(pattern, "/**") {
		dirPath := strings.TrimSuffix(pattern, "/**")
		return isPathTargeted(cmd, dirPath)
	}

	return false
}

// isPathOrGlobTargeted is a unified check for both exact paths and glob patterns.
func isPathOrGlobTargeted(cmd, path string) bool {
	if strings.ContainsAny(path, "*?") {
		return isGlobPathTargeted(cmd, path)
	}
	return isPathTargeted(cmd, path)
}

// isGlobPattern returns true if the path contains glob characters.
func isGlobPattern(path string) bool {
	return strings.ContainsAny(path, "*?")
}

// matchesExtendedGlob matches a path against an extended glob pattern
// supporting ** for recursive directory matching.
func matchesExtendedGlob(path, pattern string) bool {
	regex := globToRegex(pattern)
	re, err := regexp.Compile("^" + regex + "$")
	if err != nil {
		return false
	}
	return re.MatchString(path)
}

// normalizePath removes leading ./, trailing /, resolves double slashes,
// and strips leading ../ components.
func normalizePath(path string) string {
	path = strings.TrimPrefix(path, "./")
	path = strings.TrimSuffix(path, "/")

	// Clean double slashes
	for strings.Contains(path, "//") {
		path = strings.ReplaceAll(path, "//", "/")
	}

	// Strip leading ../
	for strings.HasPrefix(path, "../") {
		path = strings.TrimPrefix(path, "../")
	}

	return path
}

// getFlagsPortion extracts the flags portion of a git commit command
// (before -m/--message) to prevent false positives from commit messages.
func getFlagsPortion(cmd string) string {
	for _, sep := range []string{" --message=", " --message ", " -m\"", " -m'", " -m "} {
		if idx := strings.Index(cmd, sep); idx >= 0 {
			return cmd[:idx]
		}
	}
	return cmd
}

// splitCommandSegments splits a compound command into segments by &&, ||, ;, |
// while respecting quoted strings.
func splitCommandSegments(cmd string) []string {
	var segments []string
	var segment strings.Builder
	inSingle := false
	inDouble := false

	for i := 0; i < len(cmd); i++ {
		ch := cmd[i]

		// Track quote state
		if ch == '\'' && !inDouble {
			inSingle = !inSingle
			segment.WriteByte(ch)
			continue
		}
		if ch == '"' && !inSingle {
			inDouble = !inDouble
			segment.WriteByte(ch)
			continue
		}

		if !inSingle && !inDouble {
			// Check for && or ||
			if i+1 < len(cmd) {
				if ch == '&' && cmd[i+1] == '&' {
					segments = append(segments, segment.String())
					segment.Reset()
					i++ // skip second &
					continue
				}
				if ch == '|' && cmd[i+1] == '|' {
					segments = append(segments, segment.String())
					segment.Reset()
					i++ // skip second |
					continue
				}
			}
			// Semicolon
			if ch == ';' {
				segments = append(segments, segment.String())
				segment.Reset()
				continue
			}
			// Single pipe (not ||)
			if ch == '|' {
				segments = append(segments, segment.String())
				segment.Reset()
				continue
			}
		}

		segment.WriteByte(ch)
	}

	if segment.Len() > 0 {
		segments = append(segments, segment.String())
	}

	return segments
}
