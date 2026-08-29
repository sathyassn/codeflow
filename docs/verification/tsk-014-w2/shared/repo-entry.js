// Shared subject source for the D1 candidates: what must already be true before
// a task starts on this repository, and which gate refuses you.
//
// Facts, relationships, verbatim quotations and derivation only. No markup, no
// CSS, no visual primitive, no token. All three D1 candidates read this one
// object, so they cannot disagree about the subject.
//
// tools/verify.mjs binds every fact here to the repository:
//   - every `quote` against the file it declares, whitespace-normalised;
//   - every gate `rule` against the source that emits it;
//   - the shipped/building capability counts against docs/capabilities.md;
//   - the derived answer against an independent recomputation.
//
// The preconditions are not a summary of prose. They are the `WorkStartError`
// variants of crates/codeflow-core/src/workgraph/work_start.rs, in the order the
// preflight evaluates them: the enum IS the precondition list.
window.repoEntry = (() => {
  const AGENTS = "AGENTS.md";
  const PRODUCT = "docs/product.md";
  const WORKSTART = "crates/codeflow-core/src/workgraph/work_start.rs";
  const CI = "crates/codeflow-cli/src/cmd/ci.rs";
  const HOOK = "crates/codeflow-cli/src/cmd/git_hook.rs";
  const GITHOOK = "crates/codeflow-core/src/hooks/git_hook.rs";
  const GUARD = "crates/codeflow-core/src/hooks/git_guard.rs";

  // ------------------------------------------------------------------ planes
  // `short` is the narrow-viewport label. Every word of it must also appear in
  // `name`, so an abbreviation can never introduce a term the plane is not
  // actually called; `tools/verify.mjs` enforces that.
  const planes = [
    {
      id: "work-start", name: "codeflow work start", short: "work start", kind: "preflight", armedHere: true,
      gist: "the developer runs it; read-only, mutates nothing",
      quote: {
        text: "Before product edits on `task/TSK-NNN-<slug>`, run `codeflow work start TSK-NNN`. It proves from the merge-base that the task, matching non-task target declaration, parent or standalone rationale, approved specs, and completed dependencies are anchored. The CLI, pre-commit, and CI share this read-only rule.",
        source: AGENTS,
      },
    },
    {
      id: "git-guard", name: "git-guard", short: "git-guard", kind: "in-session", armedHere: true,
      gist: "PreToolUse; stops the command before it runs",
      quote: {
        text: "The PreToolUse plane is per-harness: Claude Code always; interactive codex after the one-time `/hooks` trust; a harness with no hooks engine not at all",
        source: AGENTS,
      },
    },
    {
      id: "git-hooks", name: "git hooks", short: "git hooks", kind: "local", armedHere: true,
      gist: "pre-commit · commit-msg · pre-merge-commit · pre-push · reference-transaction",
      quote: { text: "The local planes are fast in-session feedback", source: AGENTS },
    },
    {
      id: "ci", name: "codeflow ci", short: "codeflow ci", kind: "authoritative", armedHere: true,
      gist: "server-side; the same rules, run where they cannot be skipped",
      quote: {
        text: "CI and remote branch protection are the authoritative, server-enforced perimeter — the real boundary",
        source: AGENTS,
      },
    },
    {
      id: "remote", name: "remote branch protection", short: "remote protection", kind: "authoritative", armedHere: false,
      gist: "not armed on this repository",
      quote: {
        text: "**Remote branch protection is unavailable here and not pursued** (private + GitHub Free → `codeflow remote protect` returns 403). The operative boundary on this repo is server-side CI plus the local git-hook / git-guard plane plus human-merged PRs — not armed remote protection.",
        source: AGENTS,
      },
    },
  ];

  // ------------------------------------------------------------- concepts
  // `needs` is a real reading dependency: the later concept cannot be
  // understood before the earlier one, not merely "comes after it".
  const concepts = [
    {
      id: "C1", name: "what the binary is for", needs: [],
      gist: "a discipline layer installed into a repo — it does not develop, and it gates only what is irreversible or invisible.",
      why: "every rule below reads as bureaucracy until this is in place.",
      quote: {
        text: "CodeFlow is the AI-development discipline layer you install into any repo: one Rust binary (`codeflow`) plus an embedded scaffold that scaffolds, enforces, verifies, and remembers — while Claude Code (or any harness) does the developing.",
        source: PRODUCT,
      },
    },
    {
      id: "C2", name: "the six layers", needs: ["C1"],
      gist: "why, rules, what, how, work, trace — each with one home and one change rate.",
      why: "you cannot tell which file answers a question until you know the layers.",
      quote: {
        text: "The traceability spine runs downward: capability → epic/task → ADR/spec → PR → ledger.",
        source: AGENTS,
      },
    },
    {
      id: "C3", name: "durable records", needs: ["C2"],
      gist: "flat stable-ID Markdown records — epics, specs, tasks — whose relationships live in frontmatter.",
      why: "the preflight reads these fields; without them its errors are unreadable.",
      quote: {
        text: "One system owns each work item's status and acceptance; trackers and external planning methods are links, never mirrors.",
        source: AGENTS,
      },
    },
    {
      id: "C4", name: "planning lands before work", needs: ["C3"],
      gist: "planning happens on a `plan/` branch and must reach the task's declared integration target first.",
      why: "this is the rule the anchor enforces; the anchor is meaningless without it.",
      quote: {
        text: "Its validated planning PR must reach the task's `integration_target` before implementation starts; do not invent a task record on the implementation branch.",
        source: AGENTS,
      },
    },
    {
      id: "C5", name: "the merge-base anchor", needs: ["C4"],
      gist: "the check does not read your working tree. It reads the merge-base between your branch and the target.",
      why: "this is the one idea that makes every refusal below predictable.",
      quote: { text: "Git history is the planning seal.", source: WORKSTART },
    },
    {
      id: "C6", name: "the target cannot be yours", needs: ["C5"],
      gist: "the target must resolve to a real branch, and a task branch can never be one.",
      why: "otherwise a branch could authorize itself and the anchor would prove nothing.",
      quote: {
        text: "A task branch is never a valid integration target and cannot authorize its own planning record. The target must resolve to a real local or remote-tracking branch, never `HEAD`, a tag, an object ID, or another revision expression.",
        source: AGENTS,
      },
    },
    {
      id: "C7", name: "one check, three planes", needs: ["C6"],
      gist: "the same read-only preflight runs in `codeflow work start`, in `pre-commit`, and in `codeflow ci`.",
      why: "which plane caught you changes nothing about what is wrong.",
      quote: {
        text: "read-only merge-base preflight as `codeflow work start` and pre-commit.",
        source: CI,
      },
    },
    {
      id: "C8", name: "where the boundary actually is", needs: ["C7"],
      gist: "local planes are feedback; CI is the perimeter. On this repository remote protection is not armed at all.",
      why: "mistaking feedback for a boundary is how work gets laundered past a gate.",
      quote: {
        text: "gates exist only where a mistake is irreversible or invisible",
        source: PRODUCT,
      },
    },
  ];

  // -------------------------------------------------------- preconditions
  // `order` is the evaluation order of the real preflight, not a ranking.
  const preconditions = [
    {
      id: "E1", order: 1, concept: "C1",
      must: "the branch prefix is one the policy allows",
      rule: "git.branch_naming", planes: ["git-hooks", "ci"],
      error: {
        text: "Prefixes: `feat/ fix/ docs/ refactor/ test/ chore/ ci/ hotfix/ plan/ task/ spike/ experiment/ integration/`.",
        source: AGENTS,
      },
    },
    {
      id: "E2", order: 2, concept: "C3",
      must: "the branch is `task/TSK-NNN-<slug>` and resolves to a task record visible in the checkout",
      rule: "work.task_record", planes: ["work-start", "git-hooks", "ci"],
      error: {
        text: "task branch '{branch}' does not identify a visible task record",
        source: CI,
      },
    },
    {
      id: "E3", order: 3, concept: "C3",
      must: "the visible durable workgraph validates",
      rule: "work.valid_graph", planes: ["git-hooks", "ci"],
      error: {
        text: "repair the workgraph until `codeflow validate --docs` passes",
        source: CI,
      },
    },
    {
      id: "E4", order: 4, concept: "C6",
      must: "the declared `integration_target` is a stable, resolvable, non-task branch and matches the record",
      rule: "work.stable_planning_anchor", planes: ["work-start", "git-hooks", "ci"],
      error: {
        text: "target '{0}' is not a stable local or remote-tracking non-task branch",
        source: WORKSTART,
      },
      also: [
        { text: "task {task_id} targets '{declared}', not requested target '{requested}'", source: WORKSTART },
        { text: "task {0} has no integration_target", source: WORKSTART },
      ],
    },
    {
      id: "E5", order: 5, concept: "C5",
      must: "the task record already exists at the merge-base with that target",
      rule: "work.stable_planning_anchor", planes: ["work-start", "git-hooks", "ci"],
      error: {
        text: "task {0} is not present at the merge-base; merge its planning record before implementation",
        source: WORKSTART,
      },
    },
    {
      id: "E6", order: 6, concept: "C3",
      must: "its anchored status is `todo` or `in_progress`",
      rule: "work.stable_planning_anchor", planes: ["work-start", "git-hooks", "ci"],
      error: {
        text: "task {task_id} cannot start from status '{status}'; expected todo or in_progress",
        source: WORKSTART,
      },
    },
    {
      id: "E7", order: 7, concept: "C3",
      must: "its epic resolves at the merge-base, or it carries a `standalone_reason`",
      rule: "work.stable_planning_anchor", planes: ["work-start", "git-hooks", "ci"],
      error: {
        text: "task {task_id} references missing epic {epic_id} at the merge-base",
        source: WORKSTART,
      },
      also: [{ text: "task {0} is standalone but has no standalone_reason", source: WORKSTART }],
    },
    {
      id: "E8", order: 8, concept: "C4",
      must: "every spec it and its epic require is anchored and `approved` or `implemented`",
      rule: "work.stable_planning_anchor", planes: ["work-start", "git-hooks", "ci"],
      error: {
        text: "task {task_id} requires spec {spec_id}, whose anchored status is '{status}', not approved or implemented",
        source: WORKSTART,
      },
      also: [{ text: "task {task_id} requires missing spec {spec_id} at the merge-base", source: WORKSTART }],
    },
    {
      id: "E9", order: 9, concept: "C4",
      must: "every dependency is anchored and `complete`",
      rule: "work.stable_planning_anchor", planes: ["work-start", "git-hooks", "ci"],
      error: {
        text: "task {task_id} depends on {dependency}, whose anchored status is '{status}', not complete",
        source: WORKSTART,
      },
      also: [{ text: "task {task_id} depends on missing task {dependency} at the merge-base", source: WORKSTART }],
    },
  ];

  // What refuses the *mutation* rather than the start. Not preconditions of
  // starting; they are why the runway does not end at the first commit.
  const mutationGates = [
    {
      rule: "git.secret_scan", planes: ["git-hooks", "ci"], step: "commit",
      refuses: "a staged credential, key, token or `.env`",
      note: "the one gate never relaxed",
      quote: {
        text: "The pre-commit secret scan (and the CI secret-scan job) block them, and it is the one gate never relaxed",
        source: AGENTS,
      },
      source: GITHOOK,
    },
    {
      rule: "git.commit_format", planes: ["git-hooks", "ci"], step: "commit",
      refuses: "a subject that is not `type(scope): description`, over 50 characters, or ends in a period",
      source: GITHOOK,
    },
    {
      rule: "git.commit_body", planes: ["git-hooks", "ci"], step: "commit",
      refuses: "a body that is prose rather than at most three `-` bullets",
      source: GITHOOK,
    },
    {
      rule: "git.ai_attribution", planes: ["git-guard", "git-hooks", "ci"], step: "commit",
      refuses: "a Co-Authored-By or generated-with line",
      source: GITHOOK,
    },
    {
      rule: "git.commit_emoji", planes: ["git-guard", "git-hooks", "ci"], step: "commit",
      refuses: "an emoji in the subject",
      source: GITHOOK,
    },
    {
      rule: "git.commit_to_protected", planes: ["git-guard", "git-hooks"], step: "commit",
      refuses: "committing on a protected branch",
      source: GITHOOK,
    },
    {
      rule: "git.hard_reset_protected", planes: ["git-guard"], step: "rewrite",
      refuses: "hard-resetting a protected branch",
      note: "caught before the command runs; no hook fires for it",
      source: GUARD,
    },
    {
      rule: "git.no_verify_bypass", planes: ["git-guard"], step: "commit",
      refuses: "`--no-verify`",
      source: GUARD,
    },
    {
      rule: "git.override_token_laundering", planes: ["git-guard"], step: "rewrite",
      refuses: "setting an override env or gate token",
      source: GUARD,
    },
    {
      rule: "git.push_to_protected", planes: ["git-guard", "git-hooks"], step: "push",
      refuses: "pushing to a protected branch",
      source: GITHOOK,
    },
    {
      rule: "git.force_push_protected", planes: ["git-guard", "git-hooks"], step: "push",
      refuses: "force-pushing a protected branch",
      source: GITHOOK,
    },
    {
      rule: "git.test_gate_on_push", planes: ["git-hooks"], step: "push",
      refuses: "a push whose tests were never run",
      source: GITHOOK,
    },
    {
      rule: "git.pr_sections", planes: ["git-guard", "ci"], step: "pull request",
      refuses: "a PR body missing a required section",
      source: CI,
    },
    {
      rule: "git.pr_merge_to_protected", planes: ["git-guard"], step: "merge",
      refuses: "an agent merging to a protected branch",
      note: "a human merges on green CI",
      quote: {
        text: "push a feature branch, open a PR from the template; a **human** merges on green CI — agents never merge to a protected branch",
        source: AGENTS,
      },
      source: GUARD,
    },
  ];

  // The runway a developer actually walks, with the refusals that leave it.
  const steps = [
    { id: "S1", action: "codeflow orient", says: "where you are, what is open, which gates are wired", refuses: [] },
    { id: "S2", action: "git worktree add …  task/TSK-NNN-<slug>", says: "one owner, one branch, one worktree", refuses: ["E1"] },
    { id: "S3", action: "codeflow work start TSK-NNN", says: "the whole anchor, before you write anything", refuses: ["E2", "E3", "E4", "E5", "E6", "E7", "E8", "E9"] },
    { id: "S4", action: "edit · git add · git commit", says: "small and often", refuses: ["E2", "E3", "E4", "E5", "E6", "E7", "E8", "E9"], gates: ["git.secret_scan", "git.commit_format", "git.commit_body", "git.ai_attribution", "git.commit_emoji", "git.commit_to_protected", "git.no_verify_bypass"] },
    { id: "S5", action: "codeflow test", says: "the local gate warns; clear what it flags", refuses: [] },
    { id: "S6", action: "git push", says: "durability, not a merge", refuses: ["E1"], gates: ["git.push_to_protected", "git.force_push_protected", "git.test_gate_on_push"] },
    { id: "S7", action: "open a PR · green CI · a human merges", says: "the only authoritative gate on this repository", refuses: ["E1", "E2", "E3", "E4", "E5", "E6", "E7", "E8", "E9"], gates: ["git.pr_sections", "git.pr_merge_to_protected"] },
  ];

  const scale = { capabilitiesShipped: 13, capabilitiesBuilding: 3, buildingIds: ["CAP-014", "CAP-015", "CAP-016"] };

  // ------------------------------------------------------------ derivation
  const conceptById = new Map(concepts.map((c) => [c.id, c]));
  const depthCache = new Map();
  const depth = (id) => {
    if (depthCache.has(id)) return depthCache.get(id);
    const needs = conceptById.get(id).needs;
    const value = needs.length ? 1 + Math.max(...needs.map(depth)) : 0;
    depthCache.set(id, value);
    return value;
  };
  concepts.forEach((c) => depth(c.id));
  const maxDepth = Math.max(...concepts.map((c) => depth(c.id)));

  const byRule = new Map();
  preconditions.forEach((e) => byRule.set(e.rule, [...(byRule.get(e.rule) ?? []), e.id]));
  // The gate of record is the rule that refuses the most preconditions.
  const anchorRule = [...byRule.entries()].sort((a, b) => b[1].length - a[1].length)[0][0];
  const anchorPlanes = preconditions.find((e) => e.rule === anchorRule).planes;
  const siblingRules = [...byRule.keys()].filter((rule) => rule !== anchorRule && rule.startsWith("work."));
  const planeById = new Map(planes.map((p) => [p.id, p]));
  const realBoundary = planes.filter((p) => p.kind === "authoritative" && p.armedHere).map((p) => p.id);

  return {
    planes, concepts, preconditions, mutationGates, steps, scale,
    conceptById, planeById, depth, maxDepth, byRule, anchorRule, anchorPlanes, siblingRules, realBoundary,
    anchored: byRule.get(anchorRule),
    // Published to the DOM by all three D1 candidates.
    answer: {
      mustBeTrue: preconditions.map((e) => e.id),
      gate: anchorRule,
      refusedByGate: byRule.get(anchorRule),
      planes: anchorPlanes,
      siblingRules,
      realBoundary,
    },
  };
})();
