package update

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// mockHTTPResponse creates a mock HTTP response with the given status and body.
func mockHTTPResponse(status int, body string) *http.Response {
	return &http.Response{
		StatusCode: status,
		Body:       io.NopCloser(strings.NewReader(body)),
	}
}

// mockReleaseJSON returns JSON for a mock GitHub release with the given tag and asset name.
func mockReleaseJSON(tag, assetName, downloadURL string) string {
	release := releaseResponse{
		TagName: tag,
		Assets: []releaseAsset{
			{Name: assetName, BrowserDownloadURL: downloadURL},
		},
	}
	data, _ := json.Marshal(release)
	return string(data)
}

func TestCheckNewerAvailable(t *testing.T) {
	t.Parallel()

	releaseJSON := mockReleaseJSON("v2.0.0", "codeflow-linux-amd64", "https://example.com/codeflow")

	opts := &Options{
		CurrentVersion: "v1.0.0",
		HTTPGet: func(url string) (*http.Response, error) {
			return mockHTTPResponse(http.StatusOK, releaseJSON), nil
		},
	}

	info, err := Check(opts)
	if err != nil {
		t.Fatalf("Check() error: %v", err)
	}

	if !info.UpdateAvailable {
		t.Error("UpdateAvailable = false, want true")
	}
	if info.Latest != "v2.0.0" {
		t.Errorf("Latest = %s, want v2.0.0", info.Latest)
	}
	if info.Current != "v1.0.0" {
		t.Errorf("Current = %s, want v1.0.0", info.Current)
	}
	if info.DownloadURL != "https://example.com/codeflow" {
		t.Errorf("DownloadURL = %s, want https://example.com/codeflow", info.DownloadURL)
	}
}

func TestCheckAlreadyLatest(t *testing.T) {
	t.Parallel()

	releaseJSON := mockReleaseJSON("v1.0.0", "codeflow-linux-amd64", "https://example.com/codeflow")

	opts := &Options{
		CurrentVersion: "v1.0.0",
		HTTPGet: func(url string) (*http.Response, error) {
			return mockHTTPResponse(http.StatusOK, releaseJSON), nil
		},
	}

	info, err := Check(opts)
	if err != nil {
		t.Fatalf("Check() error: %v", err)
	}

	if info.UpdateAvailable {
		t.Error("UpdateAvailable = true, want false")
	}
}

func TestCheckNetworkError(t *testing.T) {
	t.Parallel()

	opts := &Options{
		CurrentVersion: "v1.0.0",
		HTTPGet: func(url string) (*http.Response, error) {
			return nil, fmt.Errorf("connection refused")
		},
	}

	_, err := Check(opts)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !errors.Is(err, ErrNetworkFailure) {
		t.Errorf("err = %v, want ErrNetworkFailure", err)
	}
}

func TestCheckNonOKStatus(t *testing.T) {
	t.Parallel()

	opts := &Options{
		CurrentVersion: "v1.0.0",
		HTTPGet: func(url string) (*http.Response, error) {
			return mockHTTPResponse(http.StatusNotFound, "not found"), nil
		},
	}

	_, err := Check(opts)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !errors.Is(err, ErrNetworkFailure) {
		t.Errorf("err = %v, want ErrNetworkFailure", err)
	}
}

func TestCheckInvalidJSON(t *testing.T) {
	t.Parallel()

	opts := &Options{
		CurrentVersion: "v1.0.0",
		HTTPGet: func(url string) (*http.Response, error) {
			return mockHTTPResponse(http.StatusOK, "not json"), nil
		},
	}

	_, err := Check(opts)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !errors.Is(err, ErrNetworkFailure) {
		t.Errorf("err = %v, want ErrNetworkFailure", err)
	}
}

func TestApplyAtomicReplacement(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	binaryPath := filepath.Join(tmpDir, "codeflow")

	// Create existing binary.
	if err := os.WriteFile(binaryPath, []byte("old-binary"), 0o755); err != nil {
		t.Fatal(err)
	}

	newContent := "new-binary-content"
	opts := &Options{
		BinaryPath: binaryPath,
		Force:      true,
		HTTPGet: func(url string) (*http.Response, error) {
			return mockHTTPResponse(http.StatusOK, newContent), nil
		},
	}

	info := &VersionInfo{
		UpdateAvailable: true,
		DownloadURL:     "https://example.com/codeflow",
		Latest:          "v2.0.0",
	}

	if err := Apply(opts, info); err != nil {
		t.Fatalf("Apply() error: %v", err)
	}

	// Verify the binary was replaced.
	data, err := os.ReadFile(binaryPath)
	if err != nil {
		t.Fatalf("reading updated binary: %v", err)
	}
	if string(data) != newContent {
		t.Errorf("binary content = %q, want %q", string(data), newContent)
	}

	// Verify executable permissions.
	stat, err := os.Stat(binaryPath)
	if err != nil {
		t.Fatal(err)
	}
	if stat.Mode().Perm()&0o111 == 0 {
		t.Error("expected executable permissions on updated binary")
	}
}

func TestApplyAlreadyLatestWithoutForce(t *testing.T) {
	t.Parallel()

	opts := &Options{
		Force: false,
	}

	info := &VersionInfo{
		UpdateAvailable: false,
	}

	err := Apply(opts, info)
	if !errors.Is(err, ErrAlreadyLatest) {
		t.Errorf("err = %v, want ErrAlreadyLatest", err)
	}
}

func TestApplyNoDownloadURL(t *testing.T) {
	t.Parallel()

	opts := &Options{
		Force: true,
	}

	info := &VersionInfo{
		UpdateAvailable: true,
		DownloadURL:     "",
	}

	err := Apply(opts, info)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !errors.Is(err, ErrUpdateFailed) {
		t.Errorf("err = %v, want ErrUpdateFailed", err)
	}
}

func TestApplyDownloadError(t *testing.T) {
	t.Parallel()

	opts := &Options{
		BinaryPath: filepath.Join(t.TempDir(), "codeflow"),
		Force:      true,
		HTTPGet: func(url string) (*http.Response, error) {
			return nil, fmt.Errorf("download failed")
		},
	}

	info := &VersionInfo{
		UpdateAvailable: true,
		DownloadURL:     "https://example.com/codeflow",
	}

	err := Apply(opts, info)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !errors.Is(err, ErrNetworkFailure) {
		t.Errorf("err = %v, want ErrNetworkFailure", err)
	}
}

func TestApplyForceBypassesVersionCheck(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	binaryPath := filepath.Join(tmpDir, "codeflow")

	if err := os.WriteFile(binaryPath, []byte("existing"), 0o755); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		BinaryPath: binaryPath,
		Force:      true,
		HTTPGet: func(url string) (*http.Response, error) {
			return mockHTTPResponse(http.StatusOK, "force-updated"), nil
		},
	}

	// UpdateAvailable is false but Force is true -- should proceed.
	info := &VersionInfo{
		UpdateAvailable: false,
		DownloadURL:     "https://example.com/codeflow",
		Latest:          "v1.0.0",
	}

	if err := Apply(opts, info); err != nil {
		t.Fatalf("Apply(force) error: %v", err)
	}

	data, err := os.ReadFile(binaryPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "force-updated" {
		t.Errorf("binary content = %q, want %q", string(data), "force-updated")
	}
}

func TestSyncTemplatesUpdatesFiles(t *testing.T) {
	t.Parallel()

	templateDir := t.TempDir()
	configDir := t.TempDir()

	// Create template files.
	subDir := filepath.Join(templateDir, "config")
	if err := os.MkdirAll(subDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(templateDir, "file1.json"), []byte(`{"a":1}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(subDir, "file2.json"), []byte(`{"b":2}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create existing config file with different content (should be updated).
	if err := os.WriteFile(filepath.Join(configDir, "file1.json"), []byte(`{"old":true}`), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		ConfigDir: configDir,
	}

	updated, err := SyncTemplates(opts, templateDir)
	if err != nil {
		t.Fatalf("SyncTemplates() error: %v", err)
	}

	if len(updated) != 2 {
		t.Errorf("len(updated) = %d, want 2: %v", len(updated), updated)
	}

	// Verify file1.json was updated.
	data, err := os.ReadFile(filepath.Join(configDir, "file1.json"))
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != `{"a":1}` {
		t.Errorf("file content = %q, want %q", string(data), `{"a":1}`)
	}

	// Verify file2.json was created.
	data, err = os.ReadFile(filepath.Join(configDir, "config", "file2.json"))
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != `{"b":2}` {
		t.Errorf("file content = %q, want %q", string(data), `{"b":2}`)
	}
}

func TestSyncTemplatesSkipsIdentical(t *testing.T) {
	t.Parallel()

	templateDir := t.TempDir()
	configDir := t.TempDir()

	content := `{"same":true}`
	if err := os.WriteFile(filepath.Join(templateDir, "file.json"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(configDir, "file.json"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		ConfigDir: configDir,
	}

	updated, err := SyncTemplates(opts, templateDir)
	if err != nil {
		t.Fatalf("SyncTemplates() error: %v", err)
	}

	if len(updated) != 0 {
		t.Errorf("len(updated) = %d, want 0 for identical content", len(updated))
	}
}

func TestSyncTemplatesEmptyTemplateDir(t *testing.T) {
	t.Parallel()

	opts := &Options{
		ConfigDir: t.TempDir(),
	}

	updated, err := SyncTemplates(opts, "")
	if err != nil {
		t.Fatalf("SyncTemplates('') error: %v", err)
	}
	if updated != nil {
		t.Errorf("SyncTemplates('') = %v, want nil", updated)
	}
}

func TestSyncTemplatesNonexistentDir(t *testing.T) {
	t.Parallel()

	opts := &Options{
		ConfigDir: t.TempDir(),
	}

	updated, err := SyncTemplates(opts, filepath.Join(t.TempDir(), "nonexistent"))
	if err != nil {
		t.Fatalf("SyncTemplates(nonexistent) error: %v", err)
	}
	if updated != nil {
		t.Errorf("SyncTemplates(nonexistent) = %v, want nil", updated)
	}
}

func TestCompareSemverNewer(t *testing.T) {
	t.Parallel()

	tests := []struct {
		a, b string
		want int
	}{
		{"1.0.0", "2.0.0", -1},
		{"1.0.0", "1.1.0", -1},
		{"1.0.0", "1.0.1", -1},
		{"0.9.9", "1.0.0", -1},
	}

	for _, tt := range tests {
		t.Run(tt.a+"_vs_"+tt.b, func(t *testing.T) {
			t.Parallel()
			got, err := CompareSemver(tt.a, tt.b)
			if err != nil {
				t.Fatalf("CompareSemver(%q, %q) error: %v", tt.a, tt.b, err)
			}
			if got != tt.want {
				t.Errorf("CompareSemver(%q, %q) = %d, want %d", tt.a, tt.b, got, tt.want)
			}
		})
	}
}

func TestCompareSemverEqual(t *testing.T) {
	t.Parallel()

	got, err := CompareSemver("1.2.3", "1.2.3")
	if err != nil {
		t.Fatalf("CompareSemver() error: %v", err)
	}
	if got != 0 {
		t.Errorf("CompareSemver(1.2.3, 1.2.3) = %d, want 0", got)
	}
}

func TestCompareSemverOlder(t *testing.T) {
	t.Parallel()

	tests := []struct {
		a, b string
		want int
	}{
		{"2.0.0", "1.0.0", 1},
		{"1.1.0", "1.0.0", 1},
		{"1.0.1", "1.0.0", 1},
		{"10.0.0", "9.9.9", 1},
	}

	for _, tt := range tests {
		t.Run(tt.a+"_vs_"+tt.b, func(t *testing.T) {
			t.Parallel()
			got, err := CompareSemver(tt.a, tt.b)
			if err != nil {
				t.Fatalf("CompareSemver(%q, %q) error: %v", tt.a, tt.b, err)
			}
			if got != tt.want {
				t.Errorf("CompareSemver(%q, %q) = %d, want %d", tt.a, tt.b, got, tt.want)
			}
		})
	}
}

func TestCompareSemverInvalidVersions(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		a, b string
	}{
		{"missing_patch", "1.0", "1.0.0"},
		{"non_numeric", "a.b.c", "1.0.0"},
		{"empty", "", "1.0.0"},
		{"too_many_parts_in_b", "1.0.0", "1.0"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			_, err := CompareSemver(tt.a, tt.b)
			if err == nil {
				t.Errorf("CompareSemver(%q, %q) expected error, got nil", tt.a, tt.b)
			}
			if !errors.Is(err, ErrInvalidVersion) {
				t.Errorf("err = %v, want ErrInvalidVersion", err)
			}
		})
	}
}

func TestStripV(t *testing.T) {
	t.Parallel()

	tests := []struct {
		input, want string
	}{
		{"v1.2.3", "1.2.3"},
		{"V1.2.3", "1.2.3"},
		{"1.2.3", "1.2.3"},
		{"", ""},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			t.Parallel()
			got := stripV(tt.input)
			if got != tt.want {
				t.Errorf("stripV(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestCheckNoMatchingAsset(t *testing.T) {
	t.Parallel()

	release := releaseResponse{
		TagName: "v2.0.0",
		Assets: []releaseAsset{
			{Name: "other-tool-linux", BrowserDownloadURL: "https://example.com/other"},
		},
	}
	data, _ := json.Marshal(release)

	opts := &Options{
		CurrentVersion: "v1.0.0",
		HTTPGet: func(url string) (*http.Response, error) {
			return mockHTTPResponse(http.StatusOK, string(data)), nil
		},
	}

	info, err := Check(opts)
	if err != nil {
		t.Fatalf("Check() error: %v", err)
	}

	if !info.UpdateAvailable {
		t.Error("UpdateAvailable = false, want true")
	}
	if info.DownloadURL != "" {
		t.Errorf("DownloadURL = %q, want empty", info.DownloadURL)
	}
}

func TestApplyDownloadNonOKStatus(t *testing.T) {
	t.Parallel()

	opts := &Options{
		BinaryPath: filepath.Join(t.TempDir(), "codeflow"),
		Force:      true,
		HTTPGet: func(url string) (*http.Response, error) {
			return mockHTTPResponse(http.StatusInternalServerError, "server error"), nil
		},
	}

	info := &VersionInfo{
		UpdateAvailable: true,
		DownloadURL:     "https://example.com/codeflow",
	}

	err := Apply(opts, info)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !errors.Is(err, ErrNetworkFailure) {
		t.Errorf("err = %v, want ErrNetworkFailure", err)
	}
}

func TestSyncTemplatesNotADirectory(t *testing.T) {
	t.Parallel()

	tmpFile := filepath.Join(t.TempDir(), "not-a-dir")
	if err := os.WriteFile(tmpFile, []byte("content"), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		ConfigDir: t.TempDir(),
	}

	_, err := SyncTemplates(opts, tmpFile)
	if err == nil {
		t.Fatal("expected error for non-directory path, got nil")
	}
}

func TestCheckCurrentNewerThanRemote(t *testing.T) {
	t.Parallel()

	releaseJSON := mockReleaseJSON("v1.0.0", "codeflow-linux", "https://example.com/codeflow")

	opts := &Options{
		CurrentVersion: "v2.0.0",
		HTTPGet: func(url string) (*http.Response, error) {
			return mockHTTPResponse(http.StatusOK, releaseJSON), nil
		},
	}

	info, err := Check(opts)
	if err != nil {
		t.Fatalf("Check() error: %v", err)
	}

	if info.UpdateAvailable {
		t.Error("UpdateAvailable = true, want false (current is newer)")
	}
}

func TestApplyCreateTempFailure(t *testing.T) {
	t.Parallel()

	// Use a nonexistent directory so CreateTemp fails.
	opts := &Options{
		BinaryPath: filepath.Join(t.TempDir(), "nonexistent-dir", "codeflow"),
		Force:      true,
		HTTPGet: func(url string) (*http.Response, error) {
			return mockHTTPResponse(http.StatusOK, "binary-data"), nil
		},
	}

	info := &VersionInfo{
		UpdateAvailable: true,
		DownloadURL:     "https://example.com/codeflow",
	}

	err := Apply(opts, info)
	if err == nil {
		t.Fatal("expected error for temp file creation failure, got nil")
	}
	if !errors.Is(err, ErrUpdateFailed) {
		t.Errorf("err = %v, want ErrUpdateFailed", err)
	}
}

// failReader is an io.Reader that always returns an error.
type failReader struct{}

func (failReader) Read([]byte) (int, error) {
	return 0, fmt.Errorf("simulated read failure")
}

func TestApplyIOCopyFailure(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	binaryPath := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(binaryPath, []byte("existing"), 0o755); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		BinaryPath: binaryPath,
		Force:      true,
		HTTPGet: func(url string) (*http.Response, error) {
			return &http.Response{
				StatusCode: http.StatusOK,
				Body:       io.NopCloser(failReader{}),
			}, nil
		},
	}

	info := &VersionInfo{
		UpdateAvailable: true,
		DownloadURL:     "https://example.com/codeflow",
	}

	err := Apply(opts, info)
	if err == nil {
		t.Fatal("expected error for io.Copy failure, got nil")
	}
	if !errors.Is(err, ErrUpdateFailed) {
		t.Errorf("err = %v, want ErrUpdateFailed", err)
	}

	// Verify original binary is untouched (atomic guarantee).
	data, err := os.ReadFile(binaryPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "existing" {
		t.Errorf("binary content = %q, want %q (atomic guarantee)", string(data), "existing")
	}
}

func TestParseSemverNegativeSegment(t *testing.T) {
	t.Parallel()

	_, err := parseSemver("-1.0.0")
	if err == nil {
		t.Fatal("expected error for negative segment, got nil")
	}
}

func TestSyncTemplatesWalkDirError(t *testing.T) {
	t.Parallel()

	templateDir := t.TempDir()
	configDir := t.TempDir()

	// Create a template file.
	if err := os.WriteFile(filepath.Join(templateDir, "file.txt"), []byte("data"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Make the config dir a file so MkdirAll fails when trying to create subdirs.
	destFile := filepath.Join(configDir, "file.txt")
	// Create the destination as a directory (which will have different content), triggering the write path.
	// Actually, let's create a read-only parent to trigger a write error.
	restrictedDir := filepath.Join(t.TempDir(), "restricted")
	if err := os.MkdirAll(restrictedDir, 0o000); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(restrictedDir, 0o755) // cleanup

	opts := &Options{
		ConfigDir: filepath.Join(restrictedDir, "subdir"),
	}

	_, err := SyncTemplates(opts, templateDir)
	if err == nil {
		// On some systems root can still write — skip check if no error.
		_ = destFile
		t.Skip("insufficient permission restriction on this system")
	}
}

func TestCheckDefaultHTTPGet(t *testing.T) {
	t.Parallel()

	// Test that Check uses default http.Get when HTTPGet is nil.
	// This will fail with a network error, which is expected.
	opts := &Options{
		CurrentVersion: "v1.0.0",
		HTTPGet:        nil, // Will use http.Get, which should fail in test.
	}

	_, err := Check(opts)
	// We expect a network error since we're not mocking and the URL won't resolve in tests.
	if err == nil {
		t.Skip("unexpectedly succeeded — network is available")
	}
	// The error should be a network failure.
	if !errors.Is(err, ErrNetworkFailure) {
		t.Errorf("err = %v, want ErrNetworkFailure", err)
	}
}
