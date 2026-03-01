package main

import (
	"encoding/json"
	"fmt"
	"io"
	"path/filepath"

	"github.com/codeflow/codeflow-cli/internal/claim"
	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/codeflow/codeflow-cli/internal/ledger"
	"github.com/spf13/cobra"
)

// newCoordinationCmd creates the top-level "coordination" command group.
func newCoordinationCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "coordination",
		Short: "Claim management and CRDT rebuild operations",
		Long: `Manage resource claims for agent coordination and rebuild coordination
state from the JSONL ledger.

Claims provide exclusive or shared access to file patterns with TTL-based
expiration. The coordination state is persisted at .state/coordination/state.json.`,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newCoordClaimAcquireCmd())
	cmd.AddCommand(newCoordClaimCheckCmd())
	cmd.AddCommand(newCoordClaimListCmd())
	cmd.AddCommand(newCoordClaimReleaseCmd())
	cmd.AddCommand(newCoordClaimRenewCmd())
	cmd.AddCommand(newCoordCRDTRebuildCmd())

	return cmd
}

// --- claim-acquire ---

func newCoordClaimAcquireCmd() *cobra.Command {
	var (
		stateDir  string
		ledgerDir string
		workID    string
		pattern   string
		ownerID   string
		mode      string
		ttl       int
	)

	cmd := &cobra.Command{
		Use:   "claim-acquire",
		Short: "Acquire a resource claim",
		Long: `Create a new claim for a file pattern. Returns conflict error (exit 1) if
an active exclusive claim exists for the same pattern.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runClaimAcquire(cmd.OutOrStdout(), stateDir, ledgerDir, workID, pattern, ownerID, mode, ttl)
		},
	}

	cmd.Flags().StringVar(&stateDir, "state", defaultCoordStateDir(), "base .state directory")
	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "JSONL ledger directory")
	cmd.Flags().StringVar(&workID, "work-id", "", "work ID for this claim (required)")
	cmd.Flags().StringVar(&pattern, "pattern", "", "file pattern to claim (required)")
	cmd.Flags().StringVar(&ownerID, "owner", "", "owner/agent ID (required)")
	cmd.Flags().StringVar(&mode, "mode", "exclusive", "claim mode: exclusive or shared")
	cmd.Flags().IntVar(&ttl, "ttl", claim.DefaultTTL, "TTL in seconds")
	_ = cmd.MarkFlagRequired("work-id")
	_ = cmd.MarkFlagRequired("pattern")
	_ = cmd.MarkFlagRequired("owner")

	return cmd
}

func runClaimAcquire(w io.Writer, stateDir, ledgerDir, workID, pattern, ownerID, mode string, ttl int) error {
	writer, err := ledger.NewWriter(ledgerDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("creating ledger writer: %w", err)}
	}

	mgr := claim.NewManager(stateDir, writer)
	c, err := mgr.Acquire(workID, pattern, ownerID, mode, ttl)
	if err != nil {
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("acquiring claim: %w", err)}
	}

	return writeJSON(w, c)
}

// --- claim-check ---

func newCoordClaimCheckCmd() *cobra.Command {
	var (
		stateDir       string
		value          string
		ownerID        string
		includeExpired bool
	)

	cmd := &cobra.Command{
		Use:   "claim-check",
		Short: "Check for conflicting claims",
		Long: `Check whether a file path or pattern conflicts with active claims.
Exits 1 if blocked by an exclusive claim.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runClaimCheck(cmd.OutOrStdout(), stateDir, value, ownerID, includeExpired)
		},
	}

	cmd.Flags().StringVar(&stateDir, "state", defaultCoordStateDir(), "base .state directory")
	cmd.Flags().StringVar(&value, "value", "", "file path or pattern to check (required)")
	cmd.Flags().StringVar(&ownerID, "owner", "", "owner/agent ID for self-check")
	cmd.Flags().BoolVar(&includeExpired, "include-expired", false, "include expired claims in results")
	_ = cmd.MarkFlagRequired("value")

	return cmd
}

func runClaimCheck(w io.Writer, stateDir, value, ownerID string, includeExpired bool) error {
	mgr := claim.NewManager(stateDir, nil)
	result, err := mgr.Check(value, ownerID, includeExpired)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("checking claim: %w", err)}
	}

	if err := writeJSON(w, result); err != nil {
		return err
	}

	if !result.CanProceed {
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("blocked by active claim")}
	}

	return nil
}

// --- claim-list ---

func newCoordClaimListCmd() *cobra.Command {
	var (
		stateDir       string
		ownerID        string
		workID         string
		status         string
		pattern        string
		includeExpired bool
	)

	cmd := &cobra.Command{
		Use:   "claim-list",
		Short: "List claims with optional filters",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runClaimList(cmd.OutOrStdout(), stateDir, ownerID, workID, status, pattern, includeExpired)
		},
	}

	cmd.Flags().StringVar(&stateDir, "state", defaultCoordStateDir(), "base .state directory")
	cmd.Flags().StringVar(&ownerID, "owner", "", "filter by owner ID")
	cmd.Flags().StringVar(&workID, "work-id", "", "filter by work ID")
	cmd.Flags().StringVar(&status, "status", "active", "filter by status: active, released, expired, all")
	cmd.Flags().StringVar(&pattern, "pattern", "", "filter by pattern substring")
	cmd.Flags().BoolVar(&includeExpired, "include-expired", false, "include expired claims")

	return cmd
}

func runClaimList(w io.Writer, stateDir, ownerID, workID, status, pattern string, includeExpired bool) error {
	mgr := claim.NewManager(stateDir, nil)
	claims, err := mgr.List(claim.ListOptions{
		OwnerID:        ownerID,
		WorkID:         workID,
		Status:         status,
		Pattern:        pattern,
		IncludeExpired: includeExpired,
	})
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("listing claims: %w", err)}
	}

	return writeJSON(w, claims)
}

// --- claim-release ---

func newCoordClaimReleaseCmd() *cobra.Command {
	var (
		stateDir  string
		ledgerDir string
		claimID   string
		pattern   string
		reason    string
	)

	cmd := &cobra.Command{
		Use:   "claim-release",
		Short: "Release a claim by ID or pattern",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runClaimRelease(cmd.OutOrStdout(), stateDir, ledgerDir, claimID, pattern, reason)
		},
	}

	cmd.Flags().StringVar(&stateDir, "state", defaultCoordStateDir(), "base .state directory")
	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "JSONL ledger directory")
	cmd.Flags().StringVar(&claimID, "claim-id", "", "claim ID to release")
	cmd.Flags().StringVar(&pattern, "pattern", "", "release by pattern (first active match)")
	cmd.Flags().StringVar(&reason, "reason", "", "reason for release")

	return cmd
}

func runClaimRelease(w io.Writer, stateDir, ledgerDir, claimID, pattern, reason string) error {
	writer, err := ledger.NewWriter(ledgerDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("creating ledger writer: %w", err)}
	}

	mgr := claim.NewManager(stateDir, writer)
	c, err := mgr.Release(claimID, pattern, reason)
	if err != nil {
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("releasing claim: %w", err)}
	}

	return writeJSON(w, c)
}

// --- claim-renew ---

func newCoordClaimRenewCmd() *cobra.Command {
	var (
		stateDir  string
		ledgerDir string
		claimID   string
		ttl       int
		all       bool
	)

	cmd := &cobra.Command{
		Use:   "claim-renew",
		Short: "Renew a claim's TTL or renew all active claims",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runClaimRenew(cmd.OutOrStdout(), stateDir, ledgerDir, claimID, ttl, all)
		},
	}

	cmd.Flags().StringVar(&stateDir, "state", defaultCoordStateDir(), "base .state directory")
	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "JSONL ledger directory")
	cmd.Flags().StringVar(&claimID, "claim-id", "", "claim ID to renew (required unless --all)")
	cmd.Flags().IntVar(&ttl, "ttl", claim.DefaultTTL, "new TTL in seconds")
	cmd.Flags().BoolVar(&all, "all", false, "renew all active claims")

	return cmd
}

func runClaimRenew(w io.Writer, stateDir, ledgerDir, claimID string, ttl int, all bool) error {
	writer, err := ledger.NewWriter(ledgerDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("creating ledger writer: %w", err)}
	}

	mgr := claim.NewManager(stateDir, writer)

	if all {
		results, err := mgr.RenewAll(ttl)
		if err != nil {
			return &exitError{code: ExitRuntimeError, err: fmt.Errorf("renewing all claims: %w", err)}
		}
		return writeJSON(w, map[string]any{
			"renewed": len(results),
			"results": results,
		})
	}

	if claimID == "" {
		return &exitError{code: ExitConfigError, err: fmt.Errorf("--claim-id is required when --all is not set")}
	}

	result, err := mgr.Renew(claimID, ttl)
	if err != nil {
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("renewing claim: %w", err)}
	}

	return writeJSON(w, result)
}

// --- crdt-rebuild ---

func newCoordCRDTRebuildCmd() *cobra.Command {
	var (
		source   string
		stateDir string
		until    string
		dryRun   bool
		verify   bool
	)

	cmd := &cobra.Command{
		Use:   "crdt-rebuild",
		Short: "Rebuild coordination state from JSONL ledger",
		Long: `Replay claim events from sessions.jsonl to reconstruct the coordination
document. Supports point-in-time rebuild, verification, and dry-run.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runCRDTRebuild(cmd.OutOrStdout(), source, stateDir, until, dryRun, verify)
		},
	}

	cmd.Flags().StringVar(&source, "source", defaultSessionsJSONLPath(), "path to sessions.jsonl")
	cmd.Flags().StringVar(&stateDir, "state", defaultCoordStateDir(), "base .state directory")
	cmd.Flags().StringVar(&until, "until", "", "point-in-time rebuild cutoff (ISO 8601)")
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "show rebuild results without saving")
	cmd.Flags().BoolVar(&verify, "verify", false, "compare rebuild against existing state")

	return cmd
}

func runCRDTRebuild(w io.Writer, source, stateDir, until string, dryRun, verify bool) error {
	opts := db.RebuildOptions{
		Source:   source,
		StateDir: stateDir,
		Until:    until,
		DryRun:   dryRun,
		Verify:   verify,
	}

	if verify {
		result, err := db.VerifyCRDT(opts)
		if err != nil {
			return &exitError{code: ExitRuntimeError, err: fmt.Errorf("verifying CRDT: %w", err)}
		}

		if err := writeJSON(w, result); err != nil {
			return err
		}

		if !result.VerifiedOK {
			return &exitError{code: ExitGeneralError,
				err: fmt.Errorf("verification failed: %d mismatches", len(result.Mismatches))}
		}
		return nil
	}

	result, err := db.RebuildCRDT(opts)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("rebuilding CRDT: %w", err)}
	}

	return writeJSON(w, map[string]any{
		"success":          true,
		"rebuild_type":     result.RebuildType,
		"source":           result.Source,
		"events_replayed":  result.Stats.EventsReplayed,
		"active_claims":    result.Stats.ActiveClaims,
		"released_claims":  result.Stats.ReleasedClaims,
		"max_fencing_token": result.Stats.MaxFencingToken,
		"duration_ms":      result.Stats.DurationMs,
		"dry_run":          result.DryRun,
		"saved":            result.Saved,
		"backup_path":      result.BackupPath,
		"until":            result.Until,
	})
}

// --- helpers ---

// defaultCoordStateDir returns the default .state directory path.
func defaultCoordStateDir() string {
	return ".state"
}

// defaultSessionsJSONLPath returns the default sessions.jsonl path.
func defaultSessionsJSONLPath() string {
	return filepath.Join(".state", "ledger", "sessions.jsonl")
}

// writeJSON encodes a value as indented JSON to the writer.
func writeJSON(w io.Writer, v any) error {
	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	if err := enc.Encode(v); err != nil {
		return &exitError{code: ExitInternalError, err: fmt.Errorf("encoding JSON: %w", err)}
	}
	return nil
}

