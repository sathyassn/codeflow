package main

import (
	"encoding/json"
	"fmt"
	"os/exec"
	"runtime"
	"runtime/debug"
	"strings"

	"github.com/spf13/cobra"
)

// versionInfo holds structured version information for JSON output.
type versionInfo struct {
	CodeflowVersion   string `json:"codeflow_version"`
	VCSRevision       string `json:"vcs_revision,omitempty"`
	VCSTime           string `json:"vcs_time,omitempty"`
	ClaudeCodeVersion string `json:"claude_code_version"`
	GoVersion         string `json:"go_version"`
}

// vcsInfo holds VCS revision and build time extracted from Go build info.
type vcsInfo struct {
	Revision  string
	BuildTime string
}

// readBuildVCS extracts VCS metadata from the Go binary's embedded build info.
// Overridden in tests.
var readBuildVCS = func() *vcsInfo {
	info, ok := debug.ReadBuildInfo()
	if !ok {
		return nil
	}
	var revision, buildTime string
	for _, s := range info.Settings {
		switch s.Key {
		case "vcs.revision":
			revision = s.Value
			if len(revision) > 7 {
				revision = revision[:7]
			}
		case "vcs.time":
			buildTime = s.Value
		}
	}
	if revision == "" {
		return nil
	}
	return &vcsInfo{Revision: revision, BuildTime: buildTime}
}

// newVersionCmd creates the version subcommand that prints the CLI version.
func newVersionCmd() *cobra.Command {
	var (
		jsonOutput bool
		check      bool
	)

	cmd := &cobra.Command{
		Use:   "version",
		Short: "Print the codeflow version",
		Long:  "Print the codeflow version, optionally with Claude Code version and Go runtime version.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runVersion(cmd, jsonOutput, check)
		},
	}

	cmd.Flags().BoolVar(&jsonOutput, "json", false, "output version information as JSON")
	cmd.Flags().BoolVar(&check, "check", false, "check if a newer version is available")

	return cmd
}

// runVersion implements the version command logic.
func runVersion(cmd *cobra.Command, jsonOutput, check bool) error {
	if check {
		fmt.Fprintln(cmd.OutOrStdout(), "Update check not yet implemented. Current version: "+version)
		return nil
	}

	vcs := readBuildVCS()

	if jsonOutput {
		info := versionInfo{
			CodeflowVersion:   version,
			ClaudeCodeVersion: detectClaudeCodeVersion(),
			GoVersion:         runtime.Version(),
		}
		if vcs != nil {
			info.VCSRevision = vcs.Revision
			info.VCSTime = vcs.BuildTime
		}
		data, err := json.MarshalIndent(info, "", "  ")
		if err != nil {
			return fmt.Errorf("marshalling version info: %w", err)
		}
		fmt.Fprintln(cmd.OutOrStdout(), string(data))
		return nil
	}

	// Default: plain text output with optional VCS build info.
	if vcs != nil {
		fmt.Fprintf(cmd.OutOrStdout(), "codeflow %s (%s %s)\n", version, vcs.Revision, vcs.BuildTime)
	} else {
		fmt.Fprintf(cmd.OutOrStdout(), "codeflow %s\n", version)
	}
	return nil
}

// detectClaudeCodeVersion runs "claude --version" and parses the output.
// Returns "unknown" if the command fails or is not found.
func detectClaudeCodeVersion() string {
	return detectClaudeCodeVersionWith(defaultClaudeRunner)
}

// claudeRunner is a function type that runs the claude --version command.
// It enables testing without requiring the claude binary.
type claudeRunner func() ([]byte, error)

// defaultClaudeRunner executes "claude --version" using os/exec.
func defaultClaudeRunner() ([]byte, error) {
	return exec.Command("claude", "--version").Output()
}

// detectClaudeCodeVersionWith runs the given claude runner and parses the output.
func detectClaudeCodeVersionWith(runner claudeRunner) string {
	out, err := runner()
	if err != nil {
		return "unknown"
	}
	trimmed := strings.TrimSpace(string(out))
	if trimmed == "" {
		return "unknown"
	}
	return trimmed
}
