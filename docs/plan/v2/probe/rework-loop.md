# Day 0 probe — the develop rework loop as a native workflow script

**Charter:** section 12 (Day 0 probe); principles 2, 8, 9, 10 (section 2); the one shipped workflow (section 4.4).
**Question:** can a native Claude Code workflow script express the develop pipeline's bounded rework loop — build -> review -> (changes_requested -> build again, max 3) -> verify — with no bespoke runtime?
**Companion artifact:** [`develop.workflow.js`](develop.workflow.js), a realistic draft of the shipped workflow.

## API grounding

A workflow script is a single JavaScript file (`.claude/workflows/<name>.workflow.js`) that the Workflow tool executes top-to-bottom as the body of an async function. The primitives the loop needs:

| Primitive | What it gives the loop |
|---|---|
| Plain JS control flow | `while` + a constant cap = the bounded back-edge, natively; sequencing is statement order |
| `await agent(prompt, opts)` | one stage = one awaited subagent call with a fresh context |
| `opts.schema` (JSON Schema) | the review verdict is a parsed enum (`approved` \| `changes_requested`) plus typed findings — never prose-matching |
| `opts.agent` | binds the review stage to the `cf-reviewer` definition; fresh context per call is the reviewer-independence property |
| Phases | progress/grouping affordance only — deliberately not load-bearing; the loop must not depend on them, and does not |

The codeflow CLI is the gate plane inside stages: agents run `codeflow test` / `codeflow validate` and report exact exit codes through the schema; the script branches only on exit codes and schema enums.

## The loop, condensed

```js
let attempt = 0;
let review = { verdict: "changes_requested", findings: [] };

while (attempt < 3 && review.verdict !== "approved") {
  attempt += 1;
  const build = await agent(buildPrompt(task, criteria, review.findings), { schema: BUILD_SCHEMA });
  if (build.quick_test_exit !== 0) { review = gateFailureAsFindings(build); continue; }
  review = await agent(reviewPrompt(task, criteria, build), { agent: "cf-reviewer", schema: REVIEW_SCHEMA });
}
if (review.verdict !== "approved")
  throw new Error(`rework cap (3) exhausted: ${JSON.stringify(review.findings)}`);

const verify = await agent(verifyPrompt(task, criteria), { schema: VERIFY_SCHEMA });
if (verify.test_exit !== 0 || verify.criteria_unmet.length > 0)
  throw new Error("verify gate failed");
```

Properties:

- **Bounded.** The cap sits in the loop condition; re-entry is checked before it happens and a fourth attempt is structurally unreachable. The cap is a script constant today; it becomes a `policy.json` value the moment two workflows want it (principle 7).
- **Deterministic.** Every branch reads a schema enum or a CLI exit code. No transcript scraping, no sentinel files, no messaging between stages — hand-off is the awaited return value.
- **Legible failure.** Cap exhaustion and gate failures `throw` with the findings attached (principle 8); the caller (`/cf-develop`) and a human see exactly why and where it stopped.
- **Independent review.** The reviewer is a separate `agent()` call bound to `cf-reviewer` — it never shares the builder's context and re-runs the gates itself.

## Relation to the v1 spike (INF-TSK-051-004)

The v1 spike (archive branch `spike/inf-tsk-051-004-rework-loop-spike`, `.claude/workflows/scratch/recommendation.md`) answered **NO** for declarative one-level `workflow()`/`pipeline()` composition: a pipeline DSL describes a DAG, rework is a data-driven bounded cycle, so v1 concluded it needed a bespoke "iteration controller / router" runtime above the stages — exactly the orchestration organ the charter deletes (D5, D6). The native script model dissolves the problem rather than solving it: the controller the spike named **is the script**. The host language supplies iteration; there is nothing left to build that could own the loop.

## Verdict: Door A

**Native workflow scripts suffice. No bespoke runtime, no stage helper, nothing stamped disposable because nothing is built.** The bounded rework loop is ~15 lines of plain JS around three `agent()` awaits; gates are codeflow CLI exit codes; verdicts are schema enums.

Door B triggers — none observed; recorded so the door can be reopened honestly if the runtime contradicts them:

1. `agent()` without reliable schema-constrained output — verdicts would regress to prose parsing and determinism is lost.
2. `agent()` calls not context-isolated — reviewer independence is lost.
3. The runtime rejecting host-language loops or long-lived awaits in scripts.

## Caveats (principle 10, 90-day humility)

- This probe is authored, not executed under the Workflow tool. Its mechanical gate — the script parses as an async-function body (`new AsyncFunction(source)`) — has been run; the end-to-end run lands with workstream E's shipped `develop` workflow and the Day 2–3 dogfood, which is the real acceptance point.
- Workflow scripts are the one harness-coupled artifact class (charter risk table): cheap to rewrite, quarterly review against the harness changelog applies.
