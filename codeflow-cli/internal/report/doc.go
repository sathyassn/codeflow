// Package report generates progress tracking reports from CodeFlow project data.
//
// The [EpicTracker] scans project-management/epics/ on the filesystem, parses
// YAML frontmatter from epic and task markdown files, and produces a summary
// markdown document (epic-tracker.md) with counts, area breakdowns, and
// per-epic task status.
//
// This package is the Go equivalent of cf-tracking-generate.sh, which uses a
// sed-based YAML frontmatter parser. The Go version uses gopkg.in/yaml.v3 for
// reliable structured parsing.
package report
