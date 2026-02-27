package update

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

// Sentinel errors for update operations.
var (
	// ErrAlreadyLatest indicates the current version is already the latest.
	ErrAlreadyLatest = errors.New("already at latest version")

	// ErrNetworkFailure indicates a failure contacting the version check endpoint.
	ErrNetworkFailure = errors.New("network failure")

	// ErrInvalidVersion indicates a version string could not be parsed.
	ErrInvalidVersion = errors.New("invalid version")

	// ErrUpdateFailed indicates the binary replacement failed.
	ErrUpdateFailed = errors.New("update failed")
)

// Options holds configuration for update operations.
type Options struct {
	// BinaryPath is the path to the codeflow binary to replace.
	BinaryPath string

	// ConfigDir is the .codeflow/ config directory for template syncing.
	ConfigDir string

	// CurrentVersion is the currently installed version (e.g., "v1.2.3" or "1.2.3").
	CurrentVersion string

	// HTTPGet performs an HTTP GET request. Injectable for testing.
	HTTPGet func(url string) (*http.Response, error)

	// CheckOnly when true only checks for updates without applying.
	CheckOnly bool

	// Force bypasses version compatibility checks (allows downgrades/reinstalls).
	Force bool
}

// VersionInfo holds information about an available version.
type VersionInfo struct {
	// Latest is the latest available version string (e.g., "v1.3.0").
	Latest string

	// Current is the currently installed version string.
	Current string

	// UpdateAvailable is true when Latest is newer than Current.
	UpdateAvailable bool

	// DownloadURL is the URL to download the new binary.
	DownloadURL string
}

// releaseResponse represents the GitHub releases API response (subset).
type releaseResponse struct {
	TagName string         `json:"tag_name"`
	Assets  []releaseAsset `json:"assets"`
}

// releaseAsset represents a single asset in a GitHub release.
type releaseAsset struct {
	Name               string `json:"name"`
	BrowserDownloadURL string `json:"browser_download_url"`
}

// defaultReleaseURL is the GitHub API endpoint for the latest release.
const defaultReleaseURL = "https://api.github.com/repos/codeflow-ai/codeflow/releases/latest"

// Check queries the remote release endpoint and compares versions.
func Check(opts *Options) (*VersionInfo, error) {
	httpGet := opts.HTTPGet
	if httpGet == nil {
		httpGet = http.Get
	}

	resp, err := httpGet(defaultReleaseURL)
	if err != nil {
		return nil, fmt.Errorf("%w: %v", ErrNetworkFailure, err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("%w: unexpected status %d", ErrNetworkFailure, resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("%w: reading response: %v", ErrNetworkFailure, err)
	}

	var release releaseResponse
	if err := json.Unmarshal(body, &release); err != nil {
		return nil, fmt.Errorf("%w: parsing response: %v", ErrNetworkFailure, err)
	}

	info := &VersionInfo{
		Latest:  release.TagName,
		Current: opts.CurrentVersion,
	}

	// Find download URL from assets.
	for _, asset := range release.Assets {
		if strings.Contains(asset.Name, "codeflow") {
			info.DownloadURL = asset.BrowserDownloadURL
			break
		}
	}

	cmp, err := CompareSemver(stripV(opts.CurrentVersion), stripV(release.TagName))
	if err != nil {
		return nil, fmt.Errorf("comparing versions: %w", err)
	}

	info.UpdateAvailable = cmp < 0
	return info, nil
}

// Apply downloads the new binary and performs atomic replacement.
func Apply(opts *Options, info *VersionInfo) error {
	if !info.UpdateAvailable && !opts.Force {
		return ErrAlreadyLatest
	}

	if info.DownloadURL == "" {
		return fmt.Errorf("%w: no download URL available", ErrUpdateFailed)
	}

	httpGet := opts.HTTPGet
	if httpGet == nil {
		httpGet = http.Get
	}

	resp, err := httpGet(info.DownloadURL)
	if err != nil {
		return fmt.Errorf("%w: downloading binary: %v", ErrNetworkFailure, err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("%w: download returned status %d", ErrNetworkFailure, resp.StatusCode)
	}

	// Write to temp file in the same directory as the target binary
	// to ensure os.Rename works (same filesystem).
	dir := filepath.Dir(opts.BinaryPath)
	tmpFile, err := os.CreateTemp(dir, "codeflow-update-*")
	if err != nil {
		return fmt.Errorf("%w: creating temp file: %v", ErrUpdateFailed, err)
	}
	tmpPath := tmpFile.Name()

	// Clean up temp file on any error path.
	defer func() {
		// If the file still exists after rename attempt, remove it.
		if _, statErr := os.Stat(tmpPath); statErr == nil {
			os.Remove(tmpPath)
		}
	}()

	if _, err := io.Copy(tmpFile, resp.Body); err != nil {
		tmpFile.Close()
		return fmt.Errorf("%w: writing binary: %v", ErrUpdateFailed, err)
	}

	if err := tmpFile.Close(); err != nil {
		return fmt.Errorf("%w: closing temp file: %v", ErrUpdateFailed, err)
	}

	// Preserve executable permissions.
	if err := os.Chmod(tmpPath, 0o755); err != nil {
		return fmt.Errorf("%w: setting permissions: %v", ErrUpdateFailed, err)
	}

	// Atomic replacement: rename over the existing binary.
	if err := os.Rename(tmpPath, opts.BinaryPath); err != nil {
		return fmt.Errorf("%w: atomic replacement: %v", ErrUpdateFailed, err)
	}

	return nil
}

// SyncTemplates compares local .codeflow/ files against source templates
// and updates those that differ. The templateDir parameter specifies the
// source directory containing reference templates.
func SyncTemplates(opts *Options, templateDir string) ([]string, error) {
	if templateDir == "" {
		return nil, nil
	}

	info, err := os.Stat(templateDir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("checking template dir: %w", err)
	}
	if !info.IsDir() {
		return nil, fmt.Errorf("template path is not a directory: %s", templateDir)
	}

	var updated []string

	err = filepath.WalkDir(templateDir, func(srcPath string, d os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if d.IsDir() {
			return nil
		}

		relPath, err := filepath.Rel(templateDir, srcPath)
		if err != nil {
			return fmt.Errorf("computing relative path: %w", err)
		}

		destPath := filepath.Join(opts.ConfigDir, relPath)

		srcData, err := os.ReadFile(srcPath)
		if err != nil {
			return fmt.Errorf("reading template %s: %w", relPath, err)
		}

		// Check if destination exists and has the same content.
		destData, err := os.ReadFile(destPath)
		if err == nil && string(destData) == string(srcData) {
			return nil // Already up to date.
		}

		// Create parent directory if needed.
		if err := os.MkdirAll(filepath.Dir(destPath), 0o755); err != nil {
			return fmt.Errorf("creating directory for %s: %w", relPath, err)
		}

		if err := os.WriteFile(destPath, srcData, 0o644); err != nil {
			return fmt.Errorf("writing %s: %w", relPath, err)
		}

		updated = append(updated, relPath)
		return nil
	})

	if err != nil {
		return updated, fmt.Errorf("syncing templates: %w", err)
	}

	return updated, nil
}

// CompareSemver compares two semantic version strings (without "v" prefix).
// Returns -1 if a < b, 0 if a == b, 1 if a > b.
// Versions must be in MAJOR.MINOR.PATCH format.
func CompareSemver(a, b string) (int, error) {
	aParts, err := parseSemver(a)
	if err != nil {
		return 0, fmt.Errorf("%w: %v", ErrInvalidVersion, err)
	}

	bParts, err := parseSemver(b)
	if err != nil {
		return 0, fmt.Errorf("%w: %v", ErrInvalidVersion, err)
	}

	for i := 0; i < 3; i++ {
		if aParts[i] < bParts[i] {
			return -1, nil
		}
		if aParts[i] > bParts[i] {
			return 1, nil
		}
	}

	return 0, nil
}

// parseSemver parses a MAJOR.MINOR.PATCH version string into three integers.
func parseSemver(v string) ([3]int, error) {
	parts := strings.SplitN(v, ".", 3)
	if len(parts) != 3 {
		return [3]int{}, fmt.Errorf("expected MAJOR.MINOR.PATCH, got %q", v)
	}

	var result [3]int
	for i, part := range parts {
		n, err := strconv.Atoi(part)
		if err != nil {
			return [3]int{}, fmt.Errorf("non-numeric segment %q in version %q", part, v)
		}
		if n < 0 {
			return [3]int{}, fmt.Errorf("negative segment %d in version %q", n, v)
		}
		result[i] = n
	}

	return result, nil
}

// stripV removes a leading "v" or "V" prefix from a version string.
func stripV(version string) string {
	return strings.TrimPrefix(strings.TrimPrefix(version, "v"), "V")
}
