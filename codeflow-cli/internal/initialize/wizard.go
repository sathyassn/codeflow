package initialize

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"

	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/codeflow/codeflow-cli/internal/preflight"
)

// Sentinel errors for the init wizard.
var (
	// ErrCancelled indicates the user cancelled the wizard via Ctrl+C or EOF.
	ErrCancelled = errors.New("init: wizard cancelled")

	// ErrPrereqMissing indicates a required prerequisite was not found.
	ErrPrereqMissing = errors.New("init: prerequisite not found")

	// ErrAuthFailed indicates Claude Code authentication failed.
	ErrAuthFailed = errors.New("init: authentication failed")

	// ErrGitProviderFailed indicates git provider setup failed.
	ErrGitProviderFailed = errors.New("init: git provider setup failed")

	// ErrSetupFailed indicates CodeFlow directory setup failed.
	ErrSetupFailed = errors.New("init: setup failed")

	// ErrVerificationFailed indicates verification checks failed.
	ErrVerificationFailed = errors.New("init: verification failed")
)

// ProjectState describes the detected state of a project directory.
type ProjectState int

const (
	// StateNew indicates no .codeflow/ directory exists.
	StateNew ProjectState = iota
	// StateExisting indicates .codeflow/ and .state/ both exist.
	StateExisting
	// StateJoin indicates .codeflow/ exists (from clone) but no .state/.
	StateJoin
)

// String returns the human-readable name for the project state.
func (s ProjectState) String() string {
	switch s {
	case StateNew:
		return "new"
	case StateExisting:
		return "existing"
	case StateJoin:
		return "join"
	default:
		return "unknown"
	}
}

// GitProvider identifies the git hosting provider.
type GitProvider int

const (
	// ProviderGitHub represents GitHub.
	ProviderGitHub GitProvider = iota
	// ProviderGitLab represents GitLab.
	ProviderGitLab
	// ProviderBitbucket represents Bitbucket.
	ProviderBitbucket
)

// String returns the human-readable name for the git provider.
func (p GitProvider) String() string {
	switch p {
	case ProviderGitHub:
		return "github"
	case ProviderGitLab:
		return "gitlab"
	case ProviderBitbucket:
		return "bitbucket"
	default:
		return "unknown"
	}
}

// Config holds the project configuration gathered during the wizard.
type Config struct {
	Dir           string
	ProjectName   string
	Description   string
	GitProvider   GitProvider
	PathFlowMode  bool
	ProjectState  ProjectState
}

// Wizard holds the wizard dependencies and state.
type Wizard struct {
	In       io.Reader
	Out      io.Writer
	Dir      string
	LookPath func(string) (string, error)
	RunCmd   func(name string, args ...string) ([]byte, error)

	// config is populated as the wizard progresses.
	config Config

	// createdDirs tracks directories created during setup for cleanup.
	createdDirs []string

	// scanner is initialized once for consistent input reading.
	scanner *bufio.Scanner
}

// Run executes the full 7-step wizard. Returns the resulting Config on
// success, or an error if any step fails or the user cancels.
func (w *Wizard) Run(ctx context.Context) (*Config, error) {
	// Set up signal handling for Ctrl+C cleanup.
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)
	defer signal.Stop(sigCh)

	// Run wizard in a goroutine so we can select on cancellation.
	type result struct {
		cfg *Config
		err error
	}
	resCh := make(chan result, 1)

	go func() {
		cfg, err := w.runSteps(ctx)
		resCh <- result{cfg, err}
	}()

	select {
	case res := <-resCh:
		if res.err != nil {
			return nil, res.err
		}
		return res.cfg, nil
	case <-sigCh:
		w.cleanup()
		return nil, ErrCancelled
	case <-ctx.Done():
		w.cleanup()
		return nil, fmt.Errorf("init: %w", ctx.Err())
	}
}

// runSteps executes all 7 wizard steps sequentially.
func (w *Wizard) runSteps(ctx context.Context) (*Config, error) {
	fmt.Fprintln(w.Out, "CodeFlow Project Initialization")
	fmt.Fprintln(w.Out, "================================")
	fmt.Fprintln(w.Out)

	// Step 1: Project location
	if err := w.stepProjectLocation(); err != nil {
		return nil, err
	}

	// Step 2: Prerequisites check
	if err := w.stepPrerequisites(); err != nil {
		return nil, err
	}

	// Step 3: Claude Code authentication
	if err := w.stepClaudeAuth(); err != nil {
		return nil, err
	}

	// Step 4: Git provider setup
	if err := w.stepGitProvider(); err != nil {
		return nil, err
	}

	// Step 5: Project configuration
	if err := w.stepProjectConfig(); err != nil {
		return nil, err
	}

	// Step 6: CodeFlow setup
	if err := w.stepCodeFlowSetup(ctx); err != nil {
		return nil, err
	}

	// Step 7: Verification
	if err := w.stepVerification(ctx); err != nil {
		return nil, err
	}

	fmt.Fprintln(w.Out)
	fmt.Fprintln(w.Out, "Project initialized successfully!")

	return &w.config, nil
}

// stepProjectLocation detects the project state and sets the directory.
func (w *Wizard) stepProjectLocation() error {
	fmt.Fprintln(w.Out, "Step 1/7: Project Location")
	fmt.Fprintln(w.Out, "--------------------------")

	dir := w.Dir
	if dir == "" {
		var err error
		dir, err = os.Getwd()
		if err != nil {
			return fmt.Errorf("init: getting working directory: %w", err)
		}
	}

	// Make absolute.
	absDir, err := filepath.Abs(dir)
	if err != nil {
		return fmt.Errorf("init: resolving path: %w", err)
	}
	w.config.Dir = absDir

	// Detect state.
	w.config.ProjectState = detectProjectState(absDir)

	switch w.config.ProjectState {
	case StateNew:
		fmt.Fprintf(w.Out, "  Directory: %s\n", absDir)
		fmt.Fprintln(w.Out, "  State: new project (no .codeflow/ found)")
	case StateExisting:
		fmt.Fprintf(w.Out, "  Directory: %s\n", absDir)
		fmt.Fprintln(w.Out, "  State: existing project (.codeflow/ and .state/ found)")
		fmt.Fprintln(w.Out, "  Existing project detected. Will validate without overwriting.")
	case StateJoin:
		fmt.Fprintf(w.Out, "  Directory: %s\n", absDir)
		fmt.Fprintln(w.Out, "  State: joining project (.codeflow/ found, no .state/)")
		fmt.Fprintln(w.Out, "  Cloned project detected. Will create local state.")
	}

	fmt.Fprintln(w.Out)
	return nil
}

// stepPrerequisites checks for required tools in PATH.
func (w *Wizard) stepPrerequisites() error {
	fmt.Fprintln(w.Out, "Step 2/7: Prerequisites Check")
	fmt.Fprintln(w.Out, "-----------------------------")

	prereqs := []struct {
		name     string
		required bool
	}{
		{"git", true},
		{"sqlite3", true},
		{"python3", true},
		{"claude", true},
	}

	var missing []string
	for _, p := range prereqs {
		_, err := w.LookPath(p.name)
		if err != nil {
			if p.required {
				fmt.Fprintf(w.Out, "  [FAIL] %s: not found in PATH\n", p.name)
				missing = append(missing, p.name)
			} else {
				fmt.Fprintf(w.Out, "  [WARN] %s: not found (optional)\n", p.name)
			}
		} else {
			fmt.Fprintf(w.Out, "  [ OK ] %s: found\n", p.name)
		}
	}

	fmt.Fprintln(w.Out)

	if len(missing) > 0 {
		return fmt.Errorf("%w: %s", ErrPrereqMissing, strings.Join(missing, ", "))
	}

	return nil
}

// stepClaudeAuth verifies Claude Code authentication.
func (w *Wizard) stepClaudeAuth() error {
	fmt.Fprintln(w.Out, "Step 3/7: Claude Code Authentication")
	fmt.Fprintln(w.Out, "------------------------------------")

	// Try to check auth status first.
	output, err := w.RunCmd("claude", "--version")
	if err != nil {
		fmt.Fprintln(w.Out, "  [FAIL] Claude Code is not responding")
		fmt.Fprintln(w.Out)
		return fmt.Errorf("%w: claude --version failed: %s", ErrAuthFailed, err)
	}

	fmt.Fprintf(w.Out, "  Claude Code version: %s\n", strings.TrimSpace(string(output)))
	fmt.Fprintln(w.Out, "  [ OK ] Claude Code accessible")
	fmt.Fprintln(w.Out)

	return nil
}

// stepGitProvider detects or prompts for git provider configuration.
func (w *Wizard) stepGitProvider() error {
	fmt.Fprintln(w.Out, "Step 4/7: Git Provider Setup")
	fmt.Fprintln(w.Out, "----------------------------")

	// Detect provider from git remote if available.
	provider, detected := w.detectGitProvider()
	if detected {
		w.config.GitProvider = provider
		fmt.Fprintf(w.Out, "  Detected provider: %s\n", provider)
	} else {
		// Prompt for provider selection.
		fmt.Fprintln(w.Out, "  Select git provider:")
		fmt.Fprintln(w.Out, "    1. GitHub")
		fmt.Fprintln(w.Out, "    2. GitLab")
		fmt.Fprintln(w.Out, "    3. Bitbucket")

		choice, err := w.prompt("  Choice [1]: ")
		if err != nil {
			return err
		}

		switch strings.TrimSpace(choice) {
		case "2":
			w.config.GitProvider = ProviderGitLab
		case "3":
			w.config.GitProvider = ProviderBitbucket
		default:
			w.config.GitProvider = ProviderGitHub
		}
	}

	// For GitHub, verify gh CLI is available.
	if w.config.GitProvider == ProviderGitHub {
		if _, err := w.LookPath("gh"); err != nil {
			fmt.Fprintln(w.Out, "  [WARN] gh CLI not found. Some features may be limited.")
		} else {
			fmt.Fprintln(w.Out, "  [ OK ] gh CLI found")
		}
	}

	fmt.Fprintln(w.Out)
	return nil
}

// stepProjectConfig prompts for project name and description.
func (w *Wizard) stepProjectConfig() error {
	fmt.Fprintln(w.Out, "Step 5/7: Project Configuration")
	fmt.Fprintln(w.Out, "-------------------------------")

	// Default name from directory basename.
	defaultName := filepath.Base(w.config.Dir)

	name, err := w.prompt(fmt.Sprintf("  Project name [%s]: ", defaultName))
	if err != nil {
		return err
	}
	name = strings.TrimSpace(name)
	if name == "" {
		name = defaultName
	}
	w.config.ProjectName = name

	desc, err := w.prompt("  Description: ")
	if err != nil {
		return err
	}
	w.config.Description = strings.TrimSpace(desc)

	fmt.Fprintln(w.Out)
	return nil
}

// stepCodeFlowSetup creates the directory structure and initializes the database.
func (w *Wizard) stepCodeFlowSetup(ctx context.Context) error {
	fmt.Fprintln(w.Out, "Step 6/7: CodeFlow Setup")
	fmt.Fprintln(w.Out, "-----------------------")

	root := w.config.Dir

	// Create directory structure.
	dirs := []string{
		filepath.Join(root, ".codeflow", "config"),
		filepath.Join(root, ".codeflow", "scripts"),
		filepath.Join(root, ".claude"),
		filepath.Join(root, ".state", "db"),
		filepath.Join(root, ".state", "ledger"),
		filepath.Join(root, ".state", "session"),
		filepath.Join(root, ".state", "runtime"),
	}

	for _, d := range dirs {
		if err := os.MkdirAll(d, 0o755); err != nil {
			return fmt.Errorf("%w: creating %s: %s", ErrSetupFailed, d, err)
		}
		w.createdDirs = append(w.createdDirs, d)
		fmt.Fprintf(w.Out, "  Created: %s\n", relPath(root, d))
	}

	// Initialize database.
	dbPath := filepath.Join(root, ".state", "db", "codeflow.db")
	d, err := db.NewDB(dbPath)
	if err != nil {
		return fmt.Errorf("%w: opening database: %s", ErrSetupFailed, err)
	}
	defer func() { _ = d.Close() }()

	if err := d.InitFromSchema(ctx); err != nil {
		return fmt.Errorf("%w: initializing schema: %s", ErrSetupFailed, err)
	}
	if _, err := d.Migrate(ctx); err != nil {
		return fmt.Errorf("%w: applying migrations: %s", ErrSetupFailed, err)
	}

	fmt.Fprintln(w.Out, "  Database initialized")

	// Step 6b: PathFlow mode (optional).
	fmt.Fprintln(w.Out)
	choice, err := w.prompt("  Enable PathFlow mode? [y/N]: ")
	if err != nil {
		return err
	}
	w.config.PathFlowMode = strings.HasPrefix(strings.ToLower(strings.TrimSpace(choice)), "y")

	if w.config.PathFlowMode {
		fmt.Fprintln(w.Out, "  PathFlow mode: enabled")
	} else {
		fmt.Fprintln(w.Out, "  PathFlow mode: disabled (can be enabled later via codeflow config set)")
	}

	fmt.Fprintln(w.Out)
	return nil
}

// stepVerification runs preflight health checks.
func (w *Wizard) stepVerification(ctx context.Context) error {
	fmt.Fprintln(w.Out, "Step 7/7: Verification")
	fmt.Fprintln(w.Out, "---------------------")

	opts := &preflight.Options{
		LookPath: w.LookPath,
	}

	results := preflight.Run(ctx, w.config.Dir, opts)

	var hasErrors bool
	for _, r := range results {
		status := "PASS"
		if !r.Passed {
			if r.Level == preflight.Critical {
				status = "FAIL"
				hasErrors = true
			} else {
				status = "WARN"
			}
		}
		fmt.Fprintf(w.Out, "  [%s] %s: %s\n", status, r.Name, r.Message)
	}

	fmt.Fprintln(w.Out)

	if hasErrors {
		return fmt.Errorf("%w: critical checks failed", ErrVerificationFailed)
	}

	if preflight.HasWarnings(results) {
		fmt.Fprintln(w.Out, "  Verification passed with warnings")
	} else {
		fmt.Fprintln(w.Out, "  All checks passed")
	}

	return nil
}

// detectProjectState determines whether the directory is new, existing, or join.
func detectProjectState(dir string) ProjectState {
	codeflowDir := filepath.Join(dir, ".codeflow")
	stateDir := filepath.Join(dir, ".state")

	codeflowInfo, codeflowErr := os.Stat(codeflowDir)
	stateInfo, stateErr := os.Stat(stateDir)

	hasCodeflow := codeflowErr == nil && codeflowInfo.IsDir()
	hasState := stateErr == nil && stateInfo.IsDir()

	switch {
	case hasCodeflow && hasState:
		return StateExisting
	case hasCodeflow && !hasState:
		return StateJoin
	default:
		return StateNew
	}
}

// detectGitProvider tries to determine the git provider from the remote URL.
func (w *Wizard) detectGitProvider() (GitProvider, bool) {
	output, err := w.RunCmd("git", "remote", "get-url", "origin")
	if err != nil {
		return ProviderGitHub, false
	}

	url := strings.ToLower(strings.TrimSpace(string(output)))

	switch {
	case strings.Contains(url, "github.com"):
		return ProviderGitHub, true
	case strings.Contains(url, "gitlab.com") || strings.Contains(url, "gitlab"):
		return ProviderGitLab, true
	case strings.Contains(url, "bitbucket.org") || strings.Contains(url, "bitbucket"):
		return ProviderBitbucket, true
	default:
		return ProviderGitHub, false
	}
}

// initScanner lazily initializes the input scanner.
func (w *Wizard) initScanner() {
	if w.scanner == nil {
		w.scanner = bufio.NewScanner(w.In)
	}
}

// prompt displays a prompt and reads user input.
func (w *Wizard) prompt(msg string) (string, error) {
	fmt.Fprint(w.Out, msg)

	w.initScanner()
	if w.scanner.Scan() {
		return w.scanner.Text(), nil
	}
	if err := w.scanner.Err(); err != nil {
		return "", fmt.Errorf("init: reading input: %w", err)
	}
	// EOF means the user closed input (e.g. Ctrl+D).
	return "", ErrCancelled
}

// cleanup removes directories created during setup (for Ctrl+C handling).
func (w *Wizard) cleanup() {
	// Remove in reverse order (deepest first).
	for i := len(w.createdDirs) - 1; i >= 0; i-- {
		_ = os.RemoveAll(w.createdDirs[i])
	}
}

// relPath returns a relative path from root, or the absolute path if Rel fails.
func relPath(root, path string) string {
	rel, err := filepath.Rel(root, path)
	if err != nil {
		return path
	}
	return rel
}
