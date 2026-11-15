---
name: cf-stack
description: Set up or extend the project's stack (test config, lint config, standards). Use when detecting or configuring the build/test/lint mechanics of a repo.
---

# cf-stack: configure stack mechanics

You are configuring stack mechanics, not building features. Stack setup is
judgment work, so it is yours, not the CLI's (CodeFlow ADR-0003): you read the project
and decide; `codeflow` verifies the result deterministically.

1. Detect the stack: manifests (Cargo.toml, package.json, pyproject.toml,
   go.mod, ...), existing CI, and the user's hint. State the discovered stack
   with its evidence, then write the authorized, reversible configuration;
   ask only for an owner choice the evidence leaves unresolved. Adopting a
   new tool, a security policy or wider authority still needs the owner's
   approval; never assume.
2. Build on the project type's standard framework, in proportion.
   - A non-trivial project uses the framework it already has, or the one
     its type treats as standard: a component framework (React, Vue,
     Svelte, Angular) or a server-rendered stack for a web application, a
     static-site or docs generator for documentation, the language's
     standard argument parser for a CLI (clap, Cobra, Click or Typer), the
     ecosystem's usual web framework for a service, and the language's
     standard test framework everywhere.
   - A project is non-trivial when any of these holds: more than one view,
     route or subcommand; state shared between components or kept between
     visits or runs; input that needs validation, such as forms or options;
     data fetched and kept in sync.
   - A one-page tool, a trivial script or a static page with no interaction
     may stay plain: HTML, CSS and JavaScript (web components where reuse
     needs them), or the language's standard library. It has outgrown that
     when one of the conditions above appears, when its own code starts
     rebuilding a router, state store, templating or argument parser, when
     its tests need the harness a framework would give, or when several
     people will maintain it over a long life.
   - A framework the project does not already use is a new dependency:
     name it, its size and supply-chain cost and the plain alternative,
     and get the owner's approval first. Never migrate a working stack for
     this rule alone.
3. Write `.codeflow/test-config.json`. Start from the closest shape among
   codeflow's shipped test-config templates (`codeflow test setup --list-templates`, then
   `--template <name>`), then tailor: real commands for
   `essential` and `full` modes, real coverage tooling or an empty `coverage`
   list. Give a `quick` mode only to fast format and lint targets: the
   pre-push hook runs exactly those and blocks on failure, so the push set
   stays under a minute and the test suite stays in `full`. Root auto-detection is intentionally non-recursive. For a monorepo,
   add one explicit target and `cwd` per package (`--add-target` or the
   multi-target template). If the file exists, extend; do not clobber working
   targets; `--replace` is only for a reviewed, deliberate template reset.
4. Wire the project's tooling. When the project has tooling (scripts,
   build helpers, code generators, release and CI helpers, hook scripts, a
   `scripts/` or `tools/` folder or package), give it a test-executing
   target in `essential` and `full`: its own target with `cwd` when it is
   a separate package, or the existing suite when that suite already runs
   its tests. Point the coverage tool at the tooling source so an untested
   script shows as uncovered (coverage.py `source`, an Istanbul `include`
   list, `go test -coverpkg`). Keep `build.rs`, Makefile and task-runner
   entries thin and test the code they call; test a generator through its
   output with golden files. For a language its coverage tool cannot
   measure, such as shell, run behaviour tests that execute the script
   against fixtures in that target (bats-core, ShellSpec or the project's
   own runner), lint it (ShellCheck for shell), keep branching logic small
   or move it to a measured language, and record the measurement gap with
   an owner, reason and removal condition, as the orchestrator's
   `resources/quality/coverage.md` says. A project with no tooling gets no
   tooling target.
5. Author lint configuration tailored to the project (clippy workspace lints,
   eslint + config, ruff, ...). Respect existing config: tighten or extend,
   never silently replace. Wire the lint command into the test config's `full`
   mode if the project gates on it. Where the existing stack supports a
   compiler or type checker, discover and reuse its accepted command and wire
   it into the appropriate mode. Do not mandate TypeScript, a validator
   library, redundant tooling, stricter compiler migration, or a language/stack
   change; dynamic stacks keep their native checks.
6. Read the orchestrator's `resources/verification-selection.md`. Configure a
   project-owned property/generative, mutation, or architecture fitness command
   only when repository evidence activates its trigger. Put routine checks in
   the appropriate test mode and keep expensive diagnostics explicit and
   bounded. Never install a tool, invent a threshold, or add all three for
   parity. Record `none selected` when no technique is earned.
7. Select deterministic security analysis from the actual stack, trust
   boundaries, hosting, and available rules. Reuse a working analyzer already
   owned by the project. Otherwise compare the language's native analyzer,
   CodeQL for an eligible GitHub repository, Semgrep or another maintained
   SAST/taint route, and `none selected: residual risk <reason>`. A remote
   service records its CI check and ownership; a local-capable analyzer uses a
   reviewed target in `.codeflow/test-config.json` at the mode justified by its
   cost. Never auto-install a scanner, assume entitlement, enable every vendor,
   or claim SCA/Clippy is source-to-sink taint analysis.
8. Append a `## Stack standards` section to `AGENTS.md` OUTSIDE the
   codeflow:managed markers (the managed block is replaced on update): the
   chosen stack, test/lint/security commands, and any conventions agreed with
   the user. Keep it under ~15 lines.
9. Verify deterministically and show the evidence: `codeflow test` (targets
   resolve and run), `codeflow doctor` (wiring healthy). Fix what fails before
   reporting done; say what was not verified. Commit the config in small units;
   when a remote is configured, push the branch for durability: backup, not a
   merge.
