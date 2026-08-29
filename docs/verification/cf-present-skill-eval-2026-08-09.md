# cf-present / cf-docs-portal skill evaluation — 2026-08-09

**Suite:** `cf-evaluate-model` validate-suite + case  
`complex-review-uses-declarative-presentation` (canary capability)  
**Binary:** `./target/debug/codeflow` 3.0.0 (integration branch)  
**Harness seats attempted:** Grok authoring trial; Claude Code lifecycle consult  

---

## 1. Suite validity (hard gate)

| Check | Result |
|-------|--------|
| `eval_kit.py validate-suite` after skill rewrites | **Initially FAIL** — missing CF-PRES-002/004 and CF-PORTAL-002 source markers after SKILL.md rewrite |
| After restoring required marker phrases + mirror sync | **PASS** `suite valid: sha256:8231c27e…` |

**Finding:** Rewriting skills without re-running validate-suite is a silent contract break. Marker phrases must remain literal substrings of live `.agents/skills/**`.

---

## 2. Packaging gate (skills must reach consumers)

| Check | Result |
|-------|--------|
| Materialize fixture with debug binary (before fix) | **FAIL** — fixture had `SKILL.md` + `document-authoring` but **no** `resources/` or `visual-craft.md` |
| Root cause | `assets/base/scaffold-manifest.toml` only listed a subset of skill files; new resources were never managed entries |
| Fix | Added managed entries for present/portal `resources/*` and `references/visual-craft.md` to **both** `.claude` and `.agents` destinations |
| Materialize after rebuild | **PASS** — `how-presentation-works.md` and peers present under fixture `.agents` and `.claude` skill trees |

**Finding:** Writing skill files under `assets/base/agents/skills` is **not** enough. Without scaffold-manifest entries, installs and eval fixtures never see them. This alone would make “skills exist in repo” look successful while models never load the thinking guide.

---

## 3. Behavioral contrast (case-shaped authoring)

Fixture brief: ship vs hold; Windows native NOT RUN; require explicit verdict.

### Control — text-wall document (pre-skill thinking)

Path: `/tmp/cf-present-skill-eval-run-a/baseline-textwall-document.json`  

Order: `narrative → bullets → narrative → comparison → bullets → feedback_prompt`  

| Signal | Result |
|--------|--------|
| Governing visual first | fail |
| Not prose-first | fail |
| Status for evidence (not bullet list) | fail |
| Explicit verdict prompt | fail (“What do you think?”) |
| **Overall** | **FAIL** (expected) |

### Candidate — skill-directed authoring

Path: `/tmp/cf-present-skill-eval-run-a/candidate-document.json`  
Re-open for visual inspect: session `b5056568-e579-451b-9af9-00459c982be8` port `53768`  
Thinking artifact: `candidate-thinking.md`  

Order: `diagram → narrative → comparison → status → callout → decision → feedback_prompt`  

| Case signal (JSON / DOM) | Result |
|--------------------------|--------|
| closed_v1_document_authored | pass |
| governing_visual first (diagram block 0) | pass |
| information_bearing_comparison_and_status | pass |
| explicit_feedback_prompt | pass |
| native Windows not claimed pass (`data-state=not-run`) | pass |
| diagram encodes ship/hold + NOT RUN | pass |
| **JSON/DOM structural grade** | **PASS** |

### 3b. Visual inspection (required; was missing on first report)

**Method:** Playwright on authenticated `/app/` after bootstrap cookie; wait for Mermaid
`data-cf-diagram=ready` + SVG; screenshots saved under workspace:

- `eval-present-fold.png` — 1440×900 first screen  
- `eval-present-full.png` — full scroll with Comment armed  
- `eval-present-comment-armed.png` — Comment + mode strip + Notes rail  
- `eval-present-mobile-fold.png` — 390 CSS px fold  
- evidence/verdict scrolls captured separately  

**Observed (eye, not just selectors):**

| Check | Result | Notes |
|-------|--------|-------|
| Diagram renders as SVG (not raw Mermaid dump) | pass | ~1136×287 SVG; state `ready` |
| Fold: figure before prose | pass | Diagram fully in first 900px; one-line frame under it |
| 5s claim readable from figure | **partial** | Ship/Hold fork and Native Windows NOT RUN visible; node labels cramped (`macOS arm64PASS`); long **figcaption** under figure steals fold attention |
| Comparison peer columns | pass | Ship vs Hold benefit/cost clear |
| Status evidence scan | pass | Hollow marks for not-run rows; Windows runtime + production correct |
| Warning callout | pass | No Windows claim from cross-build |
| Explicit verdict demand | pass | Full prompt at bottom |
| Left TOC quality | **fail** | Feedback prompt text is used as nav label and wraps into a wall in the rail (“Give the exact release decision…”) — destroys scannability |
| Comment chrome | pass | Armed: filled Comment, TEXT/CLICK/DRAG strip, Notes empty + Submit dashed, rail only when armed |
| Permanent `+` while Comment on | pass | 0 visible permanent plus |
| 390px overflow | pass | `scrollWidth == clientWidth` |
| Selection → float chip | **not verified** | Programmatic selection did not open float (may need real pointer path) |

**Visual judgment (honest):** better than a text-wall present; **not** a clean utility-board quality bar. Structural authoring worked; presentation craft still hurt by (1) prompt-as-TOC-label, (2) long figcaption under the primary figure, (3) Mermaid label wrapping. Do **not** treat JSON-only PASS as visual acceptance.

**Overall after first visual inspect:** **conditional pass** — case structure OK; craft issues remained (TOC, caption).

---

## 3c. Fixes + multi-pass retest (after “did you actually look?”)

### Code fixes landed

| Issue | Fix |
|-------|-----|
| TOC dumped full `feedback_prompt` | `review_label()` → `"Feedback request"`; all labels truncated to ≤40 chars (`document.rs`) |
| Figcaption prose wall under primary figure | Visible caption = `acc_title` only; description in `.sr-only` (`render.rs` + CSS) |
| Cramped Mermaid node text | Authoring doc v2 uses `label<br/>PASS` style nodes |
| Comment contrast axe fail (editorial/light) | Stronger idle Comment control styles + solid fill for contrast |
| Skills not shipping | Already fixed via scaffold-manifest (prior) |

### Automated gates

- `cargo test -p codeflow-present review_label_keeps_nav…` **pass**
- `npm run check:browser` (Node 26.4.0) **pass** after contrast fix

### Visual multi-pass on rebuilt binary (session `2505d2ac-…` port 54392)

| Round | Check | Result |
|-------|--------|--------|
| A desktop fold | SVG diagram first; short TOC; caption height ≤28px | **pass** |
| B Comment armed | pressed, rail open, mode strip | **pass** |
| C evidence | Native Windows `not-run` | **pass** |
| D mobile 390 | no horizontal overflow | **pass** |
| E cold reload | TOC still short (`Feedback request`) | **pass** |

Screenshots: `final-A-desktop.png` … `final-E-reload.png`, `retest-final-fold.png`.

**Aggregate visual retest:** **allPass = true**

Honest residual: Mermaid is still Mermaid (not a custom stage board); selection→float still not proven with real pointer path in this run.

---

## 4. Claude Code independent consult

| Attempt | Transport | Result |
|---------|-----------|--------|
| turn-1 full skill review | `codeflow delegate` lifecycle + interactive `claude --model fable` | **StopFailure `rate_limit` (429)** |
| turn-1 compact skill+candidate grade | same | **StopFailure `rate_limit` (429)** |

**Recorded:** lifecycle launch, arm, accept, and failed terminal with request ids (see  
`/tmp/cf-claude-present-skill-review-result.json` and `…result2.json`).  

**Not claimed:** Claude judgment verdict. Re-run when quota recovers; do not substitute host-lineage self-review as “Claude approved.”

---

## 5. Conclusions

1. **Docs alone were insufficient** — suite markers + scaffold packaging proved the skill could look complete in git while failing install and contract checks.
2. **Thinking-first skill content works when loaded** — candidate authoring produced carrier-first structure and clear verdict; text-wall control fails the same grader.
3. **Must ship resources via scaffold-manifest** — fixed in this eval cycle.
4. **Claude independent review is still required** for dual-lineage judgment; blocked this session by rate limit; retry is mandatory before calling the skill pack “externally approved.”

## 6. Fixes landed from this eval

1. Restored CF-PRES / CF-PORTAL source markers in live skills (suite green).
2. Registered skill `resources/*` and `visual-craft.md` in `scaffold-manifest.toml` so installs and fixtures actually receive them.
3. Rebuilt binary; re-materialized fixture proves `how-presentation-works.md` ships.
4. Added CF-PRES-004 hard source markers on `how-presentation-works.md` (thinking doctrine is now contract-checked).

## 7. Residual actions

- [ ] Re-run Claude lifecycle consult when 429 clears; archive `last_assistant_message` here
- [ ] Full native subject trial of `complex-review-uses-declarative-presentation` in Claude Code / Codex interactive seats (eval_kit protocol: no headless substitute)
- [ ] Canary full case × 3 for promotion — not done here
