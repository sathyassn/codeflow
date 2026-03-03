package shadowtest

import (
	"encoding/json"
	"fmt"
	"path/filepath"
)

// AllShadowTests returns all registered shadow test cases with realistic stdin
// payloads. Each test pairs a Go subcommand with its shell script equivalent.
//
// The tests cover:
//   - All 12 critical hooks (security, gate-check, sentinel-write, checkpoint-register,
//     checkpoint-complete, session-start, session-end, edit-write-guard, gh-pr-guard,
//     protection-guard, webfetch-guard, team-guard)
//   - All 5 T1 pathflow scripts (phase-transition, stage-transition, session-register,
//     task-update, session-metadata)
//   - Ledger and validation commands (validate-task, validate-epic)
//
// The projectDir parameter is used to construct absolute paths to shell scripts
// and to set the CF_PROJECT_ROOT environment variable.
func AllShadowTests(projectDir string) []ShadowTest {
	var tests []ShadowTest

	// Add hook shadow tests.
	tests = append(tests, hookShadowTests(projectDir)...)

	// Add pathflow shadow tests.
	tests = append(tests, pathflowShadowTests(projectDir)...)

	// Add validation shadow tests.
	tests = append(tests, validationShadowTests(projectDir)...)

	return tests
}

// hookShadowTests returns shadow tests for all 12 critical hooks.
func hookShadowTests(projectDir string) []ShadowTest {
	testSessionID := "ses-test-shadow-001"
	hookDir := filepath.Join(projectDir, ".claude", "hooks", "codeflow")

	// Hook env: session ID for PathFlow-aware hooks.
	hookEnv := map[string]string{
		"CODEFLOW_SESSION_ID": testSessionID,
		"CF_PROJECT_ROOT":     projectDir,
		"REPO_ROOT":           projectDir,
	}

	return []ShadowTest{
		// 1. Security hook — allows safe Bash command.
		{
			Name:      "hook/security/allow-safe-command",
			GoCommand: []string{"hooks", "pre-tool-use", "security"},
			ShellCommand: []string{
				filepath.Join(hookDir, "pre-tool-use", "cf-pre-tool-use-security.sh"),
			},
			Stdin:            mustMarshal(hookInput("Bash", map[string]interface{}{"command": "ls -la"})),
			Env:              hookEnv,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 2. Security hook — blocks dangerous command.
		{
			Name:      "hook/security/block-dangerous-command",
			GoCommand: []string{"hooks", "pre-tool-use", "security"},
			ShellCommand: []string{
				filepath.Join(hookDir, "pre-tool-use", "cf-pre-tool-use-security.sh"),
			},
			Stdin:            mustMarshal(hookInput("Bash", map[string]interface{}{"command": "rm -rf /"})),
			Env:              hookEnv,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 2,
		},

		// 3. PathFlow gate-check — allows when no PathFlow session active.
		{
			Name:      "hook/gate-check/allow-no-session",
			GoCommand: []string{"hooks", "pre-tool-use", "gate-check"},
			ShellCommand: []string{
				filepath.Join(hookDir, "pre-tool-use", "cf-pre-tool-use-pathflow-gate.sh"),
			},
			Stdin:            mustMarshal(hookInput("Edit", map[string]interface{}{"file_path": "README.md"})),
			Env:              map[string]string{"CF_PROJECT_ROOT": projectDir, "REPO_ROOT": projectDir},
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 4. Sentinel-write — no-op for non-STAGE-COMPLETE message.
		{
			Name:      "hook/sentinel-write/no-op-non-stage-complete",
			GoCommand: []string{"hooks", "post-tool-use", "sentinel-write"},
			ShellCommand: []string{
				filepath.Join(hookDir, "post-tool-use", "cf-post-tool-use-pathflow-sentinel.sh"),
			},
			Stdin: mustMarshal(hookInput("SendMessage", map[string]interface{}{
				"content": "Work complete. Moving to next phase.",
			})),
			Env:              hookEnv,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 5. Checkpoint-register — no-op for non-TaskCreate tool.
		{
			Name:      "hook/checkpoint-register/no-op-non-task-create",
			GoCommand: []string{"hooks", "post-tool-use", "checkpoint-register"},
			ShellCommand: []string{
				filepath.Join(hookDir, "post-tool-use", "cf-post-tool-use-phase-checkpoint.sh"),
			},
			Stdin: mustMarshal(hookInput("Bash", map[string]interface{}{
				"command": "echo done",
			})),
			Env:              hookEnv,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 6. Checkpoint-complete — no-op for non-PathFlow task.
		{
			Name:      "hook/checkpoint-complete/no-op-non-pathflow-task",
			GoCommand: []string{"hooks", "task-completed", "checkpoint-complete"},
			ShellCommand: []string{
				filepath.Join(hookDir, "post-tool-use", "cf-post-tool-use-phase-checkpoint.sh"),
			},
			Stdin: mustMarshal(map[string]interface{}{
				"task_subject": "My regular task",
			}),
			Env:              hookEnv,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 7. Session-start init — processes SessionStart hook stdin.
		// Shell may emit stale-session warnings to stderr; Go does not.
		{
			Name:      "hook/session-start/init",
			GoCommand: []string{"hooks", "session-start", "init"},
			ShellCommand: []string{
				filepath.Join(hookDir, "session-start", "cf-session-start-init.sh"),
			},
			Stdin: mustMarshal(map[string]interface{}{
				"session_id": "claude-uuid-test-001",
				"source":     "startup",
			}),
			Env: map[string]string{
				"CF_PROJECT_ROOT": projectDir,
				"REPO_ROOT":       projectDir,
			},
			Rules:            rules(SessionStartNormalizationRules()),
			ExpectedExitCode: 0,
			KnownDivergence:  "Go emits environment JSON to stdout ({\"env\":{...}}); shell session-start writes session state to files and produces no stdout output.",
		},

		// 8. Session-end cleanup — processes SessionEnd hook stdin.
		// Shell emits "SessionEnd:" status lines to stdout; Go does not.
		{
			Name:      "hook/session-end/cleanup",
			GoCommand: []string{"hooks", "session-end", "cleanup"},
			ShellCommand: []string{
				filepath.Join(hookDir, "session-end", "cf-session-end-cleanup.sh"),
			},
			Stdin: mustMarshal(map[string]interface{}{
				"session_id":      "claude-uuid-test-002",
				"transcript_path": "/tmp/transcript.jsonl",
			}),
			Env: map[string]string{
				"CF_PROJECT_ROOT":     projectDir,
				"CODEFLOW_SESSION_ID": testSessionID,
				"REPO_ROOT":           projectDir,
			},
			Rules:            rules(SessionEndNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 9. Edit-write-guard — allows non-protected file.
		{
			Name:      "hook/edit-write-guard/allow-non-protected",
			GoCommand: []string{"hooks", "pre-tool-use", "protection-guard"},
			ShellCommand: []string{
				filepath.Join(hookDir, "pre-tool-use", "cf-pre-tool-use-edit-write.sh"),
			},
			Stdin:            mustMarshal(hookInput("Edit", map[string]interface{}{"file_path": "README.md"})),
			Env:              hookEnv,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 10. GH-PR-guard — allows non-protected PR command.
		{
			Name:      "hook/gh-pr-guard/allow-non-merge",
			GoCommand: []string{"hooks", "pre-tool-use", "gh-pr-guard"},
			ShellCommand: []string{
				filepath.Join(hookDir, "pre-tool-use", "cf-pre-tool-use-gh-pr.sh"),
			},
			Stdin:            mustMarshal(hookInput("Bash", map[string]interface{}{"command": "gh pr view 42"})),
			Env:              hookEnv,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 11. Protection-guard — allows non-protected file.
		{
			Name:      "hook/protection-guard/allow-unprotected",
			GoCommand: []string{"hooks", "pre-tool-use", "protection-guard"},
			ShellCommand: []string{
				filepath.Join(hookDir, "pre-tool-use", "cf-pre-tool-use-protected-resource.sh"),
			},
			Stdin:            mustMarshal(hookInput("Edit", map[string]interface{}{"file_path": "docs/NOTES.md"})),
			Env:              hookEnv,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 12. Webfetch-guard — allows safe URL.
		// docs.anthropic.com is in the trusted allowlist for Go (reads standard.list via detectProjectDir).
		// Shell loads trusted domains at hook runtime using REPO_ROOT; in test environments the
		// domain resolution path may differ, causing shell to treat the domain as untrusted and
		// block (exit 2) while Go correctly allows it (exit 0).
		{
			Name:      "hook/webfetch-guard/allow-safe-url",
			GoCommand: []string{"hooks", "pre-tool-use", "webfetch-guard"},
			ShellCommand: []string{
				filepath.Join(hookDir, "pre-tool-use", "cf-pre-tool-use-webfetch.sh"),
			},
			Stdin:            mustMarshal(hookInput("WebFetch", map[string]interface{}{"url": "https://docs.anthropic.com/"})),
			Env:              hookEnv,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
			KnownDivergence:  "Go loads trusted-domains/standard.list via detectProjectDir() (git rev-parse); shell loads via REPO_ROOT env. In test environments these may differ, causing shell to block the domain while Go allows it.",
		},

		// 13. Team-guard — allows when no PathFlow session active.
		{
			Name:      "hook/team-guard/allow-no-session",
			GoCommand: []string{"hooks", "pre-tool-use", "team-guard"},
			ShellCommand: []string{
				filepath.Join(hookDir, "pre-tool-use", "cf-pre-tool-use-team-guard.sh"),
			},
			Stdin:            mustMarshal(hookInput("TeamDelete", map[string]interface{}{})),
			Env:              map[string]string{"CF_PROJECT_ROOT": projectDir, "REPO_ROOT": projectDir},
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},
	}
}

// pathflowShadowTests returns shadow tests for all 5 T1 pathflow scripts.
func pathflowShadowTests(projectDir string) []ShadowTest {
	testSessionID := "ses-shadow-test-0001"
	pathflowDir := filepath.Join(projectDir, ".codeflow", "scripts", "pathflow")

	env := map[string]string{
		"CODEFLOW_SESSION_ID": testSessionID,
		"CF_PROJECT_ROOT":     projectDir,
		"REPO_ROOT":           projectDir,
	}

	return []ShadowTest{
		// 1. Phase transition — valid phase and status.
		{
			Name:      "pathflow/phase-transition/entered",
			GoCommand: []string{"pathflow", "phase-transition", "--session-id", testSessionID, "--phase", "PF1-INIT", "--status", "entered"},
			ShellCommand: []string{
				filepath.Join(pathflowDir, "cf-pathflow-phase-transition.sh"),
				"-s", testSessionID, "-p", "PF1-INIT", "-t", "entered",
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 2. Phase transition — invalid phase returns error.
		// Both Go and shell output JSON to stderr and exit 2. OmitStderrFields
		// strips the "error" field value since wording differs between implementations.
		{
			Name:      "pathflow/phase-transition/invalid-phase",
			GoCommand: []string{"pathflow", "phase-transition", "--session-id", testSessionID, "--phase", "PF9-INVALID", "--status", "entered"},
			ShellCommand: []string{
				filepath.Join(pathflowDir, "cf-pathflow-phase-transition.sh"),
				"-s", testSessionID, "-p", "PF9-INVALID", "-t", "entered",
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(PhaseTransitionErrorNormalizationRules()),
			ExpectedExitCode: 2,
		},

		// 3. Stage transition — valid stage and status.
		{
			Name:      "pathflow/stage-transition/in-progress",
			GoCommand: []string{"pathflow", "stage-transition", "--session-id", testSessionID, "--stage", "WS-DEV", "--status", "in_progress", "--iteration", "1"},
			ShellCommand: []string{
				filepath.Join(pathflowDir, "cf-pathflow-stage-transition.sh"),
				"-s", testSessionID, "-g", "WS-DEV", "-t", "in_progress", "-i", "1",
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 4. Stage transition — iteration=0 known divergence (shell allows, Go rejects).
		{
			Name:      "pathflow/stage-transition/iteration-zero-divergence",
			GoCommand: []string{"pathflow", "stage-transition", "--session-id", testSessionID, "--stage", "WS-DEV", "--status", "in_progress", "--iteration", "0"},
			ShellCommand: []string{
				filepath.Join(pathflowDir, "cf-pathflow-stage-transition.sh"),
				"-s", testSessionID, "-g", "WS-DEV", "-t", "in_progress", "-i", "0",
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 2, // Go rejects iteration=0
			KnownDivergence:  "Shell accepts iteration=0; Go requires iteration>=1. This is a known behavioral difference documented in INF-TSK-021-030.",
		},

		// 5. Session register — interactive mode.
		{
			Name:      "pathflow/session-register/interactive",
			GoCommand: []string{"pathflow", "session-register", "--session-id", testSessionID, "--mode", "interactive"},
			ShellCommand: []string{
				filepath.Join(pathflowDir, "cf-pathflow-session-register.sh"),
				"-s", testSessionID, "-m", "interactive",
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(SessionRegisterNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 6. Session register — invalid mode.
		// Both Go and shell output JSON to stderr and exit 2. OmitStderrFields
		// strips the "error" field value since wording differs between implementations.
		{
			Name:      "pathflow/session-register/invalid-mode",
			GoCommand: []string{"pathflow", "session-register", "--session-id", testSessionID, "--mode", "batch"},
			ShellCommand: []string{
				filepath.Join(pathflowDir, "cf-pathflow-session-register.sh"),
				"-s", testSessionID, "-m", "batch",
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(SessionRegisterErrorNormalizationRules()),
			ExpectedExitCode: 2,
		},

		// 7. Task update — valid status.
		{
			Name:      "pathflow/task-update/completed",
			GoCommand: []string{"pathflow", "task-update", "--session-id", testSessionID, "--task-id", "PF3-TSK-01", "--task-status", "completed"},
			ShellCommand: []string{
				filepath.Join(pathflowDir, "cf-pathflow-task-update.sh"),
				"-s", testSessionID, "-k", "PF3-TSK-01", "-t", "completed",
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(TaskUpdateNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 8. Session metadata — records key/value.
		{
			Name:      "pathflow/session-metadata/work-type",
			GoCommand: []string{"pathflow", "session-metadata", "--session-id", testSessionID, "--key", "work_type", "--value", "FEAT"},
			ShellCommand: []string{
				filepath.Join(pathflowDir, "cf-pathflow-session-metadata.sh"),
				"-s", testSessionID, "-k", "work_type", "-v", "FEAT",
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(DefaultNormalizationRules()),
			ExpectedExitCode: 0,
		},
	}
}

// validationShadowTests returns shadow tests for validate-task and validate-epic.
func validationShadowTests(projectDir string) []ShadowTest {
	validationDir := filepath.Join(projectDir, ".codeflow", "scripts", "validation")

	env := map[string]string{
		"CF_PROJECT_ROOT": projectDir,
		"REPO_ROOT":       projectDir,
	}

	// Find a real task file for validation tests.
	realTaskPath := fmt.Sprintf("%s/project-management/epics/INF/INF-EPC-021/tasks/INF-TSK-021-030.md", projectDir)
	realEpicPath := fmt.Sprintf("%s/project-management/epics/INF/INF-EPC-021/INF-EPC-021.md", projectDir)

	return []ShadowTest{
		// 1. Validate task — real task file passes.
		// Shell emits "[INFO] " prefixed lines to stdout; Go does not.
		{
			Name:      "validate/task/real-task-file",
			GoCommand: []string{"validate", "task", realTaskPath},
			ShellCommand: []string{
				filepath.Join(validationDir, "validate-task.sh"),
				realTaskPath,
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(ValidationNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 2. Validate task — nonexistent file fails.
		// Shell emits "[INFO] " prefixed lines to stdout; Go does not.
		{
			Name:      "validate/task/missing-file",
			GoCommand: []string{"validate", "task", "/nonexistent/path/task.md"},
			ShellCommand: []string{
				filepath.Join(validationDir, "validate-task.sh"),
				"/nonexistent/path/task.md",
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(ValidationNormalizationRules()),
			ExpectedExitCode: 1,
		},

		// 3. Validate epic — real epic file passes.
		// Shell emits "[INFO] " prefixed lines to stdout; Go does not.
		{
			Name:      "validate/epic/real-epic-file",
			GoCommand: []string{"validate", "epic", realEpicPath},
			ShellCommand: []string{
				filepath.Join(validationDir, "validate-epic.sh"),
				realEpicPath,
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(ValidationNormalizationRules()),
			ExpectedExitCode: 0,
		},

		// 4. Validate epic — nonexistent file fails.
		// Shell emits "[INFO] " prefixed lines to stdout; Go does not.
		{
			Name:      "validate/epic/missing-file",
			GoCommand: []string{"validate", "epic", "/nonexistent/path/epic.md"},
			ShellCommand: []string{
				filepath.Join(validationDir, "validate-epic.sh"),
				"/nonexistent/path/epic.md",
			},
			Stdin:            []byte{},
			Env:              env,
			Rules:            rules(ValidationNormalizationRules()),
			ExpectedExitCode: 1,
		},
	}
}

// hookInput creates a standard Claude Code hook stdin payload.
func hookInput(toolName string, toolInput map[string]interface{}) map[string]interface{} {
	return map[string]interface{}{
		"tool_name":  toolName,
		"tool_input": toolInput,
	}
}

// mustMarshal marshals v to JSON, panicking on error (for test data only).
func mustMarshal(v interface{}) []byte {
	data, err := json.Marshal(v)
	if err != nil {
		panic(fmt.Sprintf("shadowtest: failed to marshal test data: %v", err))
	}
	return data
}

// rules returns a pointer to a copy of the given NormalizationRules.
func rules(r NormalizationRules) *NormalizationRules {
	return &r
}
