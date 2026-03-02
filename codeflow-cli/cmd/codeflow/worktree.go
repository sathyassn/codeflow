package main

import (
	"encoding/json"
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/worktree"
	"github.com/spf13/cobra"
)

// newWorktreeCmd creates the top-level "worktree" command with subcommands.
func newWorktreeCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "worktree",
		Short: "Manage git worktrees for parallel development",
		Long:  "Create, inspect, list, and clean up isolated git worktrees for parallel development workflows.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newWorktreeSetupCmd())
	cmd.AddCommand(newWorktreeStatusCmd())
	cmd.AddCommand(newWorktreeListCmd())
	cmd.AddCommand(newWorktreeCleanupCmd())

	return cmd
}

// newWorktreeSetupCmd creates the "worktree setup" subcommand.
func newWorktreeSetupCmd() *cobra.Command {
	var (
		name   string
		branch string
	)

	cmd := &cobra.Command{
		Use:   "setup",
		Short: "Create a new git worktree",
		Long:  "Create an isolated git worktree, copy config files, create selective .state symlinks, and register in worktrees.yaml.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runWorktreeSetup(cmd.OutOrStdout(), name, branch)
		},
	}

	cmd.Flags().StringVar(&name, "name", "", "worktree name (required)")
	cmd.Flags().StringVar(&branch, "branch", "", "branch name to create (required)")
	_ = cmd.MarkFlagRequired("name")
	_ = cmd.MarkFlagRequired("branch")

	return cmd
}

// runWorktreeSetup implements the worktree setup logic.
func runWorktreeSetup(w io.Writer, name, branch string) error {
	projectDir := detectProjectDir()

	mgr := &worktree.Manager{
		ProjectDir: projectDir,
		Out:        w,
	}

	wt, err := mgr.Setup(name, branch)
	if err != nil {
		return fmt.Errorf("setting up worktree: %w", err)
	}

	fmt.Fprintf(w, "Created worktree %q at %s on branch %s\n", wt.Name, wt.Path, wt.Branch)
	return nil
}

// newWorktreeStatusCmd creates the "worktree status" subcommand.
func newWorktreeStatusCmd() *cobra.Command {
	var (
		name       string
		jsonOutput bool
	)

	cmd := &cobra.Command{
		Use:   "status",
		Short: "Show worktree health and sync status",
		Long:  "Display branch, uncommitted changes, ahead/behind remote, and work item association for a worktree.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runWorktreeStatus(cmd.OutOrStdout(), name, jsonOutput)
		},
	}

	cmd.Flags().StringVar(&name, "name", "", "worktree name (required)")
	cmd.Flags().BoolVar(&jsonOutput, "json", false, "output in JSON format")
	_ = cmd.MarkFlagRequired("name")

	return cmd
}

// runWorktreeStatus implements the worktree status logic.
func runWorktreeStatus(w io.Writer, name string, jsonOutput bool) error {
	projectDir := detectProjectDir()

	mgr := &worktree.Manager{
		ProjectDir: projectDir,
		Out:        w,
	}

	status, err := mgr.Status(name)
	if err != nil {
		return fmt.Errorf("checking worktree status: %w", err)
	}

	if jsonOutput {
		enc := json.NewEncoder(w)
		enc.SetIndent("", "  ")
		return enc.Encode(status)
	}

	fmt.Fprintf(w, "Worktree: %s\n", status.Path)
	fmt.Fprintf(w, "  Branch: %s\n", status.Branch)
	fmt.Fprintf(w, "  Status: %s\n", status.Status)
	if status.UncommittedChanges > 0 {
		fmt.Fprintf(w, "  Uncommitted changes: %d files\n", status.UncommittedChanges)
	}
	if status.HasUpstream {
		if status.Ahead > 0 {
			fmt.Fprintf(w, "  Unpushed: %d commits ahead of remote\n", status.Ahead)
		}
		if status.Behind > 0 {
			fmt.Fprintf(w, "  Behind: %d commits behind remote\n", status.Behind)
		}
		if status.Ahead == 0 && status.Behind == 0 {
			fmt.Fprintln(w, "  Remote: synced")
		}
	} else {
		fmt.Fprintln(w, "  Remote: no upstream tracking branch")
	}
	fmt.Fprintf(w, "  Last commit: %s\n", status.LastCommit)

	return nil
}

// newWorktreeListCmd creates the "worktree list" subcommand.
func newWorktreeListCmd() *cobra.Command {
	var status string

	cmd := &cobra.Command{
		Use:   "list",
		Short: "List tracked worktrees",
		Long:  "Read worktrees.yaml and display all tracked worktrees, optionally filtered by status.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runWorktreeList(cmd.OutOrStdout(), status)
		},
	}

	cmd.Flags().StringVar(&status, "status", "", "filter by status (active, stale, merged, abandoned, removed)")

	return cmd
}

// runWorktreeList implements the worktree list logic.
func runWorktreeList(w io.Writer, filter string) error {
	projectDir := detectProjectDir()

	mgr := &worktree.Manager{
		ProjectDir: projectDir,
		Out:        w,
	}

	worktrees, err := mgr.List(filter)
	if err != nil {
		return fmt.Errorf("listing worktrees: %w", err)
	}

	if len(worktrees) == 0 {
		fmt.Fprintln(w, "No tracked worktrees")
		return nil
	}

	fmt.Fprintln(w, "=== Tracked Worktrees ===")
	fmt.Fprintln(w)
	fmt.Fprintf(w, "%-40s %-25s %-10s %s\n", "PATH", "BRANCH", "STATUS", "CREATED")
	fmt.Fprintf(w, "%-40s %-25s %-10s %s\n", "----", "------", "------", "-------")

	for _, wt := range worktrees {
		created := ""
		if !wt.CreatedAt.IsZero() {
			created = wt.CreatedAt.Format("2006-01-02")
		}
		fmt.Fprintf(w, "%-40s %-25s %-10s %s\n", wt.Path, wt.Branch, wt.Status, created)
	}

	return nil
}

// newWorktreeCleanupCmd creates the "worktree cleanup" subcommand.
func newWorktreeCleanupCmd() *cobra.Command {
	var (
		name   string
		force  bool
		dryRun bool
		prune  bool
	)

	cmd := &cobra.Command{
		Use:   "cleanup",
		Short: "Remove a worktree",
		Long:  "Remove a worktree with PathFlow guard, dry-run/force/prune support, and registry deregistration.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runWorktreeCleanup(cmd.OutOrStdout(), name, force, dryRun, prune)
		},
	}

	cmd.Flags().StringVar(&name, "name", "", "worktree name to remove")
	cmd.Flags().BoolVar(&force, "force", false, "skip safety checks")
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "show what would be done without doing it")
	cmd.Flags().BoolVar(&prune, "prune", false, "prune stale git worktree references")

	return cmd
}

// runWorktreeCleanup implements the worktree cleanup logic.
func runWorktreeCleanup(w io.Writer, name string, force, dryRun, prune bool) error {
	projectDir := detectProjectDir()

	mgr := &worktree.Manager{
		ProjectDir: projectDir,
		Out:        w,
	}

	opts := worktree.CleanupOpts{
		Force:  force,
		DryRun: dryRun,
		Prune:  prune,
	}

	if !prune && name == "" {
		return fmt.Errorf("--name is required (or use --prune)")
	}

	if err := mgr.Cleanup(name, opts); err != nil {
		return fmt.Errorf("cleaning up worktree: %w", err)
	}

	return nil
}
