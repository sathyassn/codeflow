package worktree

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// newExecCmd creates an *exec.Cmd. Replaceable in tests.
var newExecCmd = exec.Command

// Sentinel errors.
var (
	// ErrNotFound indicates the named worktree does not exist.
	ErrNotFound = errors.New("worktree: not found")

	// ErrAlreadyExists indicates a worktree with the given name already exists.
	ErrAlreadyExists = errors.New("worktree: already exists")

	// ErrPathFlowActive indicates cleanup is blocked by an active PathFlow session.
	ErrPathFlowActive = errors.New("worktree: PathFlow session active, use force to override")

	// ErrInvalidName indicates the worktree name is empty or invalid.
	ErrInvalidName = errors.New("worktree: invalid name")
)

// Worktree represents a tracked git worktree.
type Worktree struct {
	Name      string    `yaml:"name"`
	Path      string    `yaml:"path"`
	Branch    string    `yaml:"branch"`
	CreatedAt time.Time `yaml:"created_at"`
	Status    string    `yaml:"status"`
}

// WorktreeStatus holds the health/sync status of a single worktree.
type WorktreeStatus struct {
	Path               string `json:"path"`
	Branch             string `json:"branch"`
	Status             string `json:"status"`
	UncommittedChanges int    `json:"uncommitted_changes"`
	Ahead              int    `json:"ahead"`
	Behind             int    `json:"behind"`
	HasUpstream        bool   `json:"has_upstream"`
	LastCommit         string `json:"last_commit"`
	WorkItem           string `json:"work_item,omitempty"`
}

// CleanupOpts configures worktree cleanup behavior.
type CleanupOpts struct {
	Force  bool
	DryRun bool
	Prune  bool
}

// Manager manages worktree operations.
type Manager struct {
	// ProjectDir is the main repository root.
	ProjectDir string

	// WorktreeBaseDir is where worktrees are created (default: .claude/worktrees/).
	WorktreeBaseDir string

	// RegistryPath is the path to worktrees.yaml (default: .state/worktrees.yaml).
	RegistryPath string

	// PathFlowChecker returns true if a PathFlow session is active.
	// If nil, PathFlow checks are skipped.
	PathFlowChecker func() bool

	// Out is the writer for status messages. If nil, os.Stdout is used.
	Out io.Writer
}

// output returns the configured writer or stdout.
func (m *Manager) output() io.Writer {
	if m.Out != nil {
		return m.Out
	}
	return os.Stdout
}

// registryPath returns the resolved registry file path.
func (m *Manager) registryPath() string {
	if m.RegistryPath != "" {
		return m.RegistryPath
	}
	return filepath.Join(m.ProjectDir, ".state", "worktrees.yaml")
}

// worktreeBaseDir returns the resolved worktree base directory.
func (m *Manager) worktreeBaseDir() string {
	if m.WorktreeBaseDir != "" {
		return m.WorktreeBaseDir
	}
	return filepath.Join(m.ProjectDir, ".claude", "worktrees")
}

// Setup creates a new git worktree and configures it.
//
// It creates the worktree via "git worktree add", copies essential config
// files, creates selective .state/ symlinks, and registers the worktree
// in worktrees.yaml.
func (m *Manager) Setup(name, branch string) (*Worktree, error) {
	if name == "" {
		return nil, ErrInvalidName
	}
	if branch == "" {
		return nil, fmt.Errorf("worktree: branch name required")
	}

	wtPath := filepath.Join(m.worktreeBaseDir(), name)

	// Check if already exists.
	if _, err := os.Stat(wtPath); err == nil {
		return nil, fmt.Errorf("%w: %s", ErrAlreadyExists, name)
	}

	// Create worktree via git.
	cmd := newExecCmd("git", "-C", m.ProjectDir, "worktree", "add", wtPath, "-b", branch)
	if out, err := cmd.CombinedOutput(); err != nil {
		return nil, fmt.Errorf("git worktree add: %w: %s", err, string(out))
	}

	w := m.output()

	// Copy essential config files.
	fmt.Fprintln(w, "Copying essential config files...")
	for _, name := range []string{".gitignore"} {
		src := filepath.Join(m.ProjectDir, name)
		dst := filepath.Join(wtPath, name)
		if data, err := os.ReadFile(src); err == nil {
			if writeErr := os.WriteFile(dst, data, 0o644); writeErr != nil {
				fmt.Fprintf(w, "  warning: failed to copy %s: %v\n", name, writeErr)
			}
		}
	}

	// Set up selective .state symlinks.
	fmt.Fprintln(w, "Setting up .state with selective symlinks...")
	stateDir := filepath.Join(wtPath, ".state")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		return nil, fmt.Errorf("creating .state dir: %w", err)
	}

	// Shared state directories (symlinked to main repo).
	sharedDirs := []string{"db", "ledger", "registry", "backups", "coordination", "logs"}
	for _, dir := range sharedDirs {
		src := filepath.Join(m.ProjectDir, ".state", dir)
		dst := filepath.Join(stateDir, dir)
		if _, err := os.Stat(src); err == nil {
			if _, err := os.Lstat(dst); err != nil {
				if err := os.Symlink(src, dst); err == nil {
					fmt.Fprintf(w, "  Symlinked: .state/%s\n", dir)
				}
			}
		}
	}

	// Local state directories (per-worktree, not symlinked).
	localDirs := []string{"runtime", "session", "sentinels"}
	for _, dir := range localDirs {
		dst := filepath.Join(stateDir, dir)
		if err := os.MkdirAll(dst, 0o755); err == nil {
			fmt.Fprintf(w, "  Created local: .state/%s\n", dir)
		}
	}

	// Register in worktrees.yaml.
	now := time.Now().UTC()
	wt := &Worktree{
		Name:      name,
		Path:      wtPath,
		Branch:    branch,
		CreatedAt: now,
		Status:    "active",
	}

	if err := m.registerWorktree(wt); err != nil {
		return nil, fmt.Errorf("registering worktree: %w", err)
	}

	fmt.Fprintf(w, "Worktree setup complete: %s (%s)\n", wtPath, branch)
	return wt, nil
}

// Status returns the health/sync status of a worktree.
func (m *Manager) Status(name string) (*WorktreeStatus, error) {
	if name == "" {
		return nil, ErrInvalidName
	}

	wtPath := filepath.Join(m.worktreeBaseDir(), name)
	if _, err := os.Stat(wtPath); err != nil {
		return nil, fmt.Errorf("%w: %s", ErrNotFound, name)
	}

	return m.checkStatus(wtPath)
}

// checkStatus gathers git status for a worktree path.
func (m *Manager) checkStatus(wtPath string) (*WorktreeStatus, error) {
	st := &WorktreeStatus{
		Path:   wtPath,
		Status: "clean",
	}

	// Get branch name.
	cmd := newExecCmd("git", "-C", wtPath, "branch", "--show-current")
	if out, err := cmd.Output(); err == nil {
		st.Branch = strings.TrimSpace(string(out))
	} else {
		st.Branch = "detached"
	}

	// Count uncommitted changes.
	cmd = newExecCmd("git", "-C", wtPath, "status", "--porcelain")
	if out, err := cmd.Output(); err == nil {
		lines := strings.Split(strings.TrimSpace(string(out)), "\n")
		if len(lines) == 1 && lines[0] == "" {
			st.UncommittedChanges = 0
		} else {
			st.UncommittedChanges = len(lines)
		}
	}

	// Check upstream tracking.
	cmd = newExecCmd("git", "-C", wtPath, "rev-parse", "--abbrev-ref", "@{u}")
	if _, err := cmd.Output(); err == nil {
		st.HasUpstream = true

		// Ahead count.
		cmd = newExecCmd("git", "-C", wtPath, "rev-list", "--count", "@{u}..HEAD")
		if out, err := cmd.Output(); err == nil {
			fmt.Sscanf(strings.TrimSpace(string(out)), "%d", &st.Ahead)
		}

		// Behind count.
		cmd = newExecCmd("git", "-C", wtPath, "rev-list", "--count", "HEAD..@{u}")
		if out, err := cmd.Output(); err == nil {
			fmt.Sscanf(strings.TrimSpace(string(out)), "%d", &st.Behind)
		}
	}

	// Last commit time.
	cmd = newExecCmd("git", "-C", wtPath, "log", "-1", "--format=%cr")
	if out, err := cmd.Output(); err == nil {
		st.LastCommit = strings.TrimSpace(string(out))
	} else {
		st.LastCommit = "unknown"
	}

	if st.UncommittedChanges > 0 || st.Ahead > 0 || st.Behind > 0 {
		st.Status = "dirty"
	}

	// Lookup work item from registry.
	st.WorkItem = m.lookupWorkItem(wtPath)

	return st, nil
}

// List returns tracked worktrees from the registry, optionally filtered by status.
func (m *Manager) List(filter string) ([]*Worktree, error) {
	regPath := m.registryPath()
	if _, err := os.Stat(regPath); err != nil {
		return nil, nil // No registry = no worktrees.
	}

	return m.readRegistry(filter)
}

// Cleanup removes a worktree and deregisters it.
func (m *Manager) Cleanup(name string, opts CleanupOpts) error {
	w := m.output()

	// Prune mode: just run git worktree prune.
	if opts.Prune {
		fmt.Fprintln(w, "=== Worktree Prune ===")
		if opts.DryRun {
			cmd := newExecCmd("git", "-C", m.ProjectDir, "worktree", "prune", "--dry-run")
			out, _ := cmd.CombinedOutput()
			fmt.Fprintf(w, "  Mode: DRY RUN\n%s", string(out))
		} else {
			cmd := newExecCmd("git", "-C", m.ProjectDir, "worktree", "prune")
			if out, err := cmd.CombinedOutput(); err != nil {
				return fmt.Errorf("git worktree prune: %w: %s", err, string(out))
			}
			fmt.Fprintln(w, "  Pruned stale worktree references")
		}
		return nil
	}

	if name == "" {
		return ErrInvalidName
	}

	// PathFlow guard.
	if m.PathFlowChecker != nil && m.PathFlowChecker() && !opts.Force {
		return ErrPathFlowActive
	}

	wtPath := filepath.Join(m.worktreeBaseDir(), name)

	if opts.DryRun {
		fmt.Fprintf(w, "[DRY-RUN] Would remove worktree: %s\n", wtPath)
		return nil
	}

	// Try normal removal first, fall back to force.
	cmd := newExecCmd("git", "-C", m.ProjectDir, "worktree", "remove", wtPath)
	if _, err := cmd.CombinedOutput(); err != nil {
		if opts.Force {
			cmd = newExecCmd("git", "-C", m.ProjectDir, "worktree", "remove", "--force", wtPath)
			if out, err := cmd.CombinedOutput(); err != nil {
				// Try removing directory directly.
				if rmErr := os.RemoveAll(wtPath); rmErr != nil {
					return fmt.Errorf("git worktree remove --force: %w: %s", err, string(out))
				}
			}
		} else {
			// Check if directory exists; if not, it was already removed.
			if _, statErr := os.Stat(wtPath); statErr != nil {
				// Already gone, just deregister.
			} else {
				return fmt.Errorf("%w: %s", ErrNotFound, name)
			}
		}
	}

	// Remove directory if still present.
	if _, err := os.Stat(wtPath); err == nil {
		if err := os.RemoveAll(wtPath); err != nil {
			return fmt.Errorf("removing directory: %w", err)
		}
	}

	// Deregister from worktrees.yaml.
	if err := m.deregisterWorktree(wtPath); err != nil {
		fmt.Fprintf(w, "Warning: failed to deregister: %v\n", err)
	}

	fmt.Fprintf(w, "Cleaned up worktree: %s\n", name)
	return nil
}

// registerWorktree adds a worktree entry to the YAML registry.
func (m *Manager) registerWorktree(wt *Worktree) error {
	regPath := m.registryPath()
	if err := os.MkdirAll(filepath.Dir(regPath), 0o755); err != nil {
		return err
	}

	// Create file if it doesn't exist.
	if _, err := os.Stat(regPath); err != nil {
		header := fmt.Sprintf(`# Worktree Tracking
# Managed by: codeflow worktree

worktrees: []

metadata:
  version: "1.0.0"
  last_updated: "%s"
`, wt.CreatedAt.Format(time.RFC3339))
		if err := os.WriteFile(regPath, []byte(header), 0o644); err != nil {
			return err
		}
	}

	// Read current content.
	data, err := os.ReadFile(regPath)
	if err != nil {
		return err
	}

	content := string(data)

	// Build new entry.
	entry := fmt.Sprintf(`  - path: "%s"
    branch: "%s"
    created_at: "%s"
    name: "%s"
    status: active
`,
		wt.Path, wt.Branch, wt.CreatedAt.Format(time.RFC3339), wt.Name)

	// Insert after "worktrees:" line.
	content = strings.Replace(content, "worktrees: []", "worktrees:\n"+entry, 1)
	if !strings.Contains(content, entry) {
		// worktrees: already has entries, append after "worktrees:" line.
		content = strings.Replace(content, "worktrees:\n", "worktrees:\n"+entry, 1)
	}

	// Update last_updated.
	now := time.Now().UTC().Format(time.RFC3339)
	lines := strings.Split(content, "\n")
	for i, line := range lines {
		if strings.Contains(line, "last_updated:") {
			lines[i] = fmt.Sprintf(`  last_updated: "%s"`, now)
		}
	}
	content = strings.Join(lines, "\n")

	return os.WriteFile(regPath, []byte(content), 0o644)
}

// deregisterWorktree updates the worktree status to "removed" in the registry.
func (m *Manager) deregisterWorktree(wtPath string) error {
	regPath := m.registryPath()
	data, err := os.ReadFile(regPath)
	if err != nil {
		return nil // No registry, nothing to do.
	}

	// Simple line-by-line replacement: find the path entry and change status.
	lines := strings.Split(string(data), "\n")
	inTarget := false
	for i, line := range lines {
		if strings.Contains(line, fmt.Sprintf(`path: "%s"`, wtPath)) {
			inTarget = true
		}
		if inTarget && strings.Contains(line, "status:") {
			lines[i] = strings.Replace(line, "active", "removed", 1)
			inTarget = false
		}
		// Reset if we hit a new entry.
		if inTarget && strings.HasPrefix(strings.TrimSpace(line), "- path:") && !strings.Contains(line, wtPath) {
			inTarget = false
		}
	}

	return os.WriteFile(regPath, []byte(strings.Join(lines, "\n")), 0o644)
}

// readRegistry reads worktrees from the YAML registry file.
// Uses a simple line parser rather than full YAML to match the shell script behavior.
func (m *Manager) readRegistry(filter string) ([]*Worktree, error) {
	regPath := m.registryPath()
	f, err := os.Open(regPath)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	var worktrees []*Worktree
	var current *Worktree

	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := scanner.Text()
		trimmed := strings.TrimSpace(line)

		if strings.HasPrefix(trimmed, "- path:") {
			if current != nil && (filter == "" || current.Status == filter) {
				worktrees = append(worktrees, current)
			}
			current = &Worktree{}
			current.Path = extractYAMLValue(trimmed, "path")
		} else if current != nil {
			if strings.HasPrefix(trimmed, "branch:") {
				current.Branch = extractYAMLValue(trimmed, "branch")
			} else if strings.HasPrefix(trimmed, "name:") {
				current.Name = extractYAMLValue(trimmed, "name")
			} else if strings.HasPrefix(trimmed, "status:") {
				current.Status = extractYAMLValue(trimmed, "status")
			} else if strings.HasPrefix(trimmed, "created_at:") {
				ts := extractYAMLValue(trimmed, "created_at")
				if t, err := time.Parse(time.RFC3339, ts); err == nil {
					current.CreatedAt = t
				}
			}
		}
	}

	// Don't forget the last entry.
	if current != nil && (filter == "" || current.Status == filter) {
		worktrees = append(worktrees, current)
	}

	return worktrees, scanner.Err()
}

// lookupWorkItem finds the purpose/work item for a worktree path from the registry.
func (m *Manager) lookupWorkItem(wtPath string) string {
	regPath := m.registryPath()
	f, err := os.Open(regPath)
	if err != nil {
		return ""
	}
	defer f.Close()

	inTarget := false
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := scanner.Text()
		trimmed := strings.TrimSpace(line)

		if strings.Contains(trimmed, fmt.Sprintf(`path: "%s"`, wtPath)) {
			inTarget = true
			continue
		}
		if inTarget && strings.HasPrefix(trimmed, "- path:") {
			break // Hit next entry.
		}
		if inTarget && strings.HasPrefix(trimmed, "purpose:") {
			return extractYAMLValue(trimmed, "purpose")
		}
	}
	return ""
}

// extractYAMLValue extracts a simple YAML value, stripping quotes.
func extractYAMLValue(line, key string) string {
	idx := strings.Index(line, key+":")
	if idx < 0 {
		return ""
	}
	val := strings.TrimSpace(line[idx+len(key)+1:])
	val = strings.Trim(val, `"'`)
	return val
}
