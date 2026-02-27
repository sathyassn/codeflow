package autorun

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"gopkg.in/yaml.v3"
)

// Sentinel errors for batch file parsing.
var (
	// ErrInvalidBatch indicates the batch file has invalid structure.
	ErrInvalidBatch = errors.New("autorun: invalid batch file")

	// ErrDependencyCycle indicates a dependency cycle was detected.
	ErrDependencyCycle = errors.New("autorun: dependency cycle detected")

	// ErrMissingTask indicates a referenced task does not exist.
	ErrMissingTask = errors.New("autorun: referenced task not found")

	// ErrProtectedMerge indicates auto_merge targets a protected branch.
	ErrProtectedMerge = errors.New("autorun: auto_merge not allowed for protected branches")
)

// protectedBranches lists branches that cannot be auto-merged into.
var protectedBranches = []string{"main", "master", "production"}

// BatchFile represents the top-level structure of a YAML batch file.
type BatchFile struct {
	Name       string     `yaml:"name"`
	MaxWorkers int        `yaml:"max_workers"`
	AutoMerge  bool       `yaml:"auto_merge"`
	Target     string     `yaml:"target"`
	Tasks      []TaskSpec `yaml:"tasks"`
}

// TaskSpec represents a single task specification in a batch file.
type TaskSpec struct {
	ID        string   `yaml:"id"`
	DependsOn []string `yaml:"depends_on"`
}

// ParsedBatch contains the validated and resolved batch specification.
type ParsedBatch struct {
	Name       string
	FilePath   string
	MaxWorkers int
	AutoMerge  bool
	Target     string
	Tasks      []TaskSpec
	Order      []string // topologically sorted task IDs
}

// ParseBatchFile reads and validates a YAML batch file.
func ParseBatchFile(path string) (*ParsedBatch, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("autorun: reading batch file %s: %w", path, err)
	}

	return parseBatchData(data, path)
}

// parseBatchData parses and validates batch YAML data.
func parseBatchData(data []byte, filePath string) (*ParsedBatch, error) {
	var bf BatchFile
	if err := yaml.Unmarshal(data, &bf); err != nil {
		return nil, fmt.Errorf("%w: YAML syntax error: %w", ErrInvalidBatch, err)
	}

	if err := validateBatch(&bf); err != nil {
		return nil, err
	}

	order, err := topologicalSort(bf.Tasks)
	if err != nil {
		return nil, err
	}

	// Default max_workers to 3 if not specified or zero.
	maxWorkers := bf.MaxWorkers
	if maxWorkers <= 0 {
		maxWorkers = DefaultMaxWorkers
	}

	// Default name from filename if not specified.
	name := bf.Name
	if name == "" {
		base := filepath.Base(filePath)
		name = strings.TrimSuffix(base, filepath.Ext(base))
	}

	return &ParsedBatch{
		Name:       name,
		FilePath:   filePath,
		MaxWorkers: maxWorkers,
		AutoMerge:  bf.AutoMerge,
		Target:     bf.Target,
		Tasks:      bf.Tasks,
		Order:      order,
	}, nil
}

// validateBatch checks a batch file for structural and semantic errors.
func validateBatch(bf *BatchFile) error {
	if len(bf.Tasks) == 0 {
		return fmt.Errorf("%w: no tasks defined", ErrInvalidBatch)
	}

	// Build a set of known task IDs for reference validation.
	taskIDs := make(map[string]bool, len(bf.Tasks))
	for _, t := range bf.Tasks {
		if t.ID == "" {
			return fmt.Errorf("%w: task has empty id", ErrInvalidBatch)
		}
		if taskIDs[t.ID] {
			return fmt.Errorf("%w: duplicate task id %q", ErrInvalidBatch, t.ID)
		}
		taskIDs[t.ID] = true
	}

	// Validate dependency references.
	for _, t := range bf.Tasks {
		for _, dep := range t.DependsOn {
			if !taskIDs[dep] {
				return fmt.Errorf("%w: task %q depends on unknown task %q", ErrMissingTask, t.ID, dep)
			}
			if dep == t.ID {
				return fmt.Errorf("%w: task %q depends on itself", ErrDependencyCycle, t.ID)
			}
		}
	}

	// Validate auto_merge + protected branch constraint (D4).
	if bf.AutoMerge {
		target := bf.Target
		if target == "" {
			target = "main"
		}
		for _, protected := range protectedBranches {
			if target == protected {
				return fmt.Errorf("%w: cannot auto_merge into %q", ErrProtectedMerge, target)
			}
			// Check release/* pattern.
			if strings.HasPrefix(target, "release/") {
				return fmt.Errorf("%w: cannot auto_merge into %q", ErrProtectedMerge, target)
			}
		}
	}

	return nil
}

// topologicalSort performs Kahn's algorithm for dependency resolution.
// Returns task IDs in execution order (dependencies first).
func topologicalSort(tasks []TaskSpec) ([]string, error) {
	// Build adjacency list and in-degree map.
	inDegree := make(map[string]int, len(tasks))
	dependents := make(map[string][]string, len(tasks))

	for _, t := range tasks {
		if _, ok := inDegree[t.ID]; !ok {
			inDegree[t.ID] = 0
		}
		for _, dep := range t.DependsOn {
			dependents[dep] = append(dependents[dep], t.ID)
			inDegree[t.ID]++
		}
	}

	// Seed the queue with nodes having zero in-degree.
	var queue []string
	for _, t := range tasks {
		if inDegree[t.ID] == 0 {
			queue = append(queue, t.ID)
		}
	}
	// Sort for deterministic ordering.
	sort.Strings(queue)

	var order []string
	for len(queue) > 0 {
		node := queue[0]
		queue = queue[1:]
		order = append(order, node)

		deps := dependents[node]
		sort.Strings(deps) // deterministic
		for _, dep := range deps {
			inDegree[dep]--
			if inDegree[dep] == 0 {
				queue = append(queue, dep)
			}
		}
	}

	if len(order) != len(tasks) {
		// Find which tasks are in the cycle for a helpful error message.
		var cycled []string
		for id, deg := range inDegree {
			if deg > 0 {
				cycled = append(cycled, id)
			}
		}
		sort.Strings(cycled)
		return nil, fmt.Errorf("%w: tasks involved: %s", ErrDependencyCycle, strings.Join(cycled, ", "))
	}

	return order, nil
}
