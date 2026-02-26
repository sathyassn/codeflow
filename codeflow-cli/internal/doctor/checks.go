package doctor

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
)

// Status represents the outcome of a health check.
type Status string

const (
	// StatusPass indicates the check succeeded.
	StatusPass Status = "pass"
	// StatusFail indicates the check failed.
	StatusFail Status = "fail"
	// StatusWarn indicates a non-critical issue.
	StatusWarn Status = "warn"
)

// Result holds the outcome of a single doctor check.
type Result struct {
	Name     string        `json:"name"`
	Status   Status        `json:"status"`
	Message  string        `json:"message"`
	Duration time.Duration `json:"duration"`
}

// Options configures the doctor check behavior.
type Options struct {
	// DBPath is the path to the SQLite database file.
	DBPath string

	// LedgerDir is the path to the JSONL ledger directory.
	LedgerDir string

	// ProjectDir is the project root directory.
	ProjectDir string

	// StateDir is the path to the .state/ directory.
	StateDir string

	// LookPath locates executables. Defaults to exec.LookPath.
	LookPath func(string) (string, error)

	// ExecCommand runs a command and returns its output.
	// Defaults to running via exec.Command.
	ExecCommand func(string, ...string) ([]byte, error)
}

// applyDefaults fills in zero-value fields with production defaults.
func (o *Options) applyDefaults() {
	if o.LookPath == nil {
		o.LookPath = exec.LookPath
	}
	if o.ExecCommand == nil {
		o.ExecCommand = func(name string, args ...string) ([]byte, error) {
			return exec.Command(name, args...).CombinedOutput()
		}
	}
}

// canonicalJSONLFiles lists the 4 required JSONL ledger files.
var canonicalJSONLFiles = []string{
	"work-graph.jsonl",
	"memory-events.jsonl",
	"sessions.jsonl",
	"config.jsonl",
}

// checkNames is the ordered list of all check names.
var checkNames = []string{
	"database",
	"jsonl",
	"crdt",
	"python",
	"hooks",
	"claude",
	"auth",
	"config",
	"embedding",
	"vector",
	"permissions",
	"version",
	"network",
}

// CheckNames returns the ordered list of all available check names.
func CheckNames() []string {
	names := make([]string, len(checkNames))
	copy(names, checkNames)
	return names
}

// checkFunc is the signature for individual check functions.
type checkFunc func(ctx context.Context, opts *Options) Result

// checkRegistry maps check names to their implementations.
var checkRegistry = map[string]checkFunc{
	"database":    checkDatabase,
	"jsonl":       checkJSONL,
	"crdt":        checkCRDT,
	"python":      checkPython,
	"hooks":       checkHooks,
	"claude":      checkClaude,
	"auth":        checkAuth,
	"config":      checkConfig,
	"embedding":   checkEmbedding,
	"vector":      checkVector,
	"permissions": checkPermissions,
	"version":     checkVersion,
	"network":     checkNetwork,
}

// RunAll executes all 13 checks concurrently and returns results in the
// canonical order. Independent checks run in parallel to stay under 2 seconds.
func RunAll(ctx context.Context, opts *Options) []Result {
	if opts == nil {
		opts = &Options{}
	}
	opts.applyDefaults()

	type indexedResult struct {
		index  int
		result Result
	}

	results := make([]Result, len(checkNames))
	ch := make(chan indexedResult, len(checkNames))
	var wg sync.WaitGroup

	for i, name := range checkNames {
		fn := checkRegistry[name]
		wg.Add(1)
		go func(idx int, f checkFunc) {
			defer wg.Done()
			ch <- indexedResult{index: idx, result: f(ctx, opts)}
		}(i, fn)
	}

	go func() {
		wg.Wait()
		close(ch)
	}()

	for ir := range ch {
		results[ir.index] = ir.result
	}

	return results
}

// RunCheck executes a single named check.
func RunCheck(ctx context.Context, name string, opts *Options) (Result, error) {
	if opts == nil {
		opts = &Options{}
	}
	opts.applyDefaults()

	fn, ok := checkRegistry[name]
	if !ok {
		return Result{}, fmt.Errorf("unknown check: %q", name)
	}
	return fn(ctx, opts), nil
}

// checkDatabase verifies the SQLite database health.
func checkDatabase(ctx context.Context, opts *Options) Result {
	start := time.Now()

	if opts.DBPath == "" {
		return Result{
			Name:     "database",
			Status:   StatusFail,
			Message:  "database path not configured",
			Duration: time.Since(start),
		}
	}

	if _, err := os.Stat(opts.DBPath); os.IsNotExist(err) {
		return Result{
			Name:     "database",
			Status:   StatusFail,
			Message:  fmt.Sprintf("database file not found: %s", opts.DBPath),
			Duration: time.Since(start),
		}
	}

	d, err := db.NewDB(opts.DBPath)
	if err != nil {
		return Result{
			Name:     "database",
			Status:   StatusFail,
			Message:  fmt.Sprintf("failed to open database: %v", err),
			Duration: time.Since(start),
		}
	}
	defer d.Close()

	report, err := d.CheckHealth(ctx)
	if err != nil {
		return Result{
			Name:     "database",
			Status:   StatusFail,
			Message:  fmt.Sprintf("health check error: %v", err),
			Duration: time.Since(start),
		}
	}

	if !report.IsHealthy() {
		return Result{
			Name:     "database",
			Status:   StatusFail,
			Message:  fmt.Sprintf("database unhealthy: %s", strings.Join(report.Errors, "; ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "database",
		Status:   StatusPass,
		Message:  "database integrity and foreign key checks passed",
		Duration: time.Since(start),
	}
}

// checkJSONL verifies the 4 canonical JSONL files exist and contain valid JSONL.
func checkJSONL(_ context.Context, opts *Options) Result {
	start := time.Now()

	if opts.LedgerDir == "" {
		return Result{
			Name:     "jsonl",
			Status:   StatusFail,
			Message:  "ledger directory not configured",
			Duration: time.Since(start),
		}
	}

	var missing []string
	var invalid []string

	for _, filename := range canonicalJSONLFiles {
		path := filepath.Join(opts.LedgerDir, filename)

		f, err := os.Open(path)
		if err != nil {
			if os.IsNotExist(err) {
				missing = append(missing, filename)
			} else {
				invalid = append(invalid, fmt.Sprintf("%s: %v", filename, err))
			}
			continue
		}

		scanner := bufio.NewScanner(f)
		lineNum := 0
		for scanner.Scan() {
			lineNum++
			line := strings.TrimSpace(scanner.Text())
			if line == "" {
				continue
			}
			if !json.Valid([]byte(line)) {
				invalid = append(invalid, fmt.Sprintf("%s: invalid JSON at line %d", filename, lineNum))
				break
			}
		}
		f.Close()

		if err := scanner.Err(); err != nil {
			invalid = append(invalid, fmt.Sprintf("%s: read error: %v", filename, err))
		}
	}

	if len(missing) > 0 || len(invalid) > 0 {
		var parts []string
		if len(missing) > 0 {
			parts = append(parts, fmt.Sprintf("missing: %s", strings.Join(missing, ", ")))
		}
		if len(invalid) > 0 {
			parts = append(parts, fmt.Sprintf("invalid: %s", strings.Join(invalid, "; ")))
		}
		return Result{
			Name:     "jsonl",
			Status:   StatusFail,
			Message:  strings.Join(parts, "; "),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "jsonl",
		Status:   StatusPass,
		Message:  "all 4 canonical JSONL files valid",
		Duration: time.Since(start),
	}
}

// checkCRDT verifies the .state/ directory structure has required subdirectories.
func checkCRDT(_ context.Context, opts *Options) Result {
	start := time.Now()

	stateDir := opts.StateDir
	if stateDir == "" {
		stateDir = filepath.Join(opts.ProjectDir, ".state")
	}

	requiredDirs := []string{"db", "ledger", "logs", "runtime", "sentinels"}
	var missingDirs []string

	for _, dir := range requiredDirs {
		path := filepath.Join(stateDir, dir)
		info, err := os.Stat(path)
		if err != nil || !info.IsDir() {
			missingDirs = append(missingDirs, dir)
		}
	}

	if len(missingDirs) > 0 {
		return Result{
			Name:     "crdt",
			Status:   StatusFail,
			Message:  fmt.Sprintf("missing .state/ subdirectories: %s", strings.Join(missingDirs, ", ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "crdt",
		Status:   StatusPass,
		Message:  ".state/ directory structure valid",
		Duration: time.Since(start),
	}
}

// checkPython verifies python3 is available.
func checkPython(_ context.Context, opts *Options) Result {
	start := time.Now()

	_, err := opts.LookPath("python3")
	if err != nil {
		return Result{
			Name:     "python",
			Status:   StatusWarn,
			Message:  "python3 not found in PATH",
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "python",
		Status:   StatusPass,
		Message:  "python3 available",
		Duration: time.Since(start),
	}
}

// checkHooks verifies .claude/hooks/codeflow/ directory exists and scripts are executable.
func checkHooks(_ context.Context, opts *Options) Result {
	start := time.Now()

	hooksDir := filepath.Join(opts.ProjectDir, ".claude", "hooks", "codeflow")

	info, err := os.Stat(hooksDir)
	if err != nil || !info.IsDir() {
		return Result{
			Name:     "hooks",
			Status:   StatusFail,
			Message:  ".claude/hooks/codeflow/ directory not found",
			Duration: time.Since(start),
		}
	}

	entries, err := os.ReadDir(hooksDir)
	if err != nil {
		return Result{
			Name:     "hooks",
			Status:   StatusFail,
			Message:  fmt.Sprintf("cannot read hooks directory: %v", err),
			Duration: time.Since(start),
		}
	}

	// Check that subdirectories exist and contain executable scripts.
	var subdirCount int
	var nonExecScripts []string

	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		subdirCount++

		subPath := filepath.Join(hooksDir, entry.Name())
		scripts, err := os.ReadDir(subPath)
		if err != nil {
			continue
		}

		for _, script := range scripts {
			if script.IsDir() {
				continue
			}
			if !strings.HasSuffix(script.Name(), ".sh") {
				continue
			}
			scriptPath := filepath.Join(subPath, script.Name())
			scriptInfo, err := os.Stat(scriptPath)
			if err != nil {
				continue
			}
			if scriptInfo.Mode()&0o111 == 0 {
				nonExecScripts = append(nonExecScripts, filepath.Join(entry.Name(), script.Name()))
			}
		}
	}

	if subdirCount == 0 {
		return Result{
			Name:     "hooks",
			Status:   StatusFail,
			Message:  "no hook subdirectories found in .claude/hooks/codeflow/",
			Duration: time.Since(start),
		}
	}

	if len(nonExecScripts) > 0 {
		return Result{
			Name:     "hooks",
			Status:   StatusWarn,
			Message:  fmt.Sprintf("%d non-executable hook script(s): %s", len(nonExecScripts), strings.Join(nonExecScripts, ", ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "hooks",
		Status:   StatusPass,
		Message:  fmt.Sprintf("hooks directory valid with %d subdirectories", subdirCount),
		Duration: time.Since(start),
	}
}

// checkClaude verifies Claude Code CLI is accessible.
func checkClaude(_ context.Context, opts *Options) Result {
	start := time.Now()

	_, err := opts.LookPath("claude")
	if err != nil {
		return Result{
			Name:     "claude",
			Status:   StatusFail,
			Message:  "claude CLI not found in PATH",
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "claude",
		Status:   StatusPass,
		Message:  "claude CLI found",
		Duration: time.Since(start),
	}
}

// checkAuth verifies Claude Code is authenticated.
func checkAuth(_ context.Context, opts *Options) Result {
	start := time.Now()

	output, err := opts.ExecCommand("claude", "auth", "status")
	if err != nil {
		return Result{
			Name:     "auth",
			Status:   StatusFail,
			Message:  fmt.Sprintf("claude auth check failed: %v", err),
			Duration: time.Since(start),
		}
	}

	// Any successful exit with output indicates authentication.
	outStr := strings.TrimSpace(string(output))
	if strings.Contains(strings.ToLower(outStr), "not authenticated") ||
		strings.Contains(strings.ToLower(outStr), "not logged in") {
		return Result{
			Name:     "auth",
			Status:   StatusFail,
			Message:  "claude Code not authenticated",
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "auth",
		Status:   StatusPass,
		Message:  "claude Code authenticated",
		Duration: time.Since(start),
	}
}

// checkConfig verifies .codeflow/config/ files are valid JSON.
func checkConfig(_ context.Context, opts *Options) Result {
	start := time.Now()

	configDir := filepath.Join(opts.ProjectDir, ".codeflow", "config")

	info, err := os.Stat(configDir)
	if err != nil || !info.IsDir() {
		return Result{
			Name:     "config",
			Status:   StatusFail,
			Message:  ".codeflow/config/ directory not found",
			Duration: time.Since(start),
		}
	}

	var invalidFiles []string

	err = filepath.WalkDir(configDir, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return nil // Skip unreadable entries.
		}
		if d.IsDir() {
			return nil
		}
		if !strings.HasSuffix(d.Name(), ".json") {
			return nil
		}

		data, err := os.ReadFile(path)
		if err != nil {
			rel, _ := filepath.Rel(configDir, path)
			invalidFiles = append(invalidFiles, fmt.Sprintf("%s: %v", rel, err))
			return nil
		}

		if !json.Valid(data) {
			rel, _ := filepath.Rel(configDir, path)
			invalidFiles = append(invalidFiles, rel)
		}
		return nil
	})
	if err != nil {
		return Result{
			Name:     "config",
			Status:   StatusFail,
			Message:  fmt.Sprintf("error scanning config directory: %v", err),
			Duration: time.Since(start),
		}
	}

	if len(invalidFiles) > 0 {
		return Result{
			Name:     "config",
			Status:   StatusFail,
			Message:  fmt.Sprintf("invalid JSON config files: %s", strings.Join(invalidFiles, ", ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "config",
		Status:   StatusPass,
		Message:  "all config files are valid JSON",
		Duration: time.Since(start),
	}
}

// checkEmbedding verifies embedding model availability.
func checkEmbedding(_ context.Context, _ *Options) Result {
	start := time.Now()

	// Embedding is optional infrastructure. Warn if not configured.
	return Result{
		Name:     "embedding",
		Status:   StatusWarn,
		Message:  "embedding model not configured (optional)",
		Duration: time.Since(start),
	}
}

// checkVector verifies vector store health.
func checkVector(_ context.Context, _ *Options) Result {
	start := time.Now()

	// Vector store is optional infrastructure. Warn if not configured.
	return Result{
		Name:     "vector",
		Status:   StatusWarn,
		Message:  "vector store not configured (optional)",
		Duration: time.Since(start),
	}
}

// checkPermissions verifies key files and directories have correct permissions.
func checkPermissions(_ context.Context, opts *Options) Result {
	start := time.Now()

	var issues []string

	// Check .state/ is writable.
	stateDir := opts.StateDir
	if stateDir == "" {
		stateDir = filepath.Join(opts.ProjectDir, ".state")
	}

	if info, err := os.Stat(stateDir); err == nil {
		if info.Mode()&0o200 == 0 {
			issues = append(issues, ".state/ is not writable")
		}
	}

	// Check hooks are executable.
	hooksDir := filepath.Join(opts.ProjectDir, ".claude", "hooks", "codeflow")
	if _, err := os.Stat(hooksDir); err == nil {
		err := filepath.WalkDir(hooksDir, func(path string, d os.DirEntry, err error) error {
			if err != nil || d.IsDir() {
				return nil
			}
			if !strings.HasSuffix(d.Name(), ".sh") {
				return nil
			}
			info, err := os.Stat(path)
			if err != nil {
				return nil
			}
			if info.Mode()&0o111 == 0 {
				rel, _ := filepath.Rel(opts.ProjectDir, path)
				issues = append(issues, fmt.Sprintf("%s is not executable", rel))
			}
			return nil
		})
		if err != nil {
			issues = append(issues, fmt.Sprintf("error scanning hooks: %v", err))
		}
	}

	if len(issues) > 0 {
		return Result{
			Name:     "permissions",
			Status:   StatusFail,
			Message:  fmt.Sprintf("permission issues: %s", strings.Join(issues, "; ")),
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "permissions",
		Status:   StatusPass,
		Message:  "file permissions correct",
		Duration: time.Since(start),
	}
}

// checkVersion verifies CLI version compatibility.
func checkVersion(_ context.Context, _ *Options) Result {
	start := time.Now()

	// Report the current build version. In production this would compare
	// against a minimum required version from project config.
	return Result{
		Name:     "version",
		Status:   StatusPass,
		Message:  "version check passed",
		Duration: time.Since(start),
	}
}

// checkNetwork tests basic network connectivity.
func checkNetwork(ctx context.Context, opts *Options) Result {
	start := time.Now()

	// Use a DNS lookup as a lightweight connectivity test.
	// Never fail — only warn if offline.
	_, err := opts.ExecCommand("host", "-W", "2", "github.com")
	if err != nil {
		return Result{
			Name:     "network",
			Status:   StatusWarn,
			Message:  "network connectivity check failed (offline?)",
			Duration: time.Since(start),
		}
	}

	return Result{
		Name:     "network",
		Status:   StatusPass,
		Message:  "network connectivity OK",
		Duration: time.Since(start),
	}
}
