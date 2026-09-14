---
name: cf-stack
description: Set up or extend the project's stack — test config, lint config, standards. Use when detecting or configuring the build/test/lint mechanics of a repo.
---

# cf-stack — configure stack mechanics

You are configuring stack mechanics, not building features. Stack setup is
judgment work, so it is yours, not the CLI's (CodeFlow ADR-0003): you read the project
and decide; `codeflow` verifies the result deterministically.

1. Detect the stack: manifests (Cargo.toml, package.json, pyproject.toml,
   go.mod, ...), existing CI, and the user's hint. State what you found and
   confirm with the user before writing anything — never assume.
2. Write `.codeflow/test-config.json`. Start from the closest shape among
   codeflow's shipped test-config templates (`codeflow test setup --list-templates`, then
   `--template <name>`), then tailor: real commands for
   `essential` and `full` modes, real coverage tooling or an empty `coverage`
   list. Root auto-detection is intentionally non-recursive. For a monorepo,
   add one explicit target and `cwd` per package (`--add-target` or the
   multi-target template). If the file exists, extend — do not clobber working
   targets; `--replace` is only for a reviewed, deliberate template reset.
3. Author lint configuration tailored to the project (clippy workspace lints,
   eslint + config, ruff, ...). Respect existing config: tighten or extend,
   never silently replace. Wire the lint command into the test config's `full`
   mode if the project gates on it. Where the existing stack supports a
   compiler or type checker, discover and reuse its accepted command and wire
   it into the appropriate mode. Do not mandate TypeScript, a validator
   library, redundant tooling, stricter compiler migration, or a language/stack
   change; dynamic stacks keep their native checks.
4. Read the orchestrator's `resources/verification-selection.md`. Configure a
   project-owned property/generative, mutation, or architecture fitness command
   only when repository evidence activates its trigger. Put routine checks in
   the appropriate test mode and keep expensive diagnostics explicit and
   bounded. Never install a tool, invent a threshold, or add all three for
   parity. Record `none selected` when no technique is earned.
5. Select deterministic security analysis from the actual stack, trust
   boundaries, hosting, and available rules. Reuse a working analyzer already
   owned by the project. Otherwise compare the language's native analyzer,
   CodeQL for an eligible GitHub repository, Semgrep or another maintained
   SAST/taint route, and `none selected — residual risk <reason>`. A remote
   service records its CI check and ownership; a local-capable analyzer uses a
   reviewed target in `.codeflow/test-config.json` at the mode justified by its
   cost. Never auto-install a scanner, assume entitlement, enable every vendor,
   or claim SCA/Clippy is source-to-sink taint analysis.
6. Append a `## Stack standards` section to `AGENTS.md` OUTSIDE the
   codeflow:managed markers (the managed block is replaced on update): the
   chosen stack, test/lint/security commands, and any conventions agreed with
   the user. Keep it under ~15 lines.
7. Verify deterministically and show the evidence: `codeflow test` (targets
   resolve and run), `codeflow doctor` (wiring healthy). Fix what fails before
   reporting done; say what was not verified. Commit the config in small units;
   when a remote is configured, push the branch for durability — backup, not a
   merge.
