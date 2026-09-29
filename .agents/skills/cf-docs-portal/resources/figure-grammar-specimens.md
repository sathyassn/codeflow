# Figure grammar specimens (shared skill resource)

**Status:** the worked examples for `figure-grammar.md`, which holds the
families, the twelve rules, the altitude contract, the mark vocabulary, the
declaration field reference and the self-check. Load this file when authoring
a figure. It is byte-identical in `cf-present` and `cf-docs-portal`.

One specimen per family, the wide composition at 720 units, drawn with the
`--cf-fig-*` roles and the mark vocabulary of `figure-grammar.md` section 4.
Every state mark carries `data-state`; every specimen carries a title, a description, a legend, a
one-sentence caption and a table twin. The declaration beside each specimen
states the narrow recomposition. Labels are 14 units, so at the 646 px break
they render at 12.5 px. Facts are this repository's own, except in the
layering and coverage specimens, which draw the supported architecture of a
consuming repository whose remote protection has been verified active.
CodeFlow's own repository has remote protection unavailable (its
`AGENTS.md`, project-specific instructions); substitute your repository's
verified enforcement state before drawing either.

Each specimen ends with its chat form: the same facts drawn as a fenced
ASCII figure for plain-text chat and a README, with one legend line and one
caption line, every line printable ASCII and under 78 columns.

The YAML beside each specimen sketches its intent; the module loads only the
JSON declarations of `figure-grammar.md` section 6, and the complete ones for
these specimens are the portal starter's `tests/fixtures/figures/*.json`.
Check each rendered figure against the gate at both widths and in both modes.

## 1. flow

```yaml
figure:
  id: landing-paths
  family: flow
  binding: authored
  question: "How does a change reach main, and what stops it?"
  idea: "Both landing paths end at a human merge behind green checks."
  states: [done, todo, agent, merge, human, stop]
  facts:
    - claim: "two landing paths, PR and codeflow integrate"
      source: "AGENTS.md#git-rules"
      derive: "the two paths named under 'Work lands by exactly two paths'"
  narrow: { recompose: rotate, drops: [], marks: same }
```

<figure class="cf-fig" data-cf-figure="flow">
<p class="cf-fig-title"><span class="cf-fig-number">Figure</span> · <span class="cf-fig-name">Two landing paths from a task branch to main</span></p>
<svg class="cf-fig-svg" viewBox="0 0 720 212" role="img" aria-labelledby="fg-flow-t fg-flow-d" font-family="var(--cf-fig-font)" font-size="14" font-weight="500" fill="var(--cf-fig-line)">
<title id="fg-flow-t">Two landing paths from a task branch to main</title>
<desc id="fg-flow-d">Four columns: branch, review, gate, land. Path A runs from a task branch to a pull request, to required checks, to a human merge, all travelled. Path B runs from a task in an epic to codeflow integrate, an agent merge point, to the integration branch, then along travel not yet made to the same human merge. A gate that stops travel crosses both paths in the gate column.</desc>
<g fill="var(--cf-fig-line-mid)" font-weight="600" text-anchor="middle"><text x="90" y="22">Branch</text><text x="270" y="22">Review</text><text x="450" y="22">Gate</text><text x="630" y="22">Land</text></g>
<line x1="24" y1="40" x2="696" y2="40" stroke="var(--cf-fig-rule)" stroke-width="1"/>
<g data-state="done" fill="none" stroke="var(--cf-fig-accent)" stroke-width="3" stroke-linecap="round"><path d="M99 100H261"/><path d="M279 100H441"/><path d="M459 100H620"/><path d="M99 180H258"/><path d="M282 180H441"/></g>
<path data-state="todo" d="M459 180H540C590 180 630 150 630 111" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="2" stroke-dasharray="6 4" stroke-linecap="round"/>
<g data-state="stop" stroke="var(--cf-fig-stop)" stroke-width="4" stroke-linecap="square"><line x1="516" y1="89" x2="516" y2="111"/><line x1="516" y1="169" x2="516" y2="191"/></g>
<g data-state="agent" fill="var(--cf-fig-accent)" stroke="var(--cf-fig-ground)" stroke-width="2"><circle cx="90" cy="100" r="8"/><circle cx="270" cy="100" r="8"/><circle cx="450" cy="100" r="8"/><circle cx="90" cy="180" r="8"/><circle cx="450" cy="180" r="8"/></g>
<path data-state="merge" d="M270 169L281 180L270 191L259 180Z" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-accent)" stroke-width="3.5" stroke-linejoin="round"/>
<circle data-state="human" cx="630" cy="100" r="9" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="2.5"/>
<g text-anchor="middle"><text x="90" y="76">Task branch</text><text x="270" y="76">Pull request</text><text x="450" y="76">Required checks</text><text x="630" y="76" font-weight="600">Human merge</text><text x="90" y="156">Task in an epic</text><text x="270" y="156" font-family="var(--cf-fig-mono)" font-weight="400">codeflow integrate</text><text x="450" y="156">Integration branch</text></g>
</svg>
<ul class="cf-legend" aria-label="Legend">
<li data-state="done"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M3 8H25" fill="none" stroke="var(--cf-fig-accent)" stroke-width="3" stroke-linecap="round"/></svg>Travel completed</li>
<li data-state="todo"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H26" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="2" stroke-dasharray="6 4" stroke-linecap="round"/></svg>Travel not yet made</li>
<li data-state="agent"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><circle cx="14" cy="8" r="6.5" fill="var(--cf-fig-accent)"/></svg>Agent step</li>
<li data-state="merge"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M14 2.5L19.5 8L14 13.5L8.5 8Z" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-accent)" stroke-width="3.5"/></svg>Agent merge point</li>
<li data-state="human"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><circle cx="14" cy="8" r="6" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="2.5"/></svg>Human decision</li>
<li data-state="stop"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><line x1="14" y1="3" x2="14" y2="13" stroke="var(--cf-fig-stop)" stroke-width="4" stroke-linecap="square"/></svg>Gate that stops travel</li>
</ul>
<figcaption class="cf-fig-caption">One task lands as a pull request and a body of work lands task by task on an integration branch, and both end at a human merge behind green checks.</figcaption>
<details class="cf-fig-details"><summary>Details</summary><p class="cf-fig-description">Four columns: branch, review, gate, land. Path A runs from a task branch to a pull request, to required checks, to a human merge, all travelled. Path B runs from a task in an epic to codeflow integrate, an agent merge point, to the integration branch, then along travel not yet made to the same human merge. A gate that stops travel crosses both paths in the gate column.</p>
<table><thead><tr><th>Path</th><th>Who opens</th><th>Who merges</th><th>What stops it</th></tr></thead>
<tbody><tr><td>A: one task</td><td>an agent opens the pull request</td><td>a person, on green checks</td><td>a red required check</td></tr>
<tr><td>B: body of work</td><td>agents land each task with <code>codeflow integrate</code></td><td>a person merges the finished body</td><td>a red required check on the integration pull request</td></tr></tbody></table>
</details>
</figure>

Narrow: rotate; the four columns become four rows read downward, path labels
move to the right of each node, the stop bars turn horizontal.

### Chat form

```text
 Branch          Review             Gate               Land
 Task branch     Pull request       Required checks    Human merge
 o===============o==================o=========|========(H)
                                                        ^
 o===============<>=================o - - - - | - - - - +
 Task in epic    codeflow integrate Integration branch
Legend: = travelled  - - not yet  o agent  <> agent merge  (H) human  | stop
Caption: Both landing paths end at a human merge behind green checks.
```

## 2. structure

```yaml
figure:
  id: skill-copies
  family: structure
  binding: authored
  question: "Where does a skill live, and what keeps the copies honest?"
  idea: "One source, four managed copies, and a test that compares bytes."
  states: [owner, copy, copies, compares]
  facts:
    - claim: "four copies per skill source"
      source: "crates/codeflow-core/tests/manifest_consistency.rs"
      derive: "the copies array in assert_skill_source_live_and_baseline_copies"
  narrow: { recompose: stack, drops: [], marks: same }
```

<figure class="cf-fig" data-cf-figure="structure">
<p class="cf-fig-title"><span class="cf-fig-number">Figure</span> · <span class="cf-fig-name">Where a skill lives and what keeps its copies identical</span></p>
<svg class="cf-fig-svg" viewBox="0 0 720 228" role="img" aria-labelledby="fg-struct-t fg-struct-d" font-family="var(--cf-fig-font)" font-size="14" font-weight="500" fill="var(--cf-fig-line)">
<title id="fg-struct-t">Where a skill lives and what keeps its copies identical</title>
<desc id="fg-struct-d">Three regions. The owner region assets/base holds the skill source. The live region holds two copies, .claude/skills and .agents/skills. The baseline region .codeflow/.baseline holds the same two copies again. Solid arrows run from the source to each live copy and from each live copy to its baseline copy, meaning managed copy. A dashed line with open square ends runs under all regions, meaning the parity test compares every copy with the source byte for byte.</desc>
<defs><marker id="fg-struct-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto"><path d="M0 0L10 5L0 10Z" fill="var(--cf-fig-line)"/></marker></defs>
<rect data-state="owner" x="16" y="40" width="168" height="120" rx="8" fill="var(--cf-fig-fill)" stroke="var(--cf-fig-line)" stroke-width="2"/>
<text x="32" y="64" font-weight="600">assets/base</text>
<text x="32" y="106" font-family="var(--cf-fig-mono)" font-weight="400">agents/skills/</text>
<text x="32" y="126" font-family="var(--cf-fig-mono)" font-weight="400">cf-present/</text>
<g data-state="copy" fill="none" stroke="var(--cf-fig-line-soft)" stroke-width="1.5"><rect x="268" y="24" width="184" height="152" rx="8"/><rect x="520" y="24" width="184" height="152" rx="8"/></g>
<text x="284" y="48" font-weight="600">live mirrors</text>
<text x="536" y="48" font-weight="600">.codeflow/.baseline</text>
<g font-family="var(--cf-fig-mono)" font-weight="400"><text x="284" y="96">.claude/skills</text><text x="284" y="148">.agents/skills</text><text x="536" y="96">.claude/skills</text><text x="536" y="148">.agents/skills</text></g>
<g data-state="copies" fill="none" stroke="var(--cf-fig-line)" stroke-width="2" marker-end="url(#fg-struct-a)"><path d="M184 92H266"/><path d="M184 108C220 108 230 144 266 144"/><path d="M452 92H518"/><path d="M452 144H518"/></g>
<g data-state="compares" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="1.5" stroke-dasharray="6 4"><path d="M100 172V200H612V184"/><rect x="95" y="160" width="10" height="10" fill="var(--cf-fig-ground)" stroke-dasharray="none"/><rect x="607" y="176" width="10" height="10" fill="var(--cf-fig-ground)" stroke-dasharray="none"/></g>
<text x="356" y="222" text-anchor="middle" fill="var(--cf-fig-line-mid)">parity test: bytes equal, or the build fails</text>
</svg>
<ul class="cf-legend" aria-label="Legend">
<li data-state="owner"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="2" width="24" height="12" rx="3" fill="var(--cf-fig-fill)" stroke="var(--cf-fig-line)" stroke-width="2"/></svg>Owner: the edited source</li>
<li data-state="copy"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="2" width="24" height="12" rx="3" fill="none" stroke="var(--cf-fig-line-soft)" stroke-width="1.5"/></svg>Managed copy, never edited</li>
<li data-state="copies"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H20" fill="none" stroke="var(--cf-fig-line)" stroke-width="2"/><path d="M19 4L26 8L19 12Z" fill="var(--cf-fig-line)"/></svg>Copies to</li>
<li data-state="compares"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M6 8H22" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="1.5" stroke-dasharray="6 4"/><rect x="1" y="4" width="8" height="8" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line-mid)" stroke-width="1.5"/><rect x="19" y="4" width="8" height="8" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line-mid)" stroke-width="1.5"/></svg>Compared byte for byte</li>
</ul>
<figcaption class="cf-fig-caption">Edit the source under assets/base only; the four copies are written by the scaffold and a test fails the build when any copy drifts.</figcaption>
<details class="cf-fig-details"><summary>Details</summary><p class="cf-fig-description">Three regions. The owner region assets/base holds the skill source. The live region holds two copies, .claude/skills and .agents/skills. The baseline region .codeflow/.baseline holds the same two copies again. Solid arrows run from the source to each live copy and from each live copy to its baseline copy, meaning managed copy. A dashed line with open square ends runs under all regions, meaning the parity test compares every copy with the source byte for byte.</p>
<table><thead><tr><th>Region</th><th>Path</th><th>Written by</th><th>Checked by</th></tr></thead>
<tbody><tr><td>owner</td><td><code>assets/base/agents/skills/</code></td><td>the author</td><td>manifest entry</td></tr>
<tr><td>live</td><td><code>.claude/skills/</code>, <code>.agents/skills/</code></td><td><code>codeflow update</code></td><td>parity test</td></tr>
<tr><td>baseline</td><td><code>.codeflow/.baseline/.claude/skills/</code>, <code>.codeflow/.baseline/.agents/skills/</code></td><td><code>codeflow update</code></td><td>parity test</td></tr></tbody></table>
</details>
</figure>

Narrow: stack; the three regions sit one under another with the copy arrows
turned downward, and the parity line runs down the left edge.

### Chat form

```text
 owner: assets/base    live: repository root   baseline: .codeflow/.baseline/
 [# skill source #]-+->[ .claude/skills ]---->[ .claude/skills ]
                    +->[ .agents/skills ]---->[ .agents/skills ]
 <.......... the parity test compares every copy with the source ..........>
Legend: [# #] owner, edited  [ ] managed copy  -> copies to  <...> compares
Caption: Edit only the source under assets/base; any drift fails the build.
```

## 3. layering

```yaml
figure:
  id: enforcement-planes
  family: layering
  binding: authored
  question: "Which plane covers which moment, and which one is the boundary?"
  idea: "Four planes cover different moments; verified remote protection is required."
  states: [layer, layer-remote, act]
  facts:
    - claim: "four planes: git hooks, session git-guard, CI, remote branch protection"
      source: "AGENTS.md#git-rules"
      derive: "the four planes named in the first sentence of Git rules"
  narrow: { recompose: rotate, drops: [], marks: same }
```

<figure class="cf-fig" data-cf-figure="layering">
<p class="cf-fig-title"><span class="cf-fig-number">Figure</span> · <span class="cf-fig-name">Four enforcement planes along the life of one change</span></p>
<svg class="cf-fig-svg" viewBox="0 0 720 248" role="img" aria-labelledby="fg-layer-t fg-layer-d" font-family="var(--cf-fig-font)" font-size="14" font-weight="500" fill="var(--cf-fig-line)">
<title id="fg-layer-t">Four enforcement planes along the life of one change</title>
<desc id="fg-layer-d">A shared axis runs edit, commit, push, pull request, merge. Git hooks cover commit and push and can stop both. The session git-guard covers edit to merge and can stop commit, push, pull request and merge. CI covers push and pull request and can stop at the pull request. Verified remote branch protection covers push to merge and can stop push and merge; its end cap means remote and required.</desc>
<g fill="var(--cf-fig-line-mid)" text-anchor="middle"><text x="240" y="22">Edit</text><text x="340" y="22">Commit</text><text x="440" y="22">Push</text><text x="540" y="22">Pull request</text><text x="640" y="22">Merge</text></g>
<line x1="190" y1="42" x2="690" y2="42" stroke="var(--cf-fig-rule)" stroke-width="1"/>
<g stroke="var(--cf-fig-line-soft)" stroke-width="1.5" stroke-linecap="round"><line x1="240" y1="36" x2="240" y2="42"/><line x1="340" y1="36" x2="340" y2="42"/><line x1="440" y1="36" x2="440" y2="42"/><line x1="540" y1="36" x2="540" y2="42"/><line x1="640" y1="36" x2="640" y2="42"/></g>
<text x="0" y="89">Git hooks</text><text x="0" y="137">Session git-guard</text><text x="0" y="185">CI</text><text x="0" y="233" font-weight="600">Remote branch protection</text>
<g data-state="layer" fill="var(--cf-fig-fill)" stroke="var(--cf-fig-line-soft)" stroke-width="1.5"><rect x="290" y="72" width="200" height="24" rx="6"/><rect x="190" y="120" width="500" height="24" rx="6"/><rect x="390" y="168" width="200" height="24" rx="6"/></g>
<g data-state="layer-remote"><rect x="390" y="216" width="286" height="24" rx="6" fill="var(--cf-fig-fill)" stroke="var(--cf-fig-line)" stroke-width="2"/><path d="M662 216H670A6 6 0 0 1 676 222V234A6 6 0 0 1 670 240H662Z" fill="var(--cf-fig-line)"/></g>
<g data-state="act" fill="var(--cf-fig-accent)" stroke="var(--cf-fig-ground)" stroke-width="2.5"><circle cx="340" cy="84" r="8"/><circle cx="440" cy="84" r="8"/><circle cx="340" cy="132" r="8"/><circle cx="440" cy="132" r="8"/><circle cx="540" cy="132" r="8"/><circle cx="640" cy="132" r="8"/><circle cx="540" cy="180" r="8"/><circle cx="440" cy="228" r="8"/><circle cx="640" cy="228" r="8"/></g>
</svg>
<ul class="cf-legend" aria-label="Legend">
<li data-state="layer"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="3" width="24" height="10" rx="3" fill="var(--cf-fig-fill)" stroke="var(--cf-fig-line-soft)" stroke-width="1.5"/></svg>Plane covers this moment, local and editable</li>
<li data-state="layer-remote"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="3" width="24" height="10" rx="3" fill="var(--cf-fig-fill)" stroke="var(--cf-fig-line)" stroke-width="2"/><path d="M20 3H23A3 3 0 0 1 26 6V10A3 3 0 0 1 23 13H20Z" fill="var(--cf-fig-line)"/></svg>Plane covers this moment, remote and required</li>
<li data-state="act"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><circle cx="14" cy="8" r="6.5" fill="var(--cf-fig-accent)"/></svg>Plane acts here</li>
</ul>
<figcaption class="cf-fig-caption">Hooks and the guard stop changes before push; CI blocks the pull request; verified remote protection stops push and merge.</figcaption>
<details class="cf-fig-details"><summary>Details</summary><p class="cf-fig-description">A shared axis runs edit, commit, push, pull request, merge. Git hooks cover commit and push and can stop both. The session git-guard covers edit to merge and can stop commit, push, pull request and merge. CI covers push and pull request and can stop at the pull request. Verified remote branch protection covers push to merge and can stop push and merge; its end cap means remote and required.</p>
<table><thead><tr><th>Plane</th><th>Moments</th><th>Shared source</th><th>Can be bypassed locally</th></tr></thead>
<tbody><tr><td>Git hooks</td><td>commit, push</td><td><code>.codeflow/policy.json</code></td><td>yes, hooks can be edited</td></tr>
<tr><td>Session git-guard</td><td>edit to merge</td><td><code>.codeflow/policy.json</code></td><td>yes, the session setup can be edited</td></tr>
<tr><td>CI</td><td>push, pull request</td><td><code>.codeflow/policy.json</code> through <code>codeflow ci</code></td><td>yes, unless the remote requires its result</td></tr>
<tr><td>Remote branch protection</td><td>push to merge</td><td>rules derived from <code>.codeflow/policy.json</code></td><td>no, when the remote requires it</td></tr></tbody></table>
</details>
</figure>

Narrow: rotate; planes become columns with short heads (Hooks, Guard, CI,
Remote) expanded in the caption, moments become rows, the bars stand vertical.

The Remote plane is drawn as verified active; a repository without armed
remote protection draws the three planes it has and says so in the caption.

### Chat form

```text
              edit    commit  push    PR      merge
 Git hooks           [*----*---]
 git-guard   [--------*---*---*---*]
 CI                          [--------*]
 Remote                     [#*######*]|
Legend: [--] local plane  [##]| remote plane  * can stop here
Caption: Verified Remote stops push and merge; Guard spans edit to merge.
```

## 4. sequence

```yaml
figure:
  id: work-start
  family: sequence
  binding: authored
  question: "What does an agent ask, and of whom, before its first edit?"
  idea: "Three calls and two answers come before the first edit, and one answer can refuse."
  states: [trans, return, human, stop]
  facts:
    - claim: "identity, intent-match and currency precede the first mutation"
      source: "AGENTS.md#worktree-doctrine"
      derive: "the three ordered work-start assertions"
    - claim: "codeflow work start proves the task is anchored before product edits"
      source: "AGENTS.md#planning-and-tracking"
      derive: "the sentence beginning 'Before product edits'"
  narrow: { recompose: stack, drops: [], marks: same }
```

<figure class="cf-fig" data-cf-figure="sequence">
<p class="cf-fig-title"><span class="cf-fig-number">Figure</span> · <span class="cf-fig-name">The exchanges that come before the first edit</span></p>
<svg class="cf-fig-svg" viewBox="0 0 720 300" role="img" aria-labelledby="fg-seq-t fg-seq-d" font-family="var(--cf-fig-font)" font-size="14" font-weight="500" fill="var(--cf-fig-line)">
<title id="fg-seq-t">The exchanges that come before the first edit</title>
<desc id="fg-seq-d">Three lifelines: agent, git, codeflow. Time runs down. The agent calls git worktree list and git answers with the paths and branches. The agent decides identity and intent match, a human decision ring. The agent calls git fetch origin and decides currency. The agent calls codeflow work start TSK-NNN; codeflow answers anchored, or refuses at a stop bar. The first edit follows the answer.</desc>
<defs><marker id="fg-seq-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto"><path d="M0 0L10 5L0 10Z" fill="var(--cf-fig-line)"/></marker><marker id="fg-seq-r" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="9" markerHeight="9" markerUnits="userSpaceOnUse" orient="auto"><path d="M1 1L9 5L1 9Z" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-accent)" stroke-width="1.5"/></marker></defs>
<g font-weight="600" text-anchor="middle"><text x="120" y="22">Agent</text><text x="400" y="22">Git</text><text x="640" y="22">codeflow</text></g>
<g stroke="var(--cf-fig-line-soft)" stroke-width="1.5" stroke-linecap="round"><line x1="120" y1="34" x2="120" y2="292"/><line x1="400" y1="34" x2="400" y2="292"/><line x1="640" y1="34" x2="640" y2="292"/></g>
<g data-state="trans" fill="none" stroke="var(--cf-fig-line)" stroke-width="1.75" marker-end="url(#fg-seq-a)"><path d="M128 60H392"/><path d="M128 160H392"/><path d="M128 226H632"/></g>
<g data-state="return" fill="none" stroke="var(--cf-fig-accent)" stroke-width="3" stroke-linecap="round" marker-end="url(#fg-seq-r)"><path d="M392 86H130"/><path d="M632 256H130"/></g>
<g data-state="human" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="2.5"><circle cx="120" cy="124" r="9"/><circle cx="120" cy="190" r="9"/></g>
<line data-state="stop" x1="600" y1="245" x2="600" y2="267" stroke="var(--cf-fig-stop)" stroke-width="4" stroke-linecap="square"/>
<g font-family="var(--cf-fig-mono)" font-weight="400"><text x="260" y="50" text-anchor="middle">git worktree list</text><text x="260" y="150" text-anchor="middle">git fetch origin</text><text x="260" y="216" text-anchor="middle">codeflow work start TSK-NNN</text></g>
<g fill="var(--cf-fig-line-mid)"><text x="260" y="104" text-anchor="middle">paths and branches</text><text x="260" y="274" text-anchor="middle">anchored, or refused</text></g>
<text x="138" y="129">identity and intent match</text>
<text x="138" y="195">currency: base is the target tip</text>
</svg>
<ul class="cf-legend" aria-label="Legend">
<li data-state="trans"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H20" fill="none" stroke="var(--cf-fig-line)" stroke-width="1.75"/><path d="M19 4L26 8L19 12Z" fill="var(--cf-fig-line)"/></svg>Call</li>
<li data-state="return"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M26 8H9" fill="none" stroke="var(--cf-fig-accent)" stroke-width="3" stroke-linecap="round"/><path d="M9 4L2 8L9 12Z" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-accent)" stroke-width="1.5"/></svg>Answer</li>
<li data-state="human"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><circle cx="14" cy="8" r="6" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="2.5"/></svg>Decision the agent makes</li>
<li data-state="stop"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><line x1="14" y1="3" x2="14" y2="13" stroke="var(--cf-fig-stop)" stroke-width="4" stroke-linecap="square"/></svg>Refusal stops the sequence</li>
</ul>
<figcaption class="cf-fig-caption">The agent asks git where it is and whether it is current, then asks codeflow whether the task is anchored, and edits nothing until the last answer is yes.</figcaption>
<details class="cf-fig-details"><summary>Details</summary><p class="cf-fig-description">Three lifelines: agent, git, codeflow. Time runs down. The agent calls git worktree list and git answers with the paths and branches. The agent decides identity and intent match, a human decision ring. The agent calls git fetch origin and decides currency. The agent calls codeflow work start TSK-NNN; codeflow answers anchored, or refuses at a stop bar. The first edit follows the answer.</p>
<table><thead><tr><th>Step</th><th>From</th><th>To</th><th>Outcome</th></tr></thead>
<tbody><tr><td><code>git worktree list</code></td><td>agent</td><td>git</td><td>paths and branches</td></tr>
<tr><td>identity, intent match</td><td>agent</td><td>agent</td><td>stop on a mismatch</td></tr>
<tr><td><code>git fetch origin</code>, currency</td><td>agent</td><td>git</td><td>base is the target tip</td></tr>
<tr><td><code>codeflow work start TSK-NNN</code></td><td>agent</td><td>codeflow</td><td>anchored, or refused</td></tr>
<tr><td>first edit</td><td>agent</td><td>worktree</td><td>only after anchored</td></tr></tbody></table>
</details>
</figure>

Narrow: stack; one lifeline, calls and answers become labelled rows, the
participant is named in each row's label.

### Chat form

```text
 agent                     git                  codeflow
   |-- git worktree list -->|                       |
   |<== paths, branches ====|                       |
  (H) identity and intent   |                       |
   |-- git fetch origin --->|                       |
  (H) currency              |                       |
   |-- codeflow work start TSK-NNN ---------------->|
   |<== anchored ===================================|
   |                        |                      X| or refused
   |  first edit
Legend: --> call  <== answer  (H) decision the agent makes  X| refusal
Caption: The agent edits nothing until codeflow answers anchored.
```

## 5. state

```yaml
figure:
  id: change-states
  family: state
  binding: authored
  question: "What state is a change in, and what is the only way out of a red check?"
  idea: "The only route out of a red check returns to editing; a bypass is forbidden."
  states: [state, trans, return, blocked]
  facts:
    - claim: "a red check returns to editing; no bypass"
      source: "AGENTS.md#git-rules"
      derive: "the sentence beginning 'When a gate blocks you, fix the cause'"
  narrow: { recompose: stack, drops: [], marks: same }
```

<figure class="cf-fig" data-cf-figure="state">
<p class="cf-fig-title"><span class="cf-fig-number">Figure</span> · <span class="cf-fig-name">States of a change and the one route out of a red check</span></p>
<svg class="cf-fig-svg" viewBox="0 0 720 236" role="img" aria-labelledby="fg-state-t fg-state-d" font-family="var(--cf-fig-font)" font-size="14" font-weight="500" fill="var(--cf-fig-line)">
<title id="fg-state-t">States of a change and the one route out of a red check</title>
<desc id="fg-state-d">Editing, committed, pushed, pull request open, then checks green and merged. From pull request open a failing check leads to checks red. From checks red the only route returns to editing to fix the cause, drawn as a heavy dipped arc. A route from checks red to merged, a bypass, is forbidden, drawn dashed with a cross.</desc>
<defs><marker id="fg-state-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="9" markerHeight="9" markerUnits="userSpaceOnUse" orient="auto"><path d="M0 0L10 5L0 10Z" fill="var(--cf-fig-line)"/></marker><marker id="fg-state-r" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="11" markerHeight="11" markerUnits="userSpaceOnUse" orient="auto"><path d="M0 0L10 5L0 10Z" fill="var(--cf-fig-accent)"/></marker></defs>
<g data-state="trans" fill="none" stroke="var(--cf-fig-line)" stroke-width="1.75" marker-end="url(#fg-state-a)"><path d="M97 60H150"/><path d="M251 60H304"/><path d="M385 60H438"/><path d="M525 60H578"/><path d="M482 79V170"/><path d="M637 79V170"/></g>
<path data-state="return" d="M433 190C300 190 58 236 58 84" fill="none" stroke="var(--cf-fig-accent)" stroke-width="3" stroke-linecap="round" marker-end="url(#fg-state-r)"/>
<text x="196" y="176">fix the cause</text>
<g data-state="blocked" fill="none"><path d="M531 190H595" stroke="var(--cf-fig-line-soft)" stroke-width="1.5" stroke-dasharray="2 4"/><path d="M556 184L568 196M568 184L556 196" stroke="var(--cf-fig-stop)" stroke-width="2"/></g>
<text x="562" y="170" text-anchor="middle" fill="var(--cf-fig-line-mid)">bypass</text>
<g data-state="state" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="1.5"><rect x="20" y="42" width="76" height="36" rx="8"/><rect x="152" y="42" width="98" height="36" rx="8"/><rect x="306" y="42" width="78" height="36" rx="8"/><rect x="440" y="42" width="84" height="36" rx="8"/><rect x="580" y="42" width="114" height="36" rx="8"/><rect x="435" y="172" width="94" height="36" rx="8"/><rect x="597" y="172" width="80" height="36" rx="8"/></g>
<g text-anchor="middle"><text x="58" y="65">editing</text><text x="201" y="65">committed</text><text x="345" y="65">pushed</text><text x="482" y="65">PR open</text><text x="637" y="65">checks green</text><text x="482" y="195" font-weight="600">checks red</text><text x="637" y="195">merged</text></g>
</svg>
<ul class="cf-legend" aria-label="Legend">
<li data-state="state"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="2" width="24" height="12" rx="4" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="1.5"/></svg>State</li>
<li data-state="trans"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H20" fill="none" stroke="var(--cf-fig-line)" stroke-width="1.75"/><path d="M19 4L26 8L19 12Z" fill="var(--cf-fig-line)"/></svg>Transition</li>
<li data-state="return"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M26 8C18 8 12 14 6 14" fill="none" stroke="var(--cf-fig-accent)" stroke-width="3" stroke-linecap="round"/><path d="M6 8L1 14L8 16Z" fill="var(--cf-fig-accent)"/></svg>The only route out of red</li>
<li data-state="blocked"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H26" fill="none" stroke="var(--cf-fig-line-soft)" stroke-width="1.5" stroke-dasharray="2 4"/><path d="M10 4L18 12M18 4L10 12" fill="none" stroke="var(--cf-fig-stop)" stroke-width="2"/></svg>Forbidden route</li>
</ul>
<figcaption class="cf-fig-caption">A red check sends the change back to editing, and no route leads from red to merged.</figcaption>
<details class="cf-fig-details"><summary>Details</summary><p class="cf-fig-description">Editing, committed, pushed, pull request open, then checks green and merged. From pull request open a failing check leads to checks red. From checks red the only route returns to editing to fix the cause, drawn as a heavy dipped arc. A route from checks red to merged, a bypass, is forbidden, drawn dashed with a cross.</p>
<table><thead><tr><th>From</th><th>To</th><th>On</th><th>Allowed</th></tr></thead>
<tbody><tr><td>editing</td><td>committed</td><td>commit passes hooks</td><td>yes</td></tr>
<tr><td>committed</td><td>pushed</td><td>push</td><td>yes</td></tr>
<tr><td>pushed</td><td>PR open</td><td>pull request opened</td><td>yes</td></tr>
<tr><td>PR open</td><td>checks green</td><td>required checks pass</td><td>yes</td></tr>
<tr><td>PR open</td><td>checks red</td><td>a required check fails</td><td>yes</td></tr>
<tr><td>checks green</td><td>merged</td><td>a person merges</td><td>yes</td></tr>
<tr><td>checks red</td><td>editing</td><td>fix the cause</td><td>the only route</td></tr>
<tr><td>checks red</td><td>merged</td><td>bypass</td><td>no</td></tr></tbody></table>
</details>
</figure>

Narrow: stack; states run top to bottom, checks red sits beside PR open, the
return arc runs up the left margin.

### Chat form

```text
 [editing]->[committed]->[pushed]->[PR open]->[checks green]->[merged]
     ^                                 |                          :
     |                                 v                          x
     +<==== fix the cause ========[checks red] - - bypass - - - - +
Legend: [ ] state  -> transition  <== only route out of red  - x forbidden
Caption: A red check sends the change back to editing, never on to merged.
```

## 6. coverage

```yaml
figure:
  id: rules-by-plane
  family: coverage
  binding: authored
  question: "Which plane enforces which rule, and is any rule unclaimed?"
  idea: "Every rule has at least one plane; the ticket rule is not claimed remotely."
  states: [cov, part, notrun, na, nc]
  facts:
    - claim: "five rules against four planes"
      source: "AGENTS.md#git-rules"
      derive: "the rules named in Git rules and the four planes"
  narrow: { recompose: list, drops: [], marks: same }
```

<figure class="cf-fig" data-cf-figure="coverage">
<p class="cf-fig-title"><span class="cf-fig-number">Figure</span> · <span class="cf-fig-name">Which plane enforces which rule</span></p>
<svg class="cf-fig-svg" viewBox="0 0 720 232" role="img" aria-labelledby="fg-cov-t fg-cov-d" font-family="var(--cf-fig-font)" font-size="14" font-weight="500" fill="var(--cf-fig-line)">
<title id="fg-cov-t">Which plane enforces which rule</title>
<desc id="fg-cov-d">A grid of five rules against four planes. Secret scan: hooks covered, guard covered, CI covered, remote covered. Protected branch push: hooks covered, guard covered, CI not applicable, remote covered. No AI attribution: hooks covered, guard covered, CI covered, remote not applicable. Breaking change footer: hooks partial, guard not run, CI covered, remote not applicable. Ticket reference: hooks covered, guard not applicable, CI partial, remote not claimed.</desc>
<defs><pattern id="fg-cov-h" width="4.5" height="4.5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><line x1="0" y1="0" x2="0" y2="4.5" stroke="var(--cf-fig-hatch)" stroke-width="1.5"/></pattern></defs>
<g fill="var(--cf-fig-line-mid)" font-weight="600" text-anchor="middle"><text x="380" y="30">Hooks</text><text x="460" y="30">Guard</text><text x="540" y="30">CI</text><text x="620" y="30">Remote</text></g>
<line x1="0" y1="44" x2="720" y2="44" stroke="var(--cf-fig-rule)" stroke-width="1"/>
<text x="0" y="77">Secret scan</text><text x="0" y="113">Protected branch push</text><text x="0" y="149">No AI attribution</text><text x="0" y="185">Breaking change footer</text><text x="0" y="221">Ticket reference</text>
<g data-state="cov" fill="var(--cf-fig-line)"><rect x="371" y="63" width="18" height="18" rx="2"/><rect x="451" y="63" width="18" height="18" rx="2"/><rect x="531" y="63" width="18" height="18" rx="2"/><rect x="611" y="63" width="18" height="18" rx="2"/><rect x="371" y="99" width="18" height="18" rx="2"/><rect x="451" y="99" width="18" height="18" rx="2"/><rect x="611" y="99" width="18" height="18" rx="2"/><rect x="371" y="135" width="18" height="18" rx="2"/><rect x="451" y="135" width="18" height="18" rx="2"/><rect x="531" y="135" width="18" height="18" rx="2"/><rect x="531" y="171" width="18" height="18" rx="2"/><rect x="371" y="207" width="18" height="18" rx="2"/></g>
<g data-state="part" fill="none" stroke="var(--cf-fig-line)" stroke-width="1.4" stroke-dasharray="2.5 1.8"><rect x="371" y="171" width="18" height="18" rx="2"/><rect x="531" y="207" width="18" height="18" rx="2"/></g>
<g data-state="notrun" fill="url(#fg-cov-h)" stroke="var(--cf-fig-hatch)" stroke-width="1"><rect x="451" y="171" width="18" height="18" rx="2"/></g>
<g data-state="na" fill="none" stroke="var(--cf-fig-line-soft)" stroke-width="1" stroke-opacity="var(--cf-fig-na-alpha)"><rect x="531" y="99" width="18" height="18" rx="2"/><rect x="611" y="135" width="18" height="18" rx="2"/><rect x="611" y="171" width="18" height="18" rx="2"/><rect x="451" y="207" width="18" height="18" rx="2"/></g>
<g data-state="nc"><rect x="611" y="207" width="18" height="18" rx="2" fill="none" stroke="var(--cf-fig-line-soft)" stroke-width="1.4"/><path d="M612 208L628 224M628 208L612 224" fill="none" stroke="var(--cf-fig-stop)" stroke-width="1.4"/></g>
</svg>
<ul class="cf-legend" aria-label="Legend">
<li data-state="cov"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="7" y="1" width="14" height="14" rx="2" fill="var(--cf-fig-line)"/></svg>Covered</li>
<li data-state="part"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="7" y="1" width="14" height="14" rx="2" fill="none" stroke="var(--cf-fig-line)" stroke-width="1.4" stroke-dasharray="2.5 1.8"/></svg>Partial</li>
<li data-state="notrun"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="7" y="1" width="14" height="14" rx="2" fill="url(#fg-cov-h)" stroke="var(--cf-fig-hatch)" stroke-width="1"/></svg>Not run</li>
<li data-state="na"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="7" y="1" width="14" height="14" rx="2" fill="none" stroke="var(--cf-fig-line-soft)" stroke-width="1" stroke-opacity="var(--cf-fig-na-alpha)"/></svg>Not applicable</li>
<li data-state="nc"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="7" y="1" width="14" height="14" rx="2" fill="none" stroke="var(--cf-fig-line-soft)" stroke-width="1.4"/><path d="M8 2L20 14M20 2L8 14" fill="none" stroke="var(--cf-fig-stop)" stroke-width="1.4"/></svg>Not claimed</li>
</ul>
<figcaption class="cf-fig-caption">Every rule is enforced by at least one plane, and the one crossed cell is a claim no plane makes. Hooks and Guard are the local planes; Remote is branch protection.</figcaption>
<details class="cf-fig-details"><summary>Details</summary><p class="cf-fig-description">A grid of five rules against four planes. Secret scan: hooks covered, guard covered, CI covered, remote covered. Protected branch push: hooks covered, guard covered, CI not applicable, remote covered. No AI attribution: hooks covered, guard covered, CI covered, remote not applicable. Breaking change footer: hooks partial, guard not run, CI covered, remote not applicable. Ticket reference: hooks covered, guard not applicable, CI partial, remote not claimed.</p>
<table><thead><tr><th>Rule</th><th>Hooks</th><th>Guard</th><th>CI</th><th>Remote</th></tr></thead>
<tbody><tr><td>Secret scan</td><td>covered</td><td>covered</td><td>covered</td><td>covered</td></tr>
<tr><td>Protected branch push</td><td>covered</td><td>covered</td><td>not applicable</td><td>covered</td></tr>
<tr><td>No AI attribution</td><td>covered</td><td>covered</td><td>covered</td><td>not applicable</td></tr>
<tr><td>Breaking change footer</td><td>partial</td><td>not run</td><td>covered</td><td>not applicable</td></tr>
<tr><td>Ticket reference</td><td>covered</td><td>not applicable</td><td>partial</td><td>not claimed</td></tr></tbody></table>
</details>
</figure>

Narrow: list; the grid becomes one row per rule naming its planes with the
same five cell marks inline, so no cell shrinks below 14 px.

The Remote column assumes remote protection verified active; without it, each
covered Remote cell becomes not claimed.

### Chat form

```text
                            Hooks  Guard   CI   Remote
 Secret scan                  #      #     #      #
 Protected branch push        #      #     .      #
 No AI attribution            #      #     #      .
 Breaking change footer       :      /     #      .
 Ticket reference             #      .     :      X
Legend: # covered  : partial  / not run  . not applicable  X not claimed
Caption: Every rule has a plane; the crossed cell is a claim no plane makes.
```

## 7. extent

```yaml
figure:
  id: commit-lengths
  family: extent
  binding: authored
  question: "How long may a commit subject and body be, and where is the limit?"
  idea: "Subject to 50 with a 72 line, three bullets to 72, no fourth, one optional footer."
  states: [used, warn, limit, denied, optional]
  facts:
    - claim: "description at most 50, subject line at most 72, three bullets at most 72 each"
      source: "AGENTS.md#git-rules"
      derive: "the Commits bullet"
  narrow: { recompose: stack, drops: [], marks: same }
```

<figure class="cf-fig" data-cf-figure="extent">
<p class="cf-fig-title"><span class="cf-fig-number">Figure</span> · <span class="cf-fig-name">Commit message limits drawn to length</span></p>
<svg class="cf-fig-svg" viewBox="0 0 720 328" role="img" aria-labelledby="fg-ext-t fg-ext-d" font-family="var(--cf-fig-font)" font-size="14" font-weight="500" fill="var(--cf-fig-line)">
<title id="fg-ext-t">Commit message limits drawn to length</title>
<desc id="fg-ext-d">The subject feat(portal): add display panel uses 31 of 72 characters, with limit bars at 50 for the description and 72 for the whole line. The body holds three bullets, each at most 72 characters, drawn to their length with the room left dashed; a fourth bullet is not allowed and is drawn as a denied bar with a cross. An optional BREAKING CHANGE footer follows, drawn as a dashed outline.</desc>
<text x="0" y="85" font-weight="600">Subject</text>
<text x="100" y="58" font-family="var(--cf-fig-mono)" font-weight="400">feat(portal): add display panel</text>
<rect data-state="used" x="100" y="74" width="261" height="12" rx="3" fill="var(--cf-fig-accent)"/>
<g data-state="warn" fill="none" stroke="var(--cf-fig-warn)" stroke-width="2" stroke-dasharray="5 3"><path d="M365 80H703"/><path d="M492 164H703"/><path d="M610 188H703"/><path d="M424 212H703"/></g>
<g data-state="limit" stroke="var(--cf-fig-line)" stroke-width="2"><line x1="521" y1="66" x2="521" y2="94"/><line x1="707" y1="66" x2="707" y2="94"/><line x1="707" y1="146" x2="707" y2="248"/></g>
<g fill="var(--cf-fig-line-mid)"><text x="521" y="118" text-anchor="middle">50 description</text><text x="712" y="118" text-anchor="end">72 line</text></g>
<text x="0" y="169">Bullet 1</text><text x="0" y="193">Bullet 2</text><text x="0" y="217">Bullet 3</text><text x="0" y="241" fill="var(--cf-fig-line-mid)">Bullet 4</text>
<g data-state="used" fill="var(--cf-fig-accent)"><rect x="100" y="159" width="388" height="10" rx="3"/><rect x="100" y="183" width="506" height="10" rx="3"/><rect x="100" y="207" width="320" height="10" rx="3"/></g>
<g data-state="denied"><rect x="100.75" y="230.75" width="606" height="10.5" rx="3" fill="none" stroke="var(--cf-fig-line-soft)" stroke-width="1.5" stroke-dasharray="2 4"/><path d="M397 230L409 242M409 230L397 242" fill="none" stroke="var(--cf-fig-stop)" stroke-width="2"/></g>
<text x="0" y="313" font-weight="600">Footer</text>
<text x="100" y="288" font-family="var(--cf-fig-mono)" font-weight="400">BREAKING CHANGE:</text>
<rect data-state="optional" x="100.75" y="302.75" width="134" height="10.5" rx="3" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="1.5" stroke-dasharray="6 4"/>
</svg>
<ul class="cf-legend" aria-label="Legend">
<li data-state="used"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="4" width="20" height="8" rx="2" fill="var(--cf-fig-accent)"/></svg>Used</li>
<li data-state="warn"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H26" fill="none" stroke="var(--cf-fig-warn)" stroke-width="2" stroke-dasharray="5 3"/></svg>Room left</li>
<li data-state="limit"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><line x1="14" y1="1" x2="14" y2="15" stroke="var(--cf-fig-line)" stroke-width="2"/></svg>Limit</li>
<li data-state="denied"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="4" width="24" height="8" rx="2" fill="none" stroke="var(--cf-fig-line-soft)" stroke-width="1.5" stroke-dasharray="2 4"/><path d="M9 3L19 13M19 3L9 13" fill="none" stroke="var(--cf-fig-stop)" stroke-width="2"/></svg>Not allowed</li>
<li data-state="optional"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="4" width="24" height="8" rx="2" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="1.5" stroke-dasharray="6 4"/></svg>Optional</li>
</ul>
<figcaption class="cf-fig-caption">A subject fits inside 50 with the whole line inside 72, three bullets each fit inside 72, a fourth is refused, and the footer is the one optional part.</figcaption>
<details class="cf-fig-details"><summary>Details</summary><p class="cf-fig-description">The subject feat(portal): add display panel uses 31 of 72 characters, with limit bars at 50 for the description and 72 for the whole line. The body holds three bullets, each at most 72 characters, drawn to their length with the room left dashed; a fourth bullet is not allowed and is drawn as a denied bar with a cross. An optional BREAKING CHANGE footer follows, drawn as a dashed outline.</p>
<table><thead><tr><th>Part</th><th>Limit</th><th>Example length</th><th>Rule</th></tr></thead>
<tbody><tr><td>subject description</td><td>50</td><td>31</td><td>required</td></tr>
<tr><td>subject line</td><td>72</td><td>31</td><td>required</td></tr>
<tr><td>bullet 1 to 3</td><td>72 each</td><td>46, 60, 38</td><td>at most three</td></tr>
<tr><td>bullet 4</td><td>none</td><td>0</td><td>not allowed</td></tr>
<tr><td>BREAKING CHANGE footer</td><td>none</td><td>16</td><td>optional</td></tr></tbody></table>
</details>
</figure>

Narrow: stack; each part becomes its own ruler under its label with the same
scale, and the two subject limit labels move under their bars.

### Chat form

```text
 2 per column, round up   0    10   20   30   40   50   60   70
 subject description      ################---------|
 subject line             ################--------------------|
 bullet 1                 #######################-------------|
 bullet 2                 ##############################------|
 bullet 3                 ###################-----------------|
 bullet 4                 [xxxx]
 BREAKING CHANGE footer   (........)
Legend: # used  - room left  | limit  [xx] not allowed  (..) optional
Caption: Every part fits its limit, and a fourth bullet is refused.
```

## 8. derivation

```yaml
figure:
  id: portal-page-derivation
  family: derivation
  binding: authored
  question: "Where does a portal page come from, and what proves it is current?"
  idea: "The adapter derives the page and the manifest; the validator compares both with the repository."
  states: [source, transform, product, declared, derives, declares, compares, check]
  facts:
    - claim: "the Node adapter alone derives the content graph and the evidence manifest; the Rust validator executes no project code"
      source: "cf-docs-portal/SKILL.md#4"
      derive: "the two sentences beginning 'The Node adapter alone'"
  narrow: { recompose: stack, drops: [], marks: same }
```

<figure class="cf-fig" data-cf-figure="derivation">
<p class="cf-fig-title"><span class="cf-fig-number">Figure</span> · <span class="cf-fig-name">A portal page derived from repository sources and checked against them</span></p>
<svg class="cf-fig-svg" viewBox="0 0 720 244" role="img" aria-labelledby="fg-der-t fg-der-d" font-family="var(--cf-fig-font)" font-size="14" font-weight="500" fill="var(--cf-fig-line)">
<title id="fg-der-t">A portal page derived from repository sources and checked against them</title>
<desc id="fg-der-d">Repository Markdown sources, a filled source pin, feed the adapter, a transform diamond. The adapter derives the generated page, an outlined product box, and declares the evidence manifest, a dashed box. The validator, a check ring, compares the manifest against the repository bytes and the output bytes with dashed lines ending in open squares.</desc>
<defs><marker id="fg-der-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto"><path d="M0 0L10 5L0 10Z" fill="var(--cf-fig-line)"/></marker><marker id="fg-der-o" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto"><path d="M1 1L9 5L1 9Z" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line-mid)" stroke-width="1.5"/></marker><marker id="fg-der-s" viewBox="0 0 10 10" refX="5" refY="5" markerWidth="10" markerHeight="10" markerUnits="userSpaceOnUse" orient="auto"><rect x="1" y="1" width="8" height="8" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line-mid)" stroke-width="1.5"/></marker></defs>
<g data-state="derives" fill="none" stroke="var(--cf-fig-line)" stroke-width="2" marker-end="url(#fg-der-a)"><path d="M176 80H262"/><path d="M338 80H424"/></g>
<path data-state="declares" d="M300 108V150H424" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="1.5" stroke-dasharray="6 4" marker-end="url(#fg-der-o)"/>
<g data-state="compares" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="1.5" stroke-dasharray="2 4" marker-end="url(#fg-der-s)"><path d="M652 150H588"/><path d="M664 154V206H100V112"/><path d="M664 130V80H588"/></g>
<rect data-state="source" x="20" y="56" width="156" height="48" rx="4" fill="var(--cf-fig-fill)" stroke="var(--cf-fig-line)" stroke-width="2"/>
<text x="98" y="85" text-anchor="middle">repository sources</text>
<path data-state="transform" d="M300 52L338 80L300 108L262 80Z" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-accent)" stroke-width="3.5" stroke-linejoin="round"/>
<text x="300" y="36" text-anchor="middle">adapter</text>
<rect data-state="product" x="426" y="56" width="152" height="48" rx="4" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="1.5"/>
<text x="502" y="85" text-anchor="middle">generated page</text>
<rect data-state="declared" x="426" y="132" width="152" height="36" rx="4" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="1.5" stroke-dasharray="6 4"/>
<text x="502" y="155" text-anchor="middle" font-family="var(--cf-fig-mono)" font-weight="400">evidence manifest</text>
<circle data-state="check" cx="664" cy="142" r="12" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="2.5"/>
<path d="M658 142L662 147L671 137" fill="none" stroke="var(--cf-fig-accent)" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>
<text x="664" y="232" text-anchor="middle" font-family="var(--cf-fig-mono)" font-weight="400">validate</text>
<text x="376" y="232" text-anchor="middle" fill="var(--cf-fig-line-mid)">bytes of the sources at the recorded commit</text>
</svg>
<ul class="cf-legend" aria-label="Legend">
<li data-state="source"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="2" width="24" height="12" rx="2" fill="var(--cf-fig-fill)" stroke="var(--cf-fig-line)" stroke-width="2"/></svg>Source of truth</li>
<li data-state="transform"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M14 2.5L19.5 8L14 13.5L8.5 8Z" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-accent)" stroke-width="3.5"/></svg>Transform</li>
<li data-state="product"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="2" width="24" height="12" rx="2" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="1.5"/></svg>Product</li>
<li data-state="declared"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><rect x="2" y="2" width="24" height="12" rx="2" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="1.5" stroke-dasharray="6 4"/></svg>Declared claims</li>
<li data-state="derives"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H20" fill="none" stroke="var(--cf-fig-line)" stroke-width="2"/><path d="M19 4L26 8L19 12Z" fill="var(--cf-fig-line)"/></svg>Derives</li>
<li data-state="declares"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H19" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="1.5" stroke-dasharray="6 4"/><path d="M19 4L26 8L19 12Z" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line-mid)" stroke-width="1.5"/></svg>Declares</li>
<li data-state="compares"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H18" fill="none" stroke="var(--cf-fig-line-mid)" stroke-width="1.5" stroke-dasharray="2 4"/><rect x="18" y="4" width="8" height="8" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line-mid)" stroke-width="1.5"/></svg>Compares bytes</li>
<li data-state="check"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><circle cx="14" cy="8" r="6.5" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="2.5"/><path d="M10.5 8L13 10.5L17.5 5.5" fill="none" stroke="var(--cf-fig-accent)" stroke-width="2"/></svg>Check that fails the build</li>
</ul>
<figcaption class="cf-fig-caption">The adapter is the only thing that derives, and the validator proves the result against repository bytes without running any project code.</figcaption>
<details class="cf-fig-details"><summary>Details</summary><p class="cf-fig-description">Repository Markdown sources, a filled source pin, feed the adapter, a transform diamond. The adapter derives the generated page, an outlined product box, and declares the evidence manifest, a dashed box. The validator, a check ring, compares the manifest against the repository bytes and the output bytes with dashed lines ending in open squares.</p>
<table><thead><tr><th>Part</th><th>Kind</th><th>Made by</th><th>Checked against</th></tr></thead>
<tbody><tr><td>repository sources</td><td>source</td><td>authors</td><td>the recorded commit</td></tr>
<tr><td>adapter</td><td>transform</td><td>Node, in the portal workspace</td><td>adapter tests</td></tr>
<tr><td>generated page</td><td>product</td><td>the adapter</td><td>output bytes in the manifest</td></tr>
<tr><td>evidence manifest</td><td>declared claims</td><td>the adapter</td><td><code>codeflow validate --portal</code></td></tr></tbody></table>
</details>
</figure>

Narrow: stack; sources, adapter, page and manifest run top to bottom, the
validator sits at the foot with its three comparison lines drawn upward.

### Chat form

```text
 repository sources ===> adapter ===> generated page
                            :
                            + - - -> evidence manifest
 (v) validator
     checks the manifest <~> the repository bytes
     checks the manifest <~> the output bytes
Legend: ===> derives  - -> declares  <~> compares bytes  (v) check that fails
Caption: Only the adapter derives; the validator proves it from bytes.
```

## 9. graph

```yaml
figure:
  id: epic-016-graph
  family: graph
  binding: authored
  question: "What depends on what in this epic, and what is on the critical path?"
  idea: "Six tasks form the critical path from the doctrine to the closeout; three run beside it."
  states: [node, done, trans]
  facts:
    - claim: "sixteen edges among nine tasks"
      source: "project-management/epics/EPC-016.md#plan-graph-plan-v3"
      derive: "the TASK_GRAPH v3 edge list"
  narrow: { recompose: list, drops: [], marks: same }
```

<figure class="cf-fig" data-cf-figure="graph">
<p class="cf-fig-title"><span class="cf-fig-number">Figure</span> · <span class="cf-fig-name">The nine tasks of EPC-016 and their sixteen dependencies</span></p>
<svg class="cf-fig-svg" viewBox="0 0 720 320" role="img" aria-labelledby="fg-graph-t fg-graph-d" font-family="var(--cf-fig-font)" font-size="14" font-weight="500" fill="var(--cf-fig-line)">
<title id="fg-graph-t">The nine tasks of EPC-016 and their sixteen dependencies</title>
<desc id="fg-graph-d">Ring nodes ranked left to right, each labelled with its task number: 058; then 070 and 062; then 059; then 060; then 061, 063 and 071; then 064. Heavy headless accent arcs mark the critical path, with direction from ranks; ordinary arcs have heads. The critical path runs 058 to 070 to 059 to 060 to 061 to 064. Ordinary arcs: 058 to 062, 058 to 061, 058 to 063, 059 to 061, 059 to 063, 059 to 071, 060 to 063, 060 to 071, 062 to 064, 063 to 064, 071 to 064.</desc>
<defs><marker id="fg-graph-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto"><path d="M0 0L10 5L0 10Z" fill="var(--cf-fig-line)"/></marker><marker id="fg-graph-c" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="9" markerHeight="9" markerUnits="userSpaceOnUse" orient="auto"><path d="M0 0L10 5L0 10Z" fill="var(--cf-fig-accent)"/></marker></defs>
<g data-state="trans" fill="none" stroke="var(--cf-fig-line)" stroke-width="1.75" marker-end="url(#fg-graph-a)"><path d="M75 171L155 229"/><path d="M78 157L502 83"/><path d="M78 162L502 208"/><path d="M307 154L503 86"/><path d="M308 164L502 206"/><path d="M306 169L504 281"/><path d="M417 167L503 203"/><path d="M412 173L508 277"/><path d="M188 237L642 163"/><path d="M537 204L643 166"/><path d="M533 278L647 172"/></g>
<g data-state="done" fill="none" stroke="var(--cf-fig-accent)" stroke-width="3" stroke-linecap="round"><path d="M76 151L154 109"/><path d="M186 108L274 152"/><path d="M308 160H382"/><path d="M415 150L505 90"/><path d="M536 89L644 151"/></g>
<g data-state="node" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="1.5"><circle cx="60" cy="160" r="17"/><circle cx="170" cy="100" r="17"/><circle cx="170" cy="240" r="17"/><circle cx="290" cy="160" r="17"/><circle cx="400" cy="160" r="17"/><circle cx="520" cy="80" r="17"/><circle cx="520" cy="210" r="17"/><circle cx="520" cy="290" r="17"/><circle cx="660" cy="160" r="17"/></g>
<g text-anchor="middle"><text x="60" y="165">058</text><text x="170" y="105">070</text><text x="170" y="245">062</text><text x="290" y="165">059</text><text x="400" y="165">060</text><text x="520" y="85">061</text><text x="520" y="215">063</text><text x="520" y="295">071</text><text x="660" y="165">064</text></g>
</svg>
<ul class="cf-legend" aria-label="Legend">
<li data-state="node"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><circle cx="14" cy="8" r="6.5" fill="var(--cf-fig-ground)" stroke="var(--cf-fig-line)" stroke-width="1.5"/></svg>Task, labelled inside</li>
<li data-state="done"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H19" fill="none" stroke="var(--cf-fig-accent)" stroke-width="3" stroke-linecap="round"/></svg>Critical path</li>
<li data-state="trans"><svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true"><path d="M2 8H20" fill="none" stroke="var(--cf-fig-line)" stroke-width="1.75"/><path d="M19 4L26 8L19 12Z" fill="var(--cf-fig-line)"/></svg>Depends on</li>
</ul>
<figcaption class="cf-fig-caption">The doctrine, the kit, the runtime, the chrome, the pages and the closeout are the critical path; the evaluation cases, Agent OS and the annotation fixes run beside it. Ids are TSK-058 to TSK-071.</figcaption>
<details class="cf-fig-details"><summary>Details</summary><p class="cf-fig-description">Ring nodes ranked left to right, each labelled with its task number: 058; then 070 and 062; then 059; then 060; then 061, 063 and 071; then 064. Heavy headless accent arcs mark the critical path, with direction from ranks; ordinary arcs have heads. The critical path runs 058 to 070 to 059 to 060 to 061 to 064. Ordinary arcs: 058 to 062, 058 to 061, 058 to 063, 059 to 061, 059 to 063, 059 to 071, 060 to 063, 060 to 071, 062 to 064, 063 to 064, 071 to 064.</p>
<table><thead><tr><th>Task</th><th>Depends on</th><th>On the critical path</th></tr></thead>
<tbody><tr><td>TSK-058</td><td>none</td><td>yes</td></tr><tr><td>TSK-070</td><td>058</td><td>yes</td></tr><tr><td>TSK-062</td><td>058</td><td>no</td></tr><tr><td>TSK-059</td><td>070</td><td>yes</td></tr><tr><td>TSK-060</td><td>059</td><td>yes</td></tr><tr><td>TSK-061</td><td>058, 059, 060</td><td>yes</td></tr><tr><td>TSK-063</td><td>058, 059, 060</td><td>no</td></tr><tr><td>TSK-071</td><td>059, 060</td><td>no</td></tr><tr><td>TSK-064</td><td>061, 062, 063, 071</td><td>yes</td></tr></tbody></table>
</details>
</figure>

Narrow: list; each task becomes a row with its dependencies named and the
critical path rows marked with the heavy accent rule, no arcs drawn.

### Chat form

```text
 rank 1     2          3          4          5          6
 (058)====(070)====(059)====(060)====(061)====(064)
                                         <-058      <-062
                                         <-059      <-063
                                                    <-071
           (062)                         (063)
           <-058                         <-058 <-059 <-060
                                         (071)
                                         <-059 <-060
Legend: (n) task TSK-n  == critical path  <-n depends on task n
Caption: The critical path runs 058, 070, 059, 060, 061 and 064.
```
