// Package resource provides the protection-guard hook for CodeFlow.
//
// It checks whether a file path targets a protected resource (critical, high,
// or moderate tier) per enforcement-policy.json and handles the staging
// workflow for protected edits. This consolidates the shell scripts:
// cf-pre-tool-use-protected-resource.sh, cf-post-tool-use-tmp-workflow.sh,
// and 5 dormant staging scripts into a single Go implementation.
package resource
