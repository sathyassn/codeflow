package config

import (
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

// Sentinel errors for config operations.
var (
	// ErrKeyNotFound indicates the requested configuration key does not exist.
	ErrKeyNotFound = errors.New("key not found")

	// ErrInvalidValue indicates the provided value is not valid for the key.
	ErrInvalidValue = errors.New("invalid value")
)

// DefaultLocalDir is the default project-local config directory.
const DefaultLocalDir = ".codeflow/config"

// Options holds configuration for config operations.
type Options struct {
	// ProjectDir is the project root directory containing .codeflow/config/.
	ProjectDir string

	// GlobalDir is the global config directory (typically ~/.config/codeflow/).
	GlobalDir string
}

// localDir returns the project-local config directory.
func (o *Options) localDir() string {
	return filepath.Join(o.ProjectDir, DefaultLocalDir)
}

// List reads all configuration keys and values. When global is true, only the
// global config directory is read. Otherwise, global and local configs are
// merged with local values taking precedence.
func List(opts *Options, global bool) (map[string]any, error) {
	if global {
		return readConfigDir(opts.GlobalDir)
	}

	// Merge: global provides defaults, local overrides.
	globalKV, err := readConfigDir(opts.GlobalDir)
	if err != nil {
		// Global dir may not exist; treat as empty.
		globalKV = make(map[string]any)
	}

	localKV, err := readConfigDir(opts.localDir())
	if err != nil {
		return nil, fmt.Errorf("reading local config: %w", err)
	}

	// Local overrides global.
	for k, v := range localKV {
		globalKV[k] = v
	}

	return globalKV, nil
}

// Get retrieves a specific configuration value by dot-notation key.
func Get(key string, opts *Options, global bool) (any, error) {
	kv, err := List(opts, global)
	if err != nil {
		return nil, err
	}

	val, ok := kv[key]
	if !ok {
		return nil, fmt.Errorf("%w: %s", ErrKeyNotFound, key)
	}

	return val, nil
}

// Set updates a configuration value. It validates that the key exists in the
// current config and that the value type is compatible. When global is true, the
// value is written to the global config directory.
func Set(key, value string, opts *Options, global bool) error {
	targetDir := opts.localDir()
	if global {
		targetDir = opts.GlobalDir
	}

	// Read merged config (global + local) to validate key existence and type.
	allKV, err := List(opts, false)
	if err != nil {
		return fmt.Errorf("reading config: %w", err)
	}

	// Check if key exists in the merged config.
	existingVal, keyExists := allKV[key]

	if !keyExists {
		return fmt.Errorf("%w: unknown key %q", ErrInvalidValue, key)
	}

	// Convert the string value to the appropriate type based on existing value.
	converted, err := convertValue(value, existingVal)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrInvalidValue, err)
	}

	// Determine which file and nested path to write to.
	return writeConfigValue(targetDir, key, converted)
}

// readConfigDir walks a config directory, reads all *.json files, and returns
// a flat dot-notation key-value map. Keys are namespaced by the file's relative
// path from the config directory to prevent collisions between files that share
// top-level key names. For example, pathflow/pathflow-config.json with key
// "version" becomes "pathflow.pathflow-config.version".
func readConfigDir(dir string) (map[string]any, error) {
	result := make(map[string]any)

	info, err := os.Stat(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return result, nil
		}
		return nil, fmt.Errorf("stat config dir %s: %w", dir, err)
	}
	if !info.IsDir() {
		return nil, fmt.Errorf("config path %s is not a directory", dir)
	}

	err = filepath.WalkDir(dir, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() || !strings.HasSuffix(d.Name(), ".json") {
			return nil
		}

		data, err := os.ReadFile(path)
		if err != nil {
			return fmt.Errorf("reading %s: %w", path, err)
		}

		var obj map[string]any
		if err := json.Unmarshal(data, &obj); err != nil {
			return fmt.Errorf("parsing %s: %w", path, err)
		}

		prefix := fileNamespacePrefix(dir, path)
		flattenMap(prefix, obj, result)
		return nil
	})

	if err != nil {
		return nil, err
	}

	return result, nil
}

// fileNamespacePrefix computes a dot-notation prefix from a file's relative
// path to the config root directory. For example:
//
//	dir="/a/config", path="/a/config/pathflow/pathflow-config.json"
//	  -> "pathflow.pathflow-config"
//	dir="/a/config", path="/a/config/settings.json"
//	  -> "settings"
func fileNamespacePrefix(dir, path string) string {
	rel, err := filepath.Rel(dir, path)
	if err != nil {
		// Fallback: use just the filename stem.
		return strings.TrimSuffix(filepath.Base(path), ".json")
	}

	// Remove .json extension.
	rel = strings.TrimSuffix(rel, ".json")

	// Convert path separators to dots.
	return strings.ReplaceAll(rel, string(filepath.Separator), ".")
}

// flattenMap recursively flattens a nested map into dot-notation keys.
func flattenMap(prefix string, m map[string]any, result map[string]any) {
	for k, v := range m {
		key := k
		if prefix != "" {
			key = prefix + "." + k
		}

		if nested, ok := v.(map[string]any); ok {
			flattenMap(key, nested, result)
		} else {
			result[key] = v
		}
	}
}

// convertValue converts a string value to the appropriate Go type based on
// the existing value's type.
func convertValue(value string, existing any) (any, error) {
	switch existing.(type) {
	case float64:
		// JSON numbers are float64.
		f, err := strconv.ParseFloat(value, 64)
		if err != nil {
			return nil, fmt.Errorf("expected number for key, got %q", value)
		}
		return f, nil
	case bool:
		b, err := strconv.ParseBool(value)
		if err != nil {
			return nil, fmt.Errorf("expected boolean (true/false) for key, got %q", value)
		}
		return b, nil
	case string:
		return value, nil
	case nil:
		return value, nil
	default:
		// For arrays and other complex types, reject set operations.
		return nil, fmt.Errorf("cannot set complex value type %T via CLI", existing)
	}
}

// writeConfigValue writes a single key-value pair to the appropriate JSON file
// in the target directory. The key's top-level segment determines which file to
// look in, and the rest of the path identifies the nested location.
func writeConfigValue(targetDir, key string, value any) error {
	// Ensure directory exists.
	if err := os.MkdirAll(targetDir, 0o755); err != nil {
		return fmt.Errorf("creating config directory: %w", err)
	}

	// Find the file that contains this key.
	filePath, keyPath, err := resolveKeyToFile(targetDir, key)
	if err != nil {
		return err
	}

	// Read existing file content (or start fresh).
	var obj map[string]any
	data, err := os.ReadFile(filePath)
	if err != nil {
		if !os.IsNotExist(err) {
			return fmt.Errorf("reading %s: %w", filePath, err)
		}
		obj = make(map[string]any)
	} else {
		if err := json.Unmarshal(data, &obj); err != nil {
			return fmt.Errorf("parsing %s: %w", filePath, err)
		}
	}

	// Set the nested value.
	setNestedValue(obj, keyPath, value)

	// Write back.
	out, err := json.MarshalIndent(obj, "", "  ")
	if err != nil {
		return fmt.Errorf("marshaling config: %w", err)
	}
	out = append(out, '\n')

	if err := os.WriteFile(filePath, out, 0o644); err != nil {
		return fmt.Errorf("writing %s: %w", filePath, err)
	}

	return nil
}

// resolveKeyToFile finds the JSON file in targetDir that contains the given
// namespaced dot-notation key. The key's prefix identifies which file to target
// (e.g., "pathflow.pathflow-config.version" maps to pathflow/pathflow-config.json).
// Returns the file path and the in-file key path (with namespace prefix stripped).
func resolveKeyToFile(targetDir, key string) (string, []string, error) {
	type candidate struct {
		path       string
		prefix     string
		prefixDots int // number of dot segments in the prefix
	}

	var candidates []candidate

	err := filepath.WalkDir(targetDir, func(path string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() || !strings.HasSuffix(d.Name(), ".json") {
			return err
		}

		prefix := fileNamespacePrefix(targetDir, path)
		if strings.HasPrefix(key, prefix+".") || key == prefix {
			candidates = append(candidates, candidate{
				path:       path,
				prefix:     prefix,
				prefixDots: strings.Count(prefix, ".") + 1,
			})
		}

		return nil
	})

	if err != nil {
		return "", nil, fmt.Errorf("searching config files: %w", err)
	}

	if len(candidates) == 0 {
		// No file matches the namespace prefix. Fall back to first JSON file
		// or create config.json.
		entries, err := os.ReadDir(targetDir)
		if err != nil {
			return "", nil, fmt.Errorf("reading config directory: %w", err)
		}

		parts := strings.Split(key, ".")
		for _, e := range entries {
			if !e.IsDir() && strings.HasSuffix(e.Name(), ".json") {
				return filepath.Join(targetDir, e.Name()), parts, nil
			}
		}

		return filepath.Join(targetDir, "config.json"), parts, nil
	}

	// Use the candidate with the longest prefix (most specific match).
	best := candidates[0]
	for _, c := range candidates[1:] {
		if c.prefixDots > best.prefixDots {
			best = c
		}
	}

	// Strip the namespace prefix to get the in-file key path.
	inFileKey := strings.TrimPrefix(key, best.prefix+".")
	inFileParts := strings.Split(inFileKey, ".")

	// Verify the in-file key actually exists in the file.
	data, err := os.ReadFile(best.path)
	if err != nil {
		return "", nil, fmt.Errorf("reading %s: %w", best.path, err)
	}

	var obj map[string]any
	if err := json.Unmarshal(data, &obj); err != nil {
		return "", nil, fmt.Errorf("parsing %s: %w", best.path, err)
	}

	if containsKeyPath(obj, inFileParts) {
		return best.path, inFileParts, nil
	}

	// Key prefix matched the file namespace but the remaining path wasn't found.
	// Still return this file as the target for new key creation.
	return best.path, inFileParts, nil
}

// containsKeyPath checks whether the nested map contains the given key path.
func containsKeyPath(obj map[string]any, parts []string) bool {
	current := obj
	for i, part := range parts {
		val, ok := current[part]
		if !ok {
			return false
		}
		if i == len(parts)-1 {
			return true
		}
		nested, ok := val.(map[string]any)
		if !ok {
			return false
		}
		current = nested
	}
	return false
}

// setNestedValue sets a value at a dot-notation path in a nested map,
// creating intermediate maps as needed.
func setNestedValue(obj map[string]any, parts []string, value any) {
	current := obj
	for i, part := range parts {
		if i == len(parts)-1 {
			current[part] = value
			return
		}
		next, ok := current[part]
		if !ok {
			next = make(map[string]any)
			current[part] = next
		}
		nested, ok := next.(map[string]any)
		if !ok {
			nested = make(map[string]any)
			current[part] = nested
		}
		current = nested
	}
}

// SortedKeys returns the keys of a map sorted alphabetically.
func SortedKeys(m map[string]any) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
