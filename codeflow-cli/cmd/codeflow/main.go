// Package main provides the codeflow CLI entry point.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	autorunCmd "github.com/codeflow/codeflow-cli/cmd/autorun"
	"github.com/codeflow/codeflow-cli/internal/welcome"
	"github.com/spf13/cobra"
)

// version is set at build time via -ldflags "-X main.version=$(VERSION)".
var version = "dev"

// newRootCmd creates the root cobra command with all subcommands registered.
func newRootCmd() *cobra.Command {
	rootCmd := &cobra.Command{
		Use:   "codeflow",
		Short: "AI-native development framework CLI",
		Long:  "CodeFlow CLI provides tools for AI-native development workflows.",
		// Show help when no subcommand is given.
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
		// Disable cobra's default completion command.
		CompletionOptions: cobra.CompletionOptions{
			DisableDefaultCmd: true,
		},
		// Silence cobra's built-in error/usage printing so run() controls output.
		SilenceErrors: true,
		SilenceUsage:  true,
	}

	rootCmd.Version = version
	rootCmd.SetVersionTemplate("codeflow {{.Version}}\n")

	rootCmd.AddCommand(newVersionCmd())
	rootCmd.AddCommand(newUninstallCmd())
	rootCmd.AddCommand(newDBCmd())
	rootCmd.AddCommand(newSessionCmd())
	rootCmd.AddCommand(newInitCmd())
	rootCmd.AddCommand(newDoctorCmd())
	rootCmd.AddCommand(newConfigCmd())
	rootCmd.AddCommand(newUpdateCmd())
	rootCmd.AddCommand(autorunCmd.NewCmd())
	rootCmd.AddCommand(newLedgerCmd())
	rootCmd.AddCommand(newWelcomeCmd())
	rootCmd.AddCommand(newInternalCmd())
	rootCmd.AddCommand(newPathflowCmd())
	rootCmd.AddCommand(newStateCmd())

	return rootCmd
}

func main() {
	ctx := context.Background()
	if err := run(ctx); err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(exitCode(err))
	}
}

// run executes the CLI logic. Accepts context.Context for cancellation
// and deadline propagation to subcommands.
func run(ctx context.Context) error {
	return newRootCmd().ExecuteContext(ctx)
}

// newWelcomeCmd creates the "welcome" command that displays the welcome screen.
func newWelcomeCmd() *cobra.Command {
	var quiet bool

	cmd := &cobra.Command{
		Use:   "welcome",
		Short: "Display the CodeFlow welcome screen",
		Long:  "Display the welcome screen with project status, active work, PathFlow state, and team info.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runWelcome(cmd, quiet)
		},
	}

	cmd.Flags().BoolVarP(&quiet, "quiet", "q", false, "suppress welcome screen output")
	return cmd
}

// runWelcome builds welcome options from environment/filesystem and renders.
func runWelcome(cmd *cobra.Command, quiet bool) error {
	opts := []welcome.Option{
		welcome.WithVersion(version),
		welcome.WithClaudeCodeVersion(detectClaudeCodeVersion()),
		welcome.WithQuiet(quiet),
	}

	// Read PathFlow state from environment and filesystem.
	if pfOpt := readPathFlowState(); pfOpt != nil {
		opts = append(opts, pfOpt)
	}

	// Read team state.
	if teamOpt := readTeamState(); teamOpt != nil {
		opts = append(opts, teamOpt)
	}

	welcome.Show(cmd.OutOrStdout(), opts...)
	return nil
}

// readPathFlowState checks if a PathFlow session is active and returns
// a WithPathFlow option, or nil if no active session.
func readPathFlowState() welcome.Option {
	sid := os.Getenv("CODEFLOW_SESSION_ID")
	if sid == "" {
		return nil
	}

	// Check pathflow-active flag.
	flagPath := filepath.Join(".state", "session", sid, "pathflow", "is-pathflow-active")
	if _, err := os.Stat(flagPath); err != nil {
		return nil
	}

	phase := detectPhase(sid)
	stage := detectStage(sid)
	rework := detectReworkCount(sid)

	return welcome.WithPathFlow(phase, stage, rework)
}

// detectPhase reads sentinel files to determine the current PathFlow phase.
func detectPhase(sid string) string {
	sentinelDir := filepath.Join(".state", "sentinels", "pathflow", sid)
	entries, err := os.ReadDir(sentinelDir)
	if err != nil {
		return "PF1-INIT"
	}

	// Find the highest pf-N sentinel.
	highestPhase := 0
	for _, e := range entries {
		name := e.Name()
		if strings.HasPrefix(name, "pathflow-pf-") {
			var n int
			if _, err := fmt.Sscanf(name, "pathflow-pf-%d", &n); err == nil && n > highestPhase {
				highestPhase = n
			}
		}
	}

	phaseNames := map[int]string{
		1: "PF1-INIT",
		2: "PF2-CONTEXT",
		3: "PF3-CLASSIFY",
		4: "PF4-EXECUTE",
		5: "PF5-VERIFY",
		6: "PF6-COMPLETE",
		7: "PF7-END",
	}

	// The current phase is the one AFTER the highest completed sentinel.
	nextPhase := highestPhase + 1
	if name, ok := phaseNames[nextPhase]; ok {
		return name
	}
	if name, ok := phaseNames[highestPhase]; ok {
		return name
	}
	return "PF1-INIT"
}

// detectStage reads sentinel files to determine the current work stage.
func detectStage(sid string) string {
	sentinelDir := filepath.Join(".state", "sentinels", "pathflow", sid)
	entries, err := os.ReadDir(sentinelDir)
	if err != nil {
		return ""
	}

	// Collect completed stage sentinels.
	stageOrder := []string{"ws-dev", "ws-plan", "ws-docs", "ws-test", "ws-rev", "ws-qa"}
	completed := make(map[string]bool)
	for _, e := range entries {
		name := e.Name()
		if strings.HasPrefix(name, "pathflow-ws-") {
			stage := strings.TrimPrefix(name, "pathflow-")
			completed[stage] = true
		}
	}

	// The current stage is the first one NOT completed.
	for _, s := range stageOrder {
		if !completed[s] {
			return strings.ToUpper(s)
		}
	}
	return ""
}

// detectReworkCount parses pathflow-events.jsonl for the highest rework
// iteration in the current session.
func detectReworkCount(sid string) string {
	eventsPath := filepath.Join(".state", "logs", "pathflow-events.jsonl")
	f, err := os.Open(eventsPath)
	if err != nil {
		return "0"
	}
	defer f.Close()

	maxIteration := 0
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		var evt struct {
			Event           string `json:"event"`
			SessionID       string `json:"session_id"`
			Verdict         string `json:"verdict"`
			Iteration       int    `json:"iteration"`
			ReworkIteration int    `json:"rework_iteration"`
		}
		if err := json.Unmarshal(scanner.Bytes(), &evt); err != nil {
			continue
		}
		if evt.SessionID != sid {
			continue
		}
		if evt.Event != "stage_transition" || evt.Verdict != "changes_requested" {
			continue
		}
		iter := evt.Iteration
		if evt.ReworkIteration > iter {
			iter = evt.ReworkIteration
		}
		if iter > maxIteration {
			maxIteration = iter
		}
	}

	return fmt.Sprintf("%d", maxIteration)
}

// readTeamState reads team information from the session state.
func readTeamState() welcome.Option {
	sid := os.Getenv("CODEFLOW_SESSION_ID")
	if sid == "" {
		return nil
	}

	// Primary source: session-scoped pathflow-team.json.
	teamPath := filepath.Join(".state", "session", sid, "pathflow", "pathflow-team.json")
	data, err := os.ReadFile(teamPath)
	if err == nil {
		var info struct {
			TeamName string `json:"team_name"`
		}
		if json.Unmarshal(data, &info) == nil && info.TeamName != "" {
			teammates := readTeammates(info.TeamName)
			return welcome.WithTeam(info.TeamName, teammates)
		}
	}

	return nil
}

// readTeammates reads active teammate names from team config.
func readTeammates(teamName string) []string {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil
	}

	configPath := filepath.Join(home, ".claude", "teams", teamName, "config.json")
	data, err := os.ReadFile(configPath)
	if err != nil {
		return nil
	}

	var config struct {
		Teammates map[string]struct {
			Name string `json:"name"`
		} `json:"teammates"`
	}
	if err := json.Unmarshal(data, &config); err != nil {
		return nil
	}

	var names []string
	for _, t := range config.Teammates {
		if t.Name != "" {
			names = append(names, t.Name)
		}
	}
	sort.Strings(names)
	return names
}
