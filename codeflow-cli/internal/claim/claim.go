package claim

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/codeflow/codeflow-cli/internal/idgen"
	"github.com/codeflow/codeflow-cli/internal/ledger"
)

// Sentinel errors for claim operations.
var (
	ErrConflict       = errors.New("claim: conflict")
	ErrNotFound       = errors.New("claim: not found")
	ErrAlreadyReleased = errors.New("claim: already released")
	ErrExpired        = errors.New("claim: expired")
	ErrNotActive      = errors.New("claim: not active")
)

// DefaultTTL is the default claim time-to-live in seconds.
const DefaultTTL = 600

// Claim represents a resource claim in the coordination state.
type Claim struct {
	ID           string `json:"id"`
	WorkID       string `json:"work_id"`
	Pattern      string `json:"pattern"`
	Mode         string `json:"mode"`
	OwnerID      string `json:"owner_id"`
	FencingToken int64  `json:"fencing_token"`
	ExpiresAt    string `json:"expires_at"`
	Status       string `json:"status"`
	CreatedAt    string `json:"created_at"`
	ReleasedAt   string `json:"released_at,omitempty"`
}

// CoordinationDoc holds the full coordination state with claims and a
// monotonic fencing token counter.
type CoordinationDoc struct {
	Claims       map[string]*Claim `json:"claims"`
	TokenCounter int64             `json:"token_counter"`
}

// newCoordinationDoc creates an empty coordination document.
func newCoordinationDoc() *CoordinationDoc {
	return &CoordinationDoc{
		Claims: make(map[string]*Claim),
	}
}

// nextFencingToken returns the next monotonic fencing token.
func (d *CoordinationDoc) nextFencingToken() int64 {
	d.TokenCounter++
	return d.TokenCounter
}

// Manager manages claim lifecycle operations. It reads and writes the
// coordination state file and appends events to the JSONL ledger.
type Manager struct {
	// StateDir is the base .state directory (contains coordination/state.json).
	StateDir string

	// LedgerWriter appends claim events to the JSONL ledger.
	LedgerWriter *ledger.Writer

	// Now returns the current UTC time. Override in tests.
	Now func() time.Time
}

// NewManager creates a Manager for the given state and ledger directories.
func NewManager(stateDir string, ledgerWriter *ledger.Writer) *Manager {
	return &Manager{
		StateDir:     stateDir,
		LedgerWriter: ledgerWriter,
		Now:          func() time.Time { return time.Now().UTC() },
	}
}

// stateFilePath returns the path to the coordination state JSON file.
func (m *Manager) stateFilePath() string {
	return filepath.Join(m.StateDir, "coordination", "state.json")
}

// loadDoc reads the coordination document from disk.
// Returns an empty doc if the file does not exist.
func (m *Manager) loadDoc() (*CoordinationDoc, error) {
	data, err := os.ReadFile(m.stateFilePath())
	if err != nil {
		if os.IsNotExist(err) {
			return newCoordinationDoc(), nil
		}
		return nil, fmt.Errorf("claim: reading state: %w", err)
	}

	var doc CoordinationDoc
	if err := json.Unmarshal(data, &doc); err != nil {
		return nil, fmt.Errorf("claim: parsing state: %w", err)
	}
	if doc.Claims == nil {
		doc.Claims = make(map[string]*Claim)
	}
	return &doc, nil
}

// saveDoc writes the coordination document atomically (tmp file + rename).
func (m *Manager) saveDoc(doc *CoordinationDoc) error {
	dir := filepath.Dir(m.stateFilePath())
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("claim: creating state dir: %w", err)
	}

	data, err := json.MarshalIndent(doc, "", "  ")
	if err != nil {
		return fmt.Errorf("claim: marshaling state: %w", err)
	}

	tmp := m.stateFilePath() + ".tmp"
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		return fmt.Errorf("claim: writing temp file: %w", err)
	}

	if err := os.Rename(tmp, m.stateFilePath()); err != nil {
		_ = os.Remove(tmp)
		return fmt.Errorf("claim: renaming state file: %w", err)
	}

	return nil
}

// formatUTC formats a time as ISO 8601 with Z suffix.
func formatUTC(t time.Time) string {
	return t.UTC().Format("2006-01-02T15:04:05Z")
}

// parseExpiry parses an ISO 8601 timestamp with Z or +00:00 suffix.
func parseExpiry(s string) (time.Time, error) {
	s = strings.Replace(s, "Z", "+00:00", 1)
	return time.Parse("2006-01-02T15:04:05+00:00", s)
}

// Acquire creates a new claim for the given pattern. Returns ErrConflict if
// an active, non-expired exclusive claim exists for the same pattern.
func (m *Manager) Acquire(workID, pattern, ownerID, mode string, ttlSeconds int) (*Claim, error) {
	doc, err := m.loadDoc()
	if err != nil {
		return nil, err
	}

	now := m.Now()

	// Check for conflicts against active claims.
	for _, c := range doc.Claims {
		if c.Status != "active" {
			continue
		}
		if !patternsConflict(pattern, c.Pattern) {
			continue
		}
		// Check if claim is expired.
		if c.ExpiresAt != "" {
			exp, err := parseExpiry(c.ExpiresAt)
			if err == nil && !exp.After(now) {
				continue // expired
			}
		}
		// Active conflict: block if either side is exclusive.
		if c.Mode == "exclusive" || mode == "exclusive" {
			return nil, fmt.Errorf("%w: resource claimed by %s", ErrConflict, c.OwnerID)
		}
	}

	claimID := "claim_" + idgen.NewULID()
	token := doc.nextFencingToken()
	expiresAt := formatUTC(now.Add(time.Duration(ttlSeconds) * time.Second))
	createdAt := formatUTC(now)

	claim := &Claim{
		ID:           claimID,
		WorkID:       workID,
		Pattern:      pattern,
		Mode:         mode,
		OwnerID:      ownerID,
		FencingToken: token,
		ExpiresAt:    expiresAt,
		Status:       "active",
		CreatedAt:    createdAt,
	}

	doc.Claims[claimID] = claim

	if err := m.saveDoc(doc); err != nil {
		return nil, err
	}

	// Append ledger event.
	if m.LedgerWriter != nil {
		evt := ledger.Event{
			EventType: "claim_created",
			Data: map[string]any{
				"id":            claimID,
				"work_id":       workID,
				"pattern":       pattern,
				"mode":          mode,
				"owner_id":      ownerID,
				"fencing_token": token,
				"expires_at":    expiresAt,
				"created_at":    createdAt,
			},
		}
		if err := m.LedgerWriter.AppendEvent(evt); err != nil {
			return nil, fmt.Errorf("claim: appending ledger event: %w", err)
		}
	}

	return claim, nil
}

// CheckResult contains the result of a claim check.
type CheckResult struct {
	Claimed    bool           `json:"claimed"`
	Claims     []CheckedClaim `json:"claims"`
	CanProceed bool           `json:"can_proceed"`
}

// CheckedClaim is a single matching claim with conflict classification.
type CheckedClaim struct {
	ClaimID   string `json:"claim_id"`
	Pattern   string `json:"pattern"`
	Mode      string `json:"mode"`
	OwnerID   string `json:"owner_id"`
	IsOwn     bool   `json:"is_own"`
	ExpiresAt string `json:"expires_at"`
	Action    string `json:"action"` // ALLOW, WARN, BLOCK
}

// Check checks whether a file path or pattern conflicts with active claims.
func (m *Manager) Check(checkValue, ownerID string, includeExpired bool) (*CheckResult, error) {
	doc, err := m.loadDoc()
	if err != nil {
		return nil, err
	}

	now := m.Now()
	result := &CheckResult{CanProceed: true}

	for claimID, c := range doc.Claims {
		if c.Status != "active" {
			continue
		}

		isExpired := false
		if c.ExpiresAt != "" {
			exp, err := parseExpiry(c.ExpiresAt)
			if err == nil && !exp.After(now) {
				isExpired = true
				if !includeExpired {
					continue
				}
			}
		}

		if !patternsConflict(checkValue, c.Pattern) {
			continue
		}

		isOwn := ownerID != "" && c.OwnerID == ownerID
		action := classifyConflict(c, ownerID, isExpired)

		if action == "BLOCK" {
			result.CanProceed = false
		}

		result.Claims = append(result.Claims, CheckedClaim{
			ClaimID:   claimID,
			Pattern:   c.Pattern,
			Mode:      c.Mode,
			OwnerID:   c.OwnerID,
			IsOwn:     isOwn,
			ExpiresAt: c.ExpiresAt,
			Action:    action,
		})
	}

	result.Claimed = len(result.Claims) > 0
	return result, nil
}

// ListOptions configures filtering for the List operation.
type ListOptions struct {
	OwnerID        string
	WorkID         string
	Status         string // "active", "released", "expired", "all"
	Pattern        string
	IncludeExpired bool
}

// ListedClaim extends Claim with computed fields for listing.
type ListedClaim struct {
	Claim
	EffectiveStatus      string `json:"effective_status"`
	TimeRemainingSeconds *int   `json:"time_remaining_seconds,omitempty"`
}

// List returns claims matching the given filter options.
func (m *Manager) List(opts ListOptions) ([]ListedClaim, error) {
	doc, err := m.loadDoc()
	if err != nil {
		return nil, err
	}

	now := m.Now()
	statusFilter := opts.Status
	if statusFilter == "" {
		statusFilter = "active"
	}
	if opts.IncludeExpired && statusFilter == "active" {
		statusFilter = "all"
	}

	var claims []ListedClaim

	for _, c := range doc.Claims {
		effectiveStatus := c.Status
		if effectiveStatus == "active" && c.ExpiresAt != "" {
			exp, err := parseExpiry(c.ExpiresAt)
			if err == nil && !exp.After(now) {
				effectiveStatus = "expired"
			}
		}

		// Apply filters.
		if statusFilter != "all" && effectiveStatus != statusFilter {
			continue
		}
		if opts.OwnerID != "" && c.OwnerID != opts.OwnerID {
			continue
		}
		if opts.WorkID != "" && c.WorkID != opts.WorkID {
			continue
		}
		if opts.Pattern != "" && !strings.Contains(c.Pattern, opts.Pattern) {
			continue
		}

		lc := ListedClaim{
			Claim:           *c,
			EffectiveStatus: effectiveStatus,
		}

		if effectiveStatus == "active" && c.ExpiresAt != "" {
			exp, err := parseExpiry(c.ExpiresAt)
			if err == nil {
				remaining := int(exp.Sub(now).Seconds())
				lc.TimeRemainingSeconds = &remaining
			}
		}

		claims = append(claims, lc)
	}

	return claims, nil
}

// Release sets a claim's status to released. Finds by claim ID or by pattern
// (first active match). Returns ErrNotFound if no matching claim exists.
func (m *Manager) Release(claimID, pattern, reason string) (*Claim, error) {
	doc, err := m.loadDoc()
	if err != nil {
		return nil, err
	}

	var foundID string
	var claim *Claim

	if claimID != "" {
		c, ok := doc.Claims[claimID]
		if !ok {
			return nil, fmt.Errorf("%w: %s", ErrNotFound, claimID)
		}
		foundID = claimID
		claim = c
	} else if pattern != "" {
		for id, c := range doc.Claims {
			if c.Pattern == pattern && c.Status == "active" {
				foundID = id
				claim = c
				break
			}
		}
		if claim == nil {
			return nil, fmt.Errorf("%w: %s", ErrNotFound, pattern)
		}
	} else {
		return nil, fmt.Errorf("claim: either claim-id or pattern required")
	}

	if claim.Status == "released" {
		return nil, fmt.Errorf("%w: at %s", ErrAlreadyReleased, claim.ReleasedAt)
	}

	now := m.Now()
	releasedAt := formatUTC(now)
	claim.Status = "released"
	claim.ReleasedAt = releasedAt

	if err := m.saveDoc(doc); err != nil {
		return nil, err
	}

	// Append ledger event.
	if m.LedgerWriter != nil {
		data := map[string]any{
			"claim_id":    foundID,
			"released_at": releasedAt,
		}
		if reason != "" {
			data["reason"] = reason
		}
		evt := ledger.Event{
			EventType: "claim_released",
			Data:      data,
		}
		if err := m.LedgerWriter.AppendEvent(evt); err != nil {
			return nil, fmt.Errorf("claim: appending ledger event: %w", err)
		}
	}

	return claim, nil
}

// RenewResult holds the outcome of a single claim renewal.
type RenewResult struct {
	ClaimID           string `json:"claim_id"`
	Pattern           string `json:"pattern"`
	PreviousExpiresAt string `json:"previous_expires_at"`
	NewExpiresAt      string `json:"new_expires_at"`
}

// Renew extends a claim's expiration. Returns ErrNotFound, ErrNotActive, or
// ErrExpired as appropriate.
func (m *Manager) Renew(claimID string, ttlSeconds int) (*RenewResult, error) {
	doc, err := m.loadDoc()
	if err != nil {
		return nil, err
	}

	c, ok := doc.Claims[claimID]
	if !ok {
		return nil, fmt.Errorf("%w: %s", ErrNotFound, claimID)
	}

	if c.Status != "active" {
		return nil, fmt.Errorf("%w: status is %s", ErrNotActive, c.Status)
	}

	now := m.Now()
	if c.ExpiresAt != "" {
		exp, err := parseExpiry(c.ExpiresAt)
		if err == nil && !exp.After(now) {
			return nil, fmt.Errorf("%w: expired at %s", ErrExpired, c.ExpiresAt)
		}
	}

	previousExpires := c.ExpiresAt
	newExpires := formatUTC(now.Add(time.Duration(ttlSeconds) * time.Second))
	c.ExpiresAt = newExpires

	if err := m.saveDoc(doc); err != nil {
		return nil, err
	}

	// Append ledger event.
	if m.LedgerWriter != nil {
		evt := ledger.Event{
			EventType: "claim_renewed",
			Data: map[string]any{
				"claim_id":   claimID,
				"expires_at": newExpires,
				"renewed_at": formatUTC(now),
				"ttl":        ttlSeconds,
			},
		}
		if err := m.LedgerWriter.AppendEvent(evt); err != nil {
			return nil, fmt.Errorf("claim: appending ledger event: %w", err)
		}
	}

	return &RenewResult{
		ClaimID:           claimID,
		Pattern:           c.Pattern,
		PreviousExpiresAt: previousExpires,
		NewExpiresAt:      newExpires,
	}, nil
}

// RenewAll renews all active, non-expired claims. Returns the list of renewed results.
func (m *Manager) RenewAll(ttlSeconds int) ([]RenewResult, error) {
	doc, err := m.loadDoc()
	if err != nil {
		return nil, err
	}

	now := m.Now()
	var results []RenewResult

	for id, c := range doc.Claims {
		if c.Status != "active" {
			continue
		}
		if c.ExpiresAt != "" {
			exp, err := parseExpiry(c.ExpiresAt)
			if err == nil && !exp.After(now) {
				continue // skip expired
			}
		}

		previousExpires := c.ExpiresAt
		newExpires := formatUTC(now.Add(time.Duration(ttlSeconds) * time.Second))
		c.ExpiresAt = newExpires

		// Append ledger event.
		if m.LedgerWriter != nil {
			evt := ledger.Event{
				EventType: "claim_renewed",
				Data: map[string]any{
					"claim_id":   id,
					"expires_at": newExpires,
					"renewed_at": formatUTC(now),
					"ttl":        ttlSeconds,
				},
			}
			if err := m.LedgerWriter.AppendEvent(evt); err != nil {
				return nil, fmt.Errorf("claim: appending ledger event: %w", err)
			}
		}

		results = append(results, RenewResult{
			ClaimID:           id,
			Pattern:           c.Pattern,
			PreviousExpiresAt: previousExpires,
			NewExpiresAt:      newExpires,
		})
	}

	if err := m.saveDoc(doc); err != nil {
		return nil, err
	}

	return results, nil
}

// patternsConflict checks if two patterns conflict. Handles exact match,
// bidirectional glob matching via filepath.Match, and directory containment.
func patternsConflict(a, b string) bool {
	if a == b {
		return true
	}

	aType, aPath := parsePatternType(a)
	bType, bPath := parsePatternType(b)

	// Bidirectional glob matching.
	if matched, _ := filepath.Match(bPath, aPath); matched {
		return true
	}
	if matched, _ := filepath.Match(aPath, bPath); matched {
		return true
	}

	// Directory containment.
	aNorm := strings.TrimRight(aPath, "/")
	bNorm := strings.TrimRight(bPath, "/")

	if aType == "file" && bType == "dir" {
		if strings.HasPrefix(aPath, bNorm+"/") {
			return true
		}
	}
	if aType == "dir" && bType == "file" {
		if strings.HasPrefix(bPath, aNorm+"/") {
			return true
		}
	}
	if aType == "dir" && bType == "dir" {
		if strings.HasPrefix(aNorm, bNorm+"/") || strings.HasPrefix(bNorm, aNorm+"/") {
			return true
		}
	}

	return false
}

// parsePatternType extracts the type prefix (file, dir) from a pattern.
// Returns ("file", path) if no prefix is present.
func parsePatternType(p string) (string, string) {
	if idx := strings.Index(p, ":"); idx >= 0 {
		return p[:idx], p[idx+1:]
	}
	return "file", p
}

// classifyConflict determines the conflict action per the V4 conflict matrix.
func classifyConflict(c *Claim, ownerID string, isExpired bool) string {
	if ownerID != "" && c.OwnerID == ownerID {
		return "ALLOW"
	}
	if isExpired {
		return "WARN"
	}
	if c.Mode == "exclusive" {
		return "BLOCK"
	}
	return "WARN"
}
