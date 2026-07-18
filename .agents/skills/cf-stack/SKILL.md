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
   mode if the project gates on it.
4. Append a `## Stack standards` section to `AGENTS.md` OUTSIDE the
   codeflow:managed markers (the managed block is replaced on update): the
   chosen stack, test/lint commands, and any conventions agreed with the user.
   Keep it under ~15 lines.
5. Verify deterministically and show the evidence: `codeflow test` (targets
   resolve and run), `codeflow doctor` (wiring healthy). Fix what fails before
   reporting done; say what was not verified. Commit the config in small units;
   when a remote is configured, push the branch for durability — backup, not a
   merge.
