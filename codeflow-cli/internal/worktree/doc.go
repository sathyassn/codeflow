// Package worktree manages git worktree lifecycle operations for CodeFlow.
//
// It provides a [Manager] that wraps git worktree commands to create, inspect,
// list, and clean up isolated development worktrees. Worktrees are tracked in
// a YAML registry (.state/worktrees.yaml) and configured with selective
// symlinks into the main repo's .state/ directory.
//
// The Manager supports:
//   - Setup: create worktree, copy config, symlink shared state, register
//   - Status: show branch, uncommitted changes, ahead/behind remote
//   - List: read registry with optional status filter
//   - Cleanup: PathFlow guard, dry-run/force/prune modes, deregister
//
// This package is the Go equivalent of the four shell scripts:
//   - cf-worktree-setup.sh
//   - cf-worktree-status.sh
//   - cf-worktree-list.sh
//   - cf-worktree-cleanup.sh
package worktree
