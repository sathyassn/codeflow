# cf-delegate edit access (delegate tier)

Read this before any write-enabled handoff, on every lane.

A delegate that edits works **only** inside a worktree on a feature branch,
the same worktree-per-session discipline that binds every agent here. Start
the seat in the production and building posture of
[cross-family transport](../../cf-model-orchestrator/resources/routing/transport.md),
with the worktree as its working directory: the Herdr tab's `cwd`
(`cf-herdr`), or `/codex:rescue` scoped to the worktree when the plugin
fallback is in use. Let it commit conventionally.

Never grant edit access on the root checkout or a protected branch. The point
of the doctrine: a delegate's commits pass through **CodeFlow's existing gates
unchanged**: pre-commit secret scan, commit-msg format and no-AI-attribution,
the test gate, and an independent review pass through the primary harness's
normal review gate judge its work exactly as they judge yours. Enforcement is
author-agnostic, so a delegate cannot lower the bar. The git-hook plane is
harness-agnostic by design: the protected-branch merge and ref guards
(`pre-merge-commit`, `reference-transaction`) bind a delegate exactly as they
bind any agent, so it cannot ff-merge, `reset --hard`, or delete a protected
branch. Review the handoff before it ships; a delegate's output is a proposal,
not a merge; a human lands it.
