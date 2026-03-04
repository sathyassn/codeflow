package githooks

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

// PreCommitErrors collects all pre-commit validation failures.
type PreCommitErrors struct {
	Errors []string
}

func (e *PreCommitErrors) Error() string {
	return fmt.Sprintf("pre-commit validation failed with %d error(s)", len(e.Errors))
}

func (e *PreCommitErrors) add(msg string) {
	e.Errors = append(e.Errors, msg)
}

// RunPreCommitValidate runs the pure-logic pre-commit checks:
//   - Branch protection
//   - Sensitive file detection
//   - JSON validation
//   - Go test conventions (t.Parallel, context.TODO)
//   - Coverage enforcement (test file pairing)
func RunPreCommitValidate(w io.Writer, projectDir string, policy *EnforcementPolicy) error {
	errs := &PreCommitErrors{}

	// Get current branch.
	currentBranch, err := gitOutput("branch", "--show-current")
	if err != nil {
		return fmt.Errorf("get current branch: %w", err)
	}

	// 1. Branch protection.
	checkBranchProtection(currentBranch, policy, errs)
	if len(errs.Errors) > 0 {
		// Branch protection is fatal — don't continue.
		return errs
	}
	fmt.Fprintf(w, "\033[0;32m✓ Branch protection: Not on a protected branch\033[0m\n")

	// 2. Branch name validation.
	checkBranchName(w, currentBranch, policy, errs)
	if len(errs.Errors) > 0 {
		return errs
	}
	fmt.Fprintf(w, "\033[0;32m✓ Branch name valid: %s\033[0m\n", currentBranch)

	// Get staged files.
	stagedFiles, err := getStagedFiles()
	if err != nil {
		return fmt.Errorf("get staged files: %w", err)
	}

	// 3. Sensitive file check.
	checkSensitiveFiles(w, stagedFiles, policy, errs)
	if len(errs.Errors) > 0 {
		return errs
	}
	fmt.Fprintf(w, "\033[0;32m✓ No sensitive files detected\033[0m\n")

	// 4. JSON validation.
	checkJSONFiles(w, projectDir, stagedFiles, errs)
	if len(errs.Errors) > 0 {
		return errs
	}

	// 5. Go test conventions.
	checkGoTestConventions(w, projectDir, stagedFiles, errs)
	if len(errs.Errors) > 0 {
		return errs
	}

	// 6. Go test file pairing.
	checkGoTestFilePairing(w, projectDir, stagedFiles, errs)
	if len(errs.Errors) > 0 {
		return errs
	}

	return nil
}

func checkBranchProtection(branch string, policy *EnforcementPolicy, errs *PreCommitErrors) {
	for _, pattern := range policy.ProtectedBranches {
		if matchBranchPattern(branch, pattern) {
			errs.add(fmt.Sprintf("BRANCH PROTECTION: Direct commits to %s FORBIDDEN", branch))
			return
		}
	}
}

func checkBranchName(w io.Writer, branch string, policy *EnforcementPolicy, errs *PreCommitErrors) {
	// Skip branch name validation in detached HEAD (e.g., CI merge refs).
	if branch == "" {
		return
	}
	types := policy.GitFormat.BranchTypes
	if len(types) == 0 {
		return
	}

	pattern := "^(" + strings.Join(types, "|") + ")/[a-z0-9]+(-[a-z0-9]+)*$"
	re, err := regexp.Compile(pattern)
	if err != nil {
		return
	}

	if !re.MatchString(branch) {
		errs.add(fmt.Sprintf("INVALID BRANCH NAME: %s. Format: type/slug-name (kebab-case). Types: %s",
			branch, strings.Join(types, ", ")))
	}
}

func getStagedFiles() ([]string, error) {
	out, err := gitOutput("diff", "--cached", "--name-only", "--diff-filter=ACM")
	if err != nil {
		return nil, err
	}
	if out == "" {
		return nil, nil
	}
	return strings.Split(out, "\n"), nil
}

func checkSensitiveFiles(w io.Writer, stagedFiles []string, policy *EnforcementPolicy, errs *PreCommitErrors) {
	patterns := policy.SensitiveFilePatterns
	if len(patterns) == 0 {
		return
	}

	for _, file := range stagedFiles {
		lower := strings.ToLower(file)
		for _, pattern := range patterns {
			re, err := regexp.Compile("(?i)" + pattern)
			if err != nil {
				continue
			}
			if re.MatchString(lower) {
				errs.add(fmt.Sprintf("potentially sensitive file staged: %s (matched: %s)", file, pattern))
				return
			}
		}
	}
}

func checkJSONFiles(w io.Writer, projectDir string, stagedFiles []string, errs *PreCommitErrors) {
	var jsonFiles []string
	for _, f := range stagedFiles {
		if strings.HasSuffix(f, ".json") {
			jsonFiles = append(jsonFiles, f)
		}
	}

	if len(jsonFiles) == 0 {
		return
	}

	fmt.Fprintf(w, "Validating JSON files...\n")
	for _, f := range jsonFiles {
		fullPath := filepath.Join(projectDir, f)
		data, err := os.ReadFile(fullPath)
		if err != nil {
			continue // File might have been deleted.
		}
		if !json.Valid(data) {
			errs.add(fmt.Sprintf("invalid JSON: %s", f))
		}
	}

	if len(errs.Errors) == 0 {
		fmt.Fprintf(w, "\033[0;32m✓ JSON files are valid\033[0m\n")
	}
}

func checkGoTestConventions(w io.Writer, projectDir string, stagedFiles []string, errs *PreCommitErrors) {
	var testFiles []string
	for _, f := range stagedFiles {
		if strings.HasSuffix(f, "_test.go") {
			testFiles = append(testFiles, f)
		}
	}

	if len(testFiles) == 0 {
		return
	}

	fmt.Fprintf(w, "Checking Go test conventions...\n")

	// Compile regexes once.
	contextBgRe := regexp.MustCompile(`context\.Background\(\)`)
	contextBgEscapeRe := regexp.MustCompile(`// NOCHECK: context\.Background`)
	testFuncRe := regexp.MustCompile(`^func (Test\w+)\(`)
	tParallelRe := regexp.MustCompile(`t\.Parallel\(\)|// NOTE: no t\.Parallel`)

	for _, f := range testFiles {
		fullPath := filepath.Join(projectDir, f)
		data, err := os.ReadFile(fullPath)
		if err != nil {
			continue
		}
		content := string(data)
		lines := strings.Split(content, "\n")

		// Check context.Background().
		for i, line := range lines {
			if contextBgRe.MatchString(line) && !contextBgEscapeRe.MatchString(line) {
				errs.add(fmt.Sprintf("BLOCK %s:%d: uses context.Background() (use t.Context() or add // NOCHECK: context.Background)", f, i+1))
			}
		}

		// Check missing t.Parallel().
		for i, line := range lines {
			m := testFuncRe.FindStringSubmatch(line)
			if m == nil {
				continue
			}
			funcName := m[1]

			// Check next 5 lines for t.Parallel() or escape hatch.
			end := i + 6
			if end > len(lines) {
				end = len(lines)
			}
			window := strings.Join(lines[i:end], "\n")
			if !tParallelRe.MatchString(window) {
				errs.add(fmt.Sprintf("BLOCK %s:%s: missing t.Parallel() (add t.Parallel() or // NOTE: no t.Parallel)", f, funcName))
			}
		}
	}

	if len(errs.Errors) == 0 {
		fmt.Fprintf(w, "\033[0;32m✓ Go test convention check passed\033[0m\n")
	}
}

func checkGoTestFilePairing(w io.Writer, projectDir string, stagedFiles []string, errs *PreCommitErrors) {
	var goFiles []string
	for _, f := range stagedFiles {
		if strings.HasSuffix(f, ".go") && !strings.HasSuffix(f, "_test.go") &&
			filepath.Base(f) != "doc.go" && strings.HasPrefix(f, "codeflow-cli/") {
			goFiles = append(goFiles, f)
		}
	}

	if len(goFiles) == 0 {
		return
	}

	// Load exceptions from test config.
	exceptionFiles := loadGoTestExceptions(projectDir)

	for _, f := range goFiles {
		relPath := strings.TrimPrefix(f, "codeflow-cli/")
		if isException(relPath, exceptionFiles) {
			continue
		}

		testFile := strings.TrimSuffix(f, ".go") + "_test.go"
		fullTestPath := filepath.Join(projectDir, testFile)
		if _, err := os.Stat(fullTestPath); os.IsNotExist(err) {
			errs.add(fmt.Sprintf("GO TEST FILE MISSING: %s -> %s", f, testFile))
		}
	}

	if len(errs.Errors) == 0 {
		fmt.Fprintf(w, "\033[0;32m✓ All staged Go files have corresponding test files\033[0m\n")
	}
}

func loadGoTestExceptions(projectDir string) []string {
	configPath := filepath.Join(projectDir, "codeflow-cli", "config", "testing", "test-config.json")
	data, err := os.ReadFile(configPath)
	if err != nil {
		return nil
	}

	var config struct {
		Conventions struct {
			Exceptions []struct {
				File string `json:"file"`
			} `json:"exceptions"`
		} `json:"conventions"`
	}
	if err := json.Unmarshal(data, &config); err != nil {
		return nil
	}

	var files []string
	for _, e := range config.Conventions.Exceptions {
		if e.File != "" {
			files = append(files, e.File)
		}
	}
	return files
}

func isException(relPath string, exceptions []string) bool {
	for _, e := range exceptions {
		if strings.Contains(relPath, e) {
			return true
		}
	}
	return false
}
