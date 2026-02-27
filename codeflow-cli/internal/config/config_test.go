package config

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

// writeJSON is a test helper that writes a JSON file to the given path.
func writeJSON(t *testing.T, path string, data map[string]any) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("creating directory for %s: %v", path, err)
	}
	raw, err := json.MarshalIndent(data, "", "  ")
	if err != nil {
		t.Fatalf("marshaling JSON for %s: %v", path, err)
	}
	if err := os.WriteFile(path, raw, 0o644); err != nil {
		t.Fatalf("writing %s: %v", path, err)
	}
}

// setupLocalConfig creates a project-local config directory with a test JSON file.
func setupLocalConfig(t *testing.T, projectDir string) {
	t.Helper()
	configDir := filepath.Join(projectDir, DefaultLocalDir)
	writeJSON(t, filepath.Join(configDir, "pathflow-config.json"), map[string]any{
		"version": "1.0.0",
		"rework_limits": map[string]any{
			"max_rework_iterations": float64(3),
			"max_qa_retries":        float64(2),
		},
		"enabled":    true,
		"null_field": nil,
	})
}

// setupGlobalConfig creates a global config directory with a test JSON file.
func setupGlobalConfig(t *testing.T, globalDir string) {
	t.Helper()
	writeJSON(t, filepath.Join(globalDir, "settings.json"), map[string]any{
		"telemetry": false,
		"log_level": "info",
		"rework_limits": map[string]any{
			"max_rework_iterations": float64(5),
		},
	})
}

func TestListProjectLocal(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	kv, err := List(opts, false)
	if err != nil {
		t.Fatalf("List() error: %v", err)
	}

	if v, ok := kv["pathflow-config.version"]; !ok || v != "1.0.0" {
		t.Errorf("expected pathflow-config.version=1.0.0, got %v", v)
	}
	if v, ok := kv["pathflow-config.rework_limits.max_rework_iterations"]; !ok || v != float64(3) {
		t.Errorf("expected pathflow-config.rework_limits.max_rework_iterations=3, got %v", v)
	}
	if v, ok := kv["pathflow-config.enabled"]; !ok || v != true {
		t.Errorf("expected pathflow-config.enabled=true, got %v", v)
	}
}

func TestListGlobal(t *testing.T) {
	t.Parallel()
	globalDir := t.TempDir()
	setupGlobalConfig(t, globalDir)

	opts := &Options{
		ProjectDir: t.TempDir(),
		GlobalDir:  globalDir,
	}

	kv, err := List(opts, true)
	if err != nil {
		t.Fatalf("List(global=true) error: %v", err)
	}

	if v, ok := kv["settings.telemetry"]; !ok || v != false {
		t.Errorf("expected settings.telemetry=false, got %v", v)
	}
	if v, ok := kv["settings.log_level"]; !ok || v != "info" {
		t.Errorf("expected settings.log_level=info, got %v", v)
	}
}

func TestGetExistingKey(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	val, err := Get("pathflow-config.rework_limits.max_qa_retries", opts, false)
	if err != nil {
		t.Fatalf("Get() error: %v", err)
	}
	if val != float64(2) {
		t.Errorf("expected 2, got %v", val)
	}
}

func TestGetMissingKey(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	_, err := Get("nonexistent.key", opts, false)
	if err == nil {
		t.Fatal("expected error for missing key, got nil")
	}
	if !errors.Is(err, ErrKeyNotFound) {
		t.Errorf("expected ErrKeyNotFound, got: %v", err)
	}
}

func TestSetValidKey(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	err := Set("pathflow-config.rework_limits.max_rework_iterations", "10", opts, false)
	if err != nil {
		t.Fatalf("Set() error: %v", err)
	}

	// Verify the value was updated.
	val, err := Get("pathflow-config.rework_limits.max_rework_iterations", opts, false)
	if err != nil {
		t.Fatalf("Get() after Set() error: %v", err)
	}
	if val != float64(10) {
		t.Errorf("expected 10 after set, got %v", val)
	}
}

func TestSetInvalidValue(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	// pathflow-config.rework_limits.max_rework_iterations is a number; "notanumber" should fail.
	err := Set("pathflow-config.rework_limits.max_rework_iterations", "notanumber", opts, false)
	if err == nil {
		t.Fatal("expected error for invalid value, got nil")
	}
	if !errors.Is(err, ErrInvalidValue) {
		t.Errorf("expected ErrInvalidValue, got: %v", err)
	}
}

func TestSetInvalidBoolValue(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	err := Set("pathflow-config.enabled", "notabool", opts, false)
	if err == nil {
		t.Fatal("expected error for invalid bool value, got nil")
	}
	if !errors.Is(err, ErrInvalidValue) {
		t.Errorf("expected ErrInvalidValue, got: %v", err)
	}
}

func TestSetUnknownKey(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	err := Set("totally.unknown.key", "value", opts, false)
	if err == nil {
		t.Fatal("expected error for unknown key, got nil")
	}
	if !errors.Is(err, ErrInvalidValue) {
		t.Errorf("expected ErrInvalidValue, got: %v", err)
	}
}

func TestGlobalListReturnsOnlyGlobal(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	globalDir := t.TempDir()

	setupLocalConfig(t, projectDir)
	setupGlobalConfig(t, globalDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  globalDir,
	}

	kv, err := List(opts, true)
	if err != nil {
		t.Fatalf("List(global=true) error: %v", err)
	}

	// Should have global keys only.
	if _, ok := kv["settings.telemetry"]; !ok {
		t.Error("expected settings.telemetry key in global config")
	}
	// Should NOT have local-only keys (pathflow-config.enabled is only in local).
	if _, ok := kv["pathflow-config.enabled"]; ok {
		t.Error("did not expect 'pathflow-config.enabled' key in global-only listing")
	}
}

func TestGlobalSetCreatesDirectory(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	globalDir := filepath.Join(t.TempDir(), "nonexistent-global")
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  globalDir,
	}

	// Set a value that exists in local config to global.
	err := Set("pathflow-config.version", "2.0.0", opts, true)
	if err != nil {
		t.Fatalf("Set(global=true) error: %v", err)
	}

	// Verify global directory was created.
	info, err := os.Stat(globalDir)
	if err != nil {
		t.Fatalf("global dir not created: %v", err)
	}
	if !info.IsDir() {
		t.Error("global dir is not a directory")
	}

	// When writing to a previously empty global dir, the fallback creates
	// config.json. Reading back produces namespace prefix "config", so the
	// full key becomes "config.pathflow-config.version".
	val, err := Get("config.pathflow-config.version", opts, true)
	if err != nil {
		t.Fatalf("Get(global=true) error: %v", err)
	}
	if val != "2.0.0" {
		t.Errorf("expected 2.0.0, got %v", val)
	}
}

func TestMergeGlobalWithLocal(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	globalDir := t.TempDir()

	// Write global config with shared.json (max_retries=5, log_level=info).
	writeJSON(t, filepath.Join(globalDir, "shared.json"), map[string]any{
		"max_retries": float64(5),
		"log_level":   "info",
	})

	// Write local config with shared.json (max_retries=3, enabled=true).
	// Same filename -> same namespace prefix "shared", so max_retries overlaps.
	localConfigDir := filepath.Join(projectDir, DefaultLocalDir)
	writeJSON(t, filepath.Join(localConfigDir, "shared.json"), map[string]any{
		"max_retries": float64(3),
		"enabled":     true,
	})

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  globalDir,
	}

	kv, err := List(opts, false)
	if err != nil {
		t.Fatalf("List() error: %v", err)
	}

	// Global-only key should be present.
	if v, ok := kv["shared.log_level"]; !ok || v != "info" {
		t.Errorf("expected global shared.log_level=info, got %v", v)
	}

	// Local-only key should be present.
	if v, ok := kv["shared.enabled"]; !ok || v != true {
		t.Errorf("expected local shared.enabled=true, got %v", v)
	}

	// Key in both: local should win.
	if v, ok := kv["shared.max_retries"]; !ok || v != float64(3) {
		t.Errorf("expected local override shared.max_retries=3, got %v", v)
	}
}

func TestSetBooleanValue(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	err := Set("pathflow-config.enabled", "false", opts, false)
	if err != nil {
		t.Fatalf("Set(bool) error: %v", err)
	}

	val, err := Get("pathflow-config.enabled", opts, false)
	if err != nil {
		t.Fatalf("Get() after Set(bool) error: %v", err)
	}
	if val != false {
		t.Errorf("expected false, got %v", val)
	}
}

func TestSetStringValue(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	err := Set("pathflow-config.version", "2.0.0", opts, false)
	if err != nil {
		t.Fatalf("Set(string) error: %v", err)
	}

	val, err := Get("pathflow-config.version", opts, false)
	if err != nil {
		t.Fatalf("Get() after Set(string) error: %v", err)
	}
	if val != "2.0.0" {
		t.Errorf("expected 2.0.0, got %v", val)
	}
}

func TestSetNilFieldValue(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	// pathflow-config.null_field has nil type; should accept a string value.
	err := Set("pathflow-config.null_field", "now-set", opts, false)
	if err != nil {
		t.Fatalf("Set(nil field) error: %v", err)
	}

	val, err := Get("pathflow-config.null_field", opts, false)
	if err != nil {
		t.Fatalf("Get() after Set(nil) error: %v", err)
	}
	if val != "now-set" {
		t.Errorf("expected now-set, got %v", val)
	}
}

func TestListEmptyDir(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	configDir := filepath.Join(projectDir, DefaultLocalDir)
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	kv, err := List(opts, false)
	if err != nil {
		t.Fatalf("List() on empty dir error: %v", err)
	}
	if len(kv) != 0 {
		t.Errorf("expected 0 keys for empty dir, got %d", len(kv))
	}
}

func TestListNonexistentDir(t *testing.T) {
	t.Parallel()

	opts := &Options{
		ProjectDir: filepath.Join(t.TempDir(), "no-such-project"),
		GlobalDir:  filepath.Join(t.TempDir(), "no-such-global"),
	}

	// Non-merged: global returns empty for nonexistent dir.
	kv, err := List(opts, true)
	if err != nil {
		t.Fatalf("List(global=true, nonexistent) error: %v", err)
	}
	if len(kv) != 0 {
		t.Errorf("expected 0 keys, got %d", len(kv))
	}
}

func TestReadConfigDirNotADirectory(t *testing.T) {
	t.Parallel()
	tmp := t.TempDir()
	filePath := filepath.Join(tmp, "not-a-dir.json")
	if err := os.WriteFile(filePath, []byte(`{}`), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := readConfigDir(filePath)
	if err == nil {
		t.Fatal("expected error for non-directory path, got nil")
	}
}

func TestSortedKeys(t *testing.T) {
	t.Parallel()
	m := map[string]any{
		"zebra":  1,
		"alpha":  2,
		"middle": 3,
		"beta":   4,
	}

	keys := SortedKeys(m)
	expected := []string{"alpha", "beta", "middle", "zebra"}
	if len(keys) != len(expected) {
		t.Fatalf("expected %d keys, got %d", len(expected), len(keys))
	}
	for i, k := range keys {
		if k != expected[i] {
			t.Errorf("key[%d]: expected %q, got %q", i, expected[i], k)
		}
	}
}

func TestMultipleJSONFiles(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	configDir := filepath.Join(projectDir, DefaultLocalDir)

	writeJSON(t, filepath.Join(configDir, "enforcement.json"), map[string]any{
		"version":     "1.4.0",
		"description": "enforcement config",
	})
	writeJSON(t, filepath.Join(configDir, "pathflow.json"), map[string]any{
		"version": "1.0.0",
		"phases": map[string]any{
			"count": float64(7),
		},
	})

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	kv, err := List(opts, false)
	if err != nil {
		t.Fatalf("List() error: %v", err)
	}

	// Should have namespaced keys from both files — no collision.
	if v, ok := kv["enforcement.version"]; !ok || v != "1.4.0" {
		t.Errorf("expected enforcement.version=1.4.0, got %v", v)
	}
	if v, ok := kv["pathflow.version"]; !ok || v != "1.0.0" {
		t.Errorf("expected pathflow.version=1.0.0, got %v", v)
	}
	if v, ok := kv["enforcement.description"]; !ok || v != "enforcement config" {
		t.Errorf("expected enforcement.description, got %v", v)
	}
	if _, ok := kv["pathflow.phases.count"]; !ok {
		t.Error("expected 'pathflow.phases.count' key from pathflow.json")
	}
}

func TestConvertValueComplexType(t *testing.T) {
	t.Parallel()
	// Arrays are complex types that should be rejected.
	_, err := convertValue("test", []any{"a", "b"})
	if err == nil {
		t.Fatal("expected error for complex type, got nil")
	}
}

func TestSetCreatesIntermediateKeys(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	configDir := filepath.Join(projectDir, DefaultLocalDir)

	// Create a config with a deeply nested structure.
	writeJSON(t, filepath.Join(configDir, "test.json"), map[string]any{
		"a": map[string]any{
			"b": map[string]any{
				"c": "original",
			},
		},
	})

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	err := Set("test.a.b.c", "updated", opts, false)
	if err != nil {
		t.Fatalf("Set(nested) error: %v", err)
	}

	val, err := Get("test.a.b.c", opts, false)
	if err != nil {
		t.Fatalf("Get(nested) error: %v", err)
	}
	if val != "updated" {
		t.Errorf("expected updated, got %v", val)
	}
}

func TestContainsKeyPathNonMapIntermediate(t *testing.T) {
	t.Parallel()
	// When an intermediate value is not a map, containsKeyPath should return false.
	obj := map[string]any{
		"a": "string-not-map",
	}
	if containsKeyPath(obj, []string{"a", "b"}) {
		t.Error("expected false for non-map intermediate")
	}
}

func TestSetNestedValueOverwritesNonMap(t *testing.T) {
	t.Parallel()
	obj := map[string]any{
		"a": "string-value",
	}
	// Setting a.b should overwrite "a" (a string) with a map containing "b".
	setNestedValue(obj, []string{"a", "b"}, "nested-value")
	nested, ok := obj["a"].(map[string]any)
	if !ok {
		t.Fatalf("expected obj[a] to be map, got %T", obj["a"])
	}
	if nested["b"] != "nested-value" {
		t.Errorf("expected nested-value, got %v", nested["b"])
	}
}

func TestResolveKeyToFileEmptyDir(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	// Empty directory should fall back to config.json.
	path, parts, err := resolveKeyToFile(dir, "some.key")
	if err != nil {
		t.Fatalf("resolveKeyToFile() error: %v", err)
	}
	if filepath.Base(path) != "config.json" {
		t.Errorf("expected config.json fallback, got %s", filepath.Base(path))
	}
	if len(parts) != 2 || parts[0] != "some" || parts[1] != "key" {
		t.Errorf("unexpected parts: %v", parts)
	}
}

func TestReadConfigDirWithInvalidJSON(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "bad.json"), []byte(`{not valid`), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := readConfigDir(dir)
	if err == nil {
		t.Fatal("expected error for invalid JSON, got nil")
	}
}

func TestGetGlobalKey(t *testing.T) {
	t.Parallel()
	globalDir := t.TempDir()
	setupGlobalConfig(t, globalDir)

	opts := &Options{
		ProjectDir: t.TempDir(),
		GlobalDir:  globalDir,
	}

	val, err := Get("settings.log_level", opts, true)
	if err != nil {
		t.Fatalf("Get(global) error: %v", err)
	}
	if val != "info" {
		t.Errorf("expected info, got %v", val)
	}
}

func TestSetGlobalKey(t *testing.T) {
	t.Parallel()
	globalDir := t.TempDir()
	projectDir := t.TempDir()

	setupGlobalConfig(t, globalDir)
	setupLocalConfig(t, projectDir)

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  globalDir,
	}

	err := Set("settings.log_level", "debug", opts, true)
	if err != nil {
		t.Fatalf("Set(global) error: %v", err)
	}

	val, err := Get("settings.log_level", opts, true)
	if err != nil {
		t.Fatalf("Get(global) after Set error: %v", err)
	}
	if val != "debug" {
		t.Errorf("expected debug, got %v", val)
	}
}

func TestFlattenMapTopLevel(t *testing.T) {
	t.Parallel()
	result := make(map[string]any)
	m := map[string]any{
		"key1": "val1",
		"key2": float64(42),
	}
	flattenMap("", m, result)

	if result["key1"] != "val1" {
		t.Errorf("expected val1, got %v", result["key1"])
	}
	if result["key2"] != float64(42) {
		t.Errorf("expected 42, got %v", result["key2"])
	}
}

func TestWriteConfigValueExistingFile(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	writeJSON(t, filepath.Join(dir, "test.json"), map[string]any{
		"existing": "value",
	})

	err := writeConfigValue(dir, "test.existing", "new-value")
	if err != nil {
		t.Fatalf("writeConfigValue() error: %v", err)
	}

	// Read back and verify.
	data, err := os.ReadFile(filepath.Join(dir, "test.json"))
	if err != nil {
		t.Fatal(err)
	}
	var obj map[string]any
	if err := json.Unmarshal(data, &obj); err != nil {
		t.Fatal(err)
	}
	if obj["existing"] != "new-value" {
		t.Errorf("expected new-value, got %v", obj["existing"])
	}
}

func TestSetComplexValueRejected(t *testing.T) {
	t.Parallel()
	projectDir := t.TempDir()
	configDir := filepath.Join(projectDir, DefaultLocalDir)

	// Create config with an array value.
	writeJSON(t, filepath.Join(configDir, "test.json"), map[string]any{
		"list_field": []any{"a", "b", "c"},
	})

	opts := &Options{
		ProjectDir: projectDir,
		GlobalDir:  filepath.Join(t.TempDir(), "global"),
	}

	err := Set("test.list_field", "value", opts, false)
	if err == nil {
		t.Fatal("expected error for complex value type, got nil")
	}
	if !errors.Is(err, ErrInvalidValue) {
		t.Errorf("expected ErrInvalidValue, got: %v", err)
	}
}
