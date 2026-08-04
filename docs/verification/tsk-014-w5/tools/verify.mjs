/* W5 verifier — the machine-decided half of rubric.v5.md, and nothing else.
 *
 * It reads the pinned rubric and registration, the checks written by
 * render.mjs, and the repository itself, and it decides section A. It decides
 * no G gate, because every G gate needs an observer who authored nothing here
 * and this session authored all of it.
 *
 * A clean run means the board is internally consistent, accessible to the
 * automated checks, reproducible and admissible. It has never meant that a
 * composition communicates, and on the previous board a clean run passed a
 * figure that stated a repository fact incorrectly. A18 exists because of that.
 */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

const ROOT = path.resolve(import.meta.dirname, '..');
const REPO = path.resolve(ROOT, '..', '..', '..');
const CHECKS = path.join(ROOT, 'checks');

const read = (p) => fs.readFileSync(path.join(ROOT, p), 'utf8');
const json = (p) => JSON.parse(read(p));
const check = (name) => (fs.existsSync(path.join(CHECKS, `${name}.json`)) ? json(`checks/${name}.json`) : null);
const sha = (p) => crypto.createHash('sha256').update(fs.readFileSync(path.join(ROOT, p))).digest('hex');

const registry = json('registry.json');
const amendments = fs.existsSync(path.join(ROOT, 'amendments.json')) ? json('amendments.json') : { amendments: [] };

const results = [];
const gate = (id, what, ok, detail) => {
  results.push({ id, what, verdict: ok === null ? 'not run' : ok ? 'pass' : 'FAIL', detail });
  return ok;
};

/* ---- the locks --------------------------------------------------------- */
for (const [file, lock] of [['rubric.v5.md', 'rubric.v5.lock.json'], ['registry.json', 'registry.lock.json']]) {
  const declared = json(lock);
  const actual = sha(file);
  gate('LOCK', `${file} matches ${lock}`, declared.sha256 === actual,
    declared.sha256 === actual ? `sha256 ${actual.slice(0, 16)}…`
      : `pinned ${declared.sha256.slice(0, 16)}… but file is ${actual.slice(0, 16)}… — a pinned file was edited`);
}
/* The earlier boards stay byte-identical. W5 never rewrites its own history.
 * Each board named the pinned file differently — W2 and W3 use `rubric`, W4 uses
 * `file`, and one W3 lock carries no digest at all — so the shape is read rather
 * than assumed. A lock with no digest is reported as unverifiable, never as a
 * pass: that distinction is the whole point of this section. */
for (const board of ['tsk-014-w2', 'tsk-014-w3', 'tsk-014-w4']) {
  const dir = path.resolve(ROOT, '..', board);
  const locks = fs.existsSync(dir) ? fs.readdirSync(dir).filter((f) => f.endsWith('.lock.json')) : [];
  for (const lockName of locks) {
    const lock = JSON.parse(fs.readFileSync(path.join(dir, lockName), 'utf8'));
    const named = lock.file ?? lock.rubric;
    if (!named || !lock.sha256) {
      gate('PRIOR', `${board}/${lockName}`, null,
        `carries no single pinned file with a digest (${Object.keys(lock).join(', ')}) — unverifiable here, not a pass`);
      continue;
    }
    const target = path.join(dir, named);
    if (!fs.existsSync(target)) { gate('PRIOR', `${board}/${named}`, false, 'pinned file missing'); continue; }
    const actual = crypto.createHash('sha256').update(fs.readFileSync(target)).digest('hex');
    gate('PRIOR', `${board}/${named} unchanged`, actual === lock.sha256,
      actual === lock.sha256 ? 'byte-identical' : 'DRIFTED — a previous board was edited');
  }
}

/* ---- A21 — intent and outcome are separate, and outcome is earned ------- */
{
  const OUTCOMES = ['pending', 'achieved', 'partial', 'failed', 'withdrawn'];
  const entries = [
    ...registry.blind_findings.map((f) => ({ where: `blind_findings.${f.id}`, ...f })),
    ...Object.entries(registry.part_d_defects)
      .filter(([, v]) => Array.isArray(v))
      .flatMap(([group, list]) => list.map((d) => ({ where: `part_d_defects.${group}.${d.id}`, ...d }))),
    ...registry.directions.map((d) => ({ where: `directions.${d.id}`, target_disposition: d.target_status, ...d })),
    ...registry.portal_families.filter((f) => f.demonstrated).map((f) => ({ where: `portal_families.${f.id}`, target_disposition: f.targets ? 'repair' : 'none', ...f })),
  ];
  const missing = entries.filter((e) => !e.target_disposition && !e.target_status);
  const badOutcome = entries.filter((e) => e.outcome && !OUTCOMES.includes(e.outcome));
  const unearned = entries.filter((e) => e.outcome && e.outcome !== 'pending' && e.outcome !== 'withdrawn');
  gate('A21', 'every disposition separates intent from outcome', missing.length === 0 && badOutcome.length === 0,
    missing.length || badOutcome.length
      ? `${missing.length} without a target_disposition, ${badOutcome.length} with an outcome outside the vocabulary`
      : `${entries.length} entries, each with a target_disposition and an outcome in ${OUTCOMES.join('/')}`);
  gate('A21', 'no outcome is claimed that this session may not claim', unearned.length === 0,
    unearned.length ? `${unearned.map((e) => e.where).join(', ')} claim a non-pending outcome`
      : 'every registered outcome is still pending — correct: no artefact outcome may be recorded by its own author');
}

/* ---- A22 — no uncompleted turn is cited -------------------------------- */
{
  const completed = new Set(registry.evidence_provenance.completed_turns);
  const cited = registry.blind_findings.map((f) => f.source_turn).filter(Boolean);
  const bad = cited.filter((t) => !completed.has(t));
  gate('A22', 'every finding cites a completed observer turn', bad.length === 0,
    bad.length ? `cites ${[...new Set(bad)].join(', ')}, which did not complete`
      : `${cited.length} findings, all citing ${[...completed].join(' or ')}`);

  const notEvidence = registry.evidence_provenance.not_evidence.id;
  const blob = JSON.stringify({ ...registry, evidence_provenance: null });
  gate('A22', `the uncompleted ${notEvidence} is cited nowhere`, !blob.includes(notEvidence),
    blob.includes(notEvidence) ? `${notEvidence} appears outside evidence_provenance` : 'appears only in evidence_provenance.not_evidence');

  const declared = registry.evidence_provenance;
  gate('A22', 'the reading boundary is declared', Boolean(declared.what_this_session_could_read && declared.what_this_session_could_not_read),
    'the registration states what this session read and what it could not');
}

/* ---- A17 — a control exists, or the question is unscorable -------------- */
{
  const need = [
    ...registry.cases.filter((c) => c.scorable).map((c) => ({ kind: 'case', id: c.id })),
    ...registry.portal_families.filter((f) => f.demonstrated && f.scorable).map((f) => ({ kind: 'family', id: f.id })),
  ];
  /* d1 is the control for orient-model, d2 for record-trust, d3 for area-drilldown:
   * same subject source, no composition. The mapping is declared here rather than
   * inferred from a name. */
  const CONTROL = { p1: 'p1', p2: 'p2', p3: 'p3', 'orient-model': 'd1', 'record-trust': 'd2', 'area-drilldown': 'd3' };
  const missing = [];
  for (const item of need) {
    const b = CONTROL[item.id];
    const have = b && ['baseline.html', 'chat.md', 'markdown.md'].every((f) => fs.existsSync(path.join(ROOT, 'baselines', b, f)));
    if (!have) missing.push(`${item.kind} ${item.id}`);
  }
  gate('A17', 'every scorable surface has a plain control', missing.length === 0,
    missing.length ? `no control for ${missing.join(', ')}` : `${need.length} scorable surfaces, each with chat, Markdown and plain-HTML controls`);
}

/* ---- A16 — no uninspected corner --------------------------------------- */
{
  const coverage = check('coverage');
  const geometry = check('geometry');
  if (!coverage) gate('A16', 'the full width-by-mode matrix is rendered', null, 'checks/coverage.json absent — render first');
  else {
    const pages = [...new Set(coverage.map((c) => c.page))];
    const want = registry.thresholds.modes.length * Object.keys(registry.thresholds.viewports).length;
    const short = pages.filter((p) => coverage.filter((c) => c.page === p).length !== want);
    gate('A16', 'every artefact is rendered at every width in every mode', short.length === 0,
      short.length ? `${short.join(', ')} missing corners` : `${pages.length} artefacts x ${want} contexts = ${coverage.length} renders, no gap`);
    gate('A16', 'observation coverage is NOT claimed', true,
      'rendering coverage is complete; the W4 corners the observer never inspected remain unobserved, and so does every W5 corner, until a fresh answer-blind observer reads this board');
    void geometry;
  }
}

/* ---- A19 — a narrow band is a composition, not an elongation ------------ */
{
  const geometry = check('geometry');
  if (!geometry) gate('A19', 'narrow elongation within the declared ceiling', null, 'checks/geometry.json absent');
  else {
    const max = registry.thresholds.narrow_elongation_max;
    const by = {};
    for (const g of geometry) { by[g.page] = by[g.page] ?? {}; by[g.page][`${g.mode}/${g.viewport}`] = g.height; }
    const over = Object.entries(by)
      .map(([p, v]) => ({ page: p, ratio: v['light/mobile'] / v['light/desktop'] }))
      .filter((r) => r.ratio > max);
    gate('A19', `narrow height within ${max}x its wide height`, over.length === 0,
      over.length ? over.map((r) => `${r.page} ${r.ratio.toFixed(2)}x`).join(', ')
        : `worst ratio ${Math.max(...Object.values(by).map((v) => v['light/mobile'] / v['light/desktop'])).toFixed(2)}x. See amendments A-04: this gate is weak and did not catch the artefact it was written for.`);
  }
}

/* ---- A15 — two independently legible channels, and the mark floor ------- */
{
  const marks = check('marks');
  if (!marks) gate('A15', 'declared channels present and above the mark floor', null, 'checks/marks.json absent');
  else {
    const floor = registry.thresholds.mark_floor_px;
    /* The registry scopes the floor to "the smallest information-bearing
     * dimension … a tick's length, a cell's side, a stroke's separation". For a
     * one-dimensional mark — a line, a path, a rule — that dimension is its
     * EXTENT; for a two-dimensional one — a cell, a disc, a chip — it is the
     * shorter side. Measuring min(width, height) for both reports every
     * horizontal rule on the board as a 0px mark, which is a bug in the ruler,
     * not a defect in the drawing. A <g> is a container and is not measured. */
    const ONE_D = new Set(['line', 'path', 'polyline']);
    const dim = (k) => (ONE_D.has(k.tag) ? Math.max(k.w, k.h) : Math.min(k.w, k.h));
    const measured = marks.flatMap((m) => m.marks.filter((k) => k.tag !== 'g').map((k) => ({ ...k, render: m.render, d: dim(k) })));
    const under = measured.filter((k) => k.d < floor);
    gate('A15', `every drawn state mark at or above the ${floor}px mark floor`, under.length === 0,
      under.length
        ? `${under.length} under floor: ${under.slice(0, 4).map((k) => `${k.render} ${k.state} ${k.tag} ${k.d}px`).join('; ')}`
        : `${measured.length} state marks measured across ${marks.length} renders; smallest information-bearing dimension ${Math.min(...measured.map((k) => k.d))}px against a ${floor}px floor`);

  }
}

/* ---- A15, the rendered half ---------------------------------------------
 * A15 requires that `checks/channels.json` record each declared channel as
 * PRESENT IN THE RENDERED MARKUP. Two implementations of this gate have now
 * been wrong, both found by independent review rather than by anything here:
 *
 *   1. The first read registry.channels and asserted that the strings
 *      `channel_a` and `channel_b` were non-empty. A declaration checking
 *      itself. A15 passed on nothing.
 *   2. The second measured real tokens but selected the evidence with a
 *      predicate ending in `|| true`, against a field the registry does not
 *      have — so evidence was matched by state NAME alone and never confined to
 *      its distinction. It also proved only that "at least two of interior,
 *      edge, geometry, overlay differ", which is not what the rubric asks: the
 *      rubric asks for the DECLARED channels. A figure could have lost both
 *      declared channels and passed on an unregistered difference.
 *
 * What is checked now. Every piece of evidence is selected by an explicit
 * binding in amendments.json A-10 — artefact to page and rendered distinction —
 * and by nothing else. Every declared channel is tied to one named measured
 * channel and to an exact per-state token map. The binding quotes the pinned
 * registry prose verbatim and the verifier compares it character for character,
 * so a binding cannot claim to bind a channel it is not binding. Where the
 * built encoding diverges from the pinned prose, the named amendment must carry
 * a matching `authorises` entry: an amendment id on its own authorises nothing.
 *
 * Everything below fails closed. A thing that cannot be checked is a failure,
 * never a pass. */
{
  const channels = check('channels');
  const MEASURED = ['interior', 'edge', 'geometry', 'overlay', 'terminal', 'entry'];
  const amendmentById = new Map((amendments.amendments ?? []).map((a) => [a.id, a]));
  const bindingAmendment = amendmentById.get('A-10');
  const WITHDRAWN = { 'cases/p3b': 'A-03' };
  const problems = [];
  const push = (m) => problems.push(m);

  /* an amendment authorises a divergence only if it SAYS it does */
  const authorises = (id, want) => {
    const a = amendmentById.get(id);
    if (!a || !Array.isArray(a.authorises)) return false;
    return a.authorises.some((e) => Object.entries(want).every(([k, v]) => e[k] === v));
  };

  if (!channels) {
    push('checks/channels.json is absent');
  } else if (!bindingAmendment || !Array.isArray(bindingAmendment.bindings)) {
    push('amendments.json A-10 carries no channel bindings');
  } else {
    const material = registry.channels.filter((c) => c.matters === true);
    const bindings = bindingAmendment.bindings;

    /* ---- exhaustiveness, both directions ---------------------------------- */
    const boundArtefacts = new Set(bindings.map((b) => b.artefact));
    if (boundArtefacts.size !== bindings.length) push('A-10 binds an artefact more than once');
    for (const decl of material) {
      const withdrawn = WITHDRAWN[decl.artefact];
      if (withdrawn) {
        if (!amendmentById.has(withdrawn)) push(`${decl.artefact}: absent from the board with no amendment authorising it`);
        if (boundArtefacts.has(decl.artefact)) push(`${decl.artefact}: withdrawn yet still bound`);
        continue;
      }
      if (!boundArtefacts.has(decl.artefact)) push(`${decl.artefact}: material distinction with no binding`);
    }
    const materialIds = new Set(material.map((c) => c.artefact));
    for (const b of bindings) if (!materialIds.has(b.artefact)) push(`A-10 binds ${b.artefact}, which the registry does not mark material`);

    /* every rendered distinction on the board must be bound to something */
    const renderedPairs = new Set();
    for (const r of channels) for (const st of r.states) renderedPairs.add(`${r.page}::${st.distinction}`);
    const boundPairs = new Set(bindings.map((b) => `${b.page}::${b.rendered_distinction}`));
    for (const pair of renderedPairs) if (!boundPairs.has(pair)) push(`rendered distinction ${pair} is bound to nothing`);
    for (const pair of boundPairs) if (!renderedPairs.has(pair)) push(`binding ${pair} matches nothing on the board`);

    /* ---- per binding ------------------------------------------------------ */
    for (const b of bindings) {
      const decl = material.find((c) => c.artefact === b.artefact);
      if (!decl) continue;

      if (b.state_map_amended_by) {
        for (const [from, to] of Object.entries(b.state_map)) {
          if (from !== to && !authorises(b.state_map_amended_by, { artefact: b.artefact, kind: 'state_map', from, to })) {
            push(`${b.artefact}: state ${from}->${to} is not authorised by ${b.state_map_amended_by}`);
          }
        }
      } else if (Object.entries(b.state_map).some(([k, v]) => k !== v)) {
        push(`${b.artefact}: state vocabulary diverges with no amendment`);
      }
      const declaredStates = decl.states.slice().sort().join(',');
      const mappedFrom = Object.keys(b.state_map).slice().sort().join(',');
      if (declaredStates !== mappedFrom) push(`${b.artefact}: state_map covers ${mappedFrom}, the registry declares ${declaredStates}`);
      const want = [...new Set(Object.values(b.state_map))];

      /* the binding must quote the pinned registry, and name a real channel */
      for (const ch of b.channels) {
        if (!MEASURED.includes(ch.measured)) push(`${b.artefact}: measured channel "${ch.measured}" is outside the probe's vocabulary`);
        if (ch.registry_field) {
          if (!['channel_a', 'channel_b'].includes(ch.registry_field)) push(`${b.artefact}: unknown registry field ${ch.registry_field}`);
          else if (ch.amended_by && !authorises(ch.amended_by, { artefact: b.artefact, kind: 'channel_measured', channel: ch.registry_field, to: ch.measured })) {
            push(`${b.artefact}.${ch.registry_field}: measured as "${ch.measured}" without a matching authorises entry in ${ch.amended_by}`);
          }
        } else if (!ch.amended_by || !authorises(ch.amended_by, { artefact: b.artefact, kind: 'channel_added', channel: ch.measured })) {
          push(`${b.artefact}: channel "${ch.measured}" is not declared by the registry and not authorised as added`);
        }
        const covered = Object.keys(ch.expected ?? {}).slice().sort().join(',');
        if (covered !== want.slice().sort().join(',')) push(`${b.artefact}.${ch.measured}: expected covers ${covered}, the states are ${want.slice().sort().join(',')}`);
        /* a channel that does not vary carries nothing */
        if (new Set(Object.values(ch.expected ?? {})).size < 2) push(`${b.artefact}.${ch.measured}: the same token for every state — it carries nothing`);
      }
      for (const field of ['channel_a', 'channel_b']) {
        if (!b.channels.some((c) => c.registry_field === field)) push(`${b.artefact}: registered ${field} is bound to no measured channel`);
      }

      /* ---- the render itself, in every context --------------------------- */
      const contexts = channels.filter((r) => r.page === b.page);
      if (contexts.length !== 6) { push(`${b.artefact}: ${contexts.length} contexts measured, not 6`); continue; }

      for (const ctx of contexts) {
        /* selected by the BOUND distinction, and by nothing else */
        const mine = ctx.states.filter((st) => st.distinction === b.rendered_distinction);
        if (mine.some((st) => st.measurable === false)) { push(`${ctx.render}: a state element draws nothing`); continue; }
        const seen = [...new Set(mine.map((st) => st.state))].sort();
        const expectSeen = want.slice().sort();
        if (seen.join(',') !== expectSeen.join(',')) {
          push(`${ctx.render}: ${b.rendered_distinction} draws [${seen}], the binding declares [${expectSeen}]`);
          continue;
        }
        const tok = {};
        for (const st of mine) {
          tok[st.state] = tok[st.state] ?? {};
          for (const ch of b.channels) {
            const v = st[ch.measured];
            if (v === undefined || v === null || String(v).includes('unknown')) {
              push(`${ctx.render}: "${st.state}" channel "${ch.measured}" is ${v === undefined ? 'absent' : v}`);
            }
            const norm = String(v).replace(/:\d+(\.\d+)?x\d+(\.\d+)?/g, '');
            (tok[st.state][ch.measured] = tok[st.state][ch.measured] ?? new Set()).add(norm);
          }
        }
        const flat = {};
        for (const st of want) {
          flat[st] = {};
          for (const ch of b.channels) {
            const set = tok[st]?.[ch.measured];
            if (!set) { push(`${ctx.render}: "${st}" has no ${ch.measured}`); continue; }
            if (set.size !== 1) push(`${ctx.render}: "${st}" renders ${set.size} different "${ch.measured}" tokens`);
            const got = [...set][0];
            flat[st][ch.measured] = got;
            /* the registered semantics, proved — not an incidental difference */
            if (got !== ch.expected[st]) push(`${ctx.render}: "${st}" ${ch.measured} is "${got}", the binding requires "${ch.expected[st]}"`);
          }
        }
        /* every pair separated by at least two of the NAMED channels */
        for (let i = 0; i < want.length; i += 1) {
          for (let j = i + 1; j < want.length; j += 1) {
            const a = want[i]; const c = want[j];
            const differ = b.channels.filter((ch) => flat[a]?.[ch.measured] !== flat[c]?.[ch.measured]).map((ch) => ch.measured);
            if (differ.length < 2) push(`${ctx.render}: "${a}" vs "${c}" separated by ${differ.length ? `only ${differ[0]}` : 'nothing'}`);
          }
        }
      }

      /* hue-independence, proved: same viewport, both modes, same tokens */
      for (const vp of ['desktop', 'tablet', 'mobile']) {
        const light = contexts.find((r) => r.mode === 'light' && r.viewport === vp);
        const dark = contexts.find((r) => r.mode === 'dark' && r.viewport === vp);
        if (!light || !dark) { push(`${b.page}: ${vp} missing a mode, so hue-independence cannot be proved`); continue; }
        const pick = (r) => Object.fromEntries(r.states.filter((st) => st.distinction === b.rendered_distinction)
          .map((st) => [st.state, b.channels.map((ch) => String(st[ch.measured]).replace(/:\d+(\.\d+)?x\d+(\.\d+)?/g, '')).join('|')]));
        const l = pick(light); const dk = pick(dark);
        for (const st of Object.keys(l)) {
          if (l[st] !== dk[st]) push(`${b.page} ${vp}: "${st}" encodes differently in light and dark — the token depends on colour`);
        }
      }
    }
  }

  /* the binding must quote the pinned file, so it cannot drift from what it binds */
  if (bindingAmendment && Array.isArray(bindingAmendment.bindings)) {
    for (const b of bindingAmendment.bindings) {
      const decl = registry.channels.find((c) => c.artefact === b.artefact);
      if (!decl) continue;
      for (const ch of b.channels) {
        if (!ch.registry_field) continue;
        /* a missing quote is a missing proof, not a pass */
        if (ch.registered === undefined) {
          problems.push(`${b.artefact}.${ch.registry_field}: the binding does not quote the pinned registry at all`);
        } else if (ch.registered !== decl[ch.registry_field]) {
          problems.push(`${b.artefact}.${ch.registry_field}: the binding misquotes the pinned registry`);
        }
      }
    }
  }

  const bound = (bindingAmendment?.bindings ?? []).length;
  const hue = problems.filter((x) => x.includes('light and dark'));
  gate('A15', 'every declared channel is bound to measured evidence and present in all six contexts', problems.length === 0,
    problems.length
      ? `${problems.length} problem(s): ${[...new Set(problems)].slice(0, 4).join(' | ')}`
      : `${bound} bound distinctions x 6 contexts. Evidence selected only by the bound rendered distinction; every registered channel_a/channel_b tied to one measured channel and to an exact per-state token map; every divergence covered by a matching authorises entry; every pair of states separated by at least two of the NAMED channels`);

  gate('A15', 'no channel token depends on colour (proved in light and dark)', hue.length === 0,
    hue.length ? hue[0] : `${bound} distinctions: every state's bound tokens identical between the light and dark render of the same viewport, so no token carries hue`);

  const w = Object.entries(WITHDRAWN);
  gate('A15', 'withdrawn artefacts are reconciled, not skipped', w.every(([, id]) => amendmentById.has(id)),
    `${w.map(([a, id]) => `${a} (${id})`).join(', ')} — declared in the pinned registry, absent from the board, authorised by a named amendment rather than silently dropped`);
}

/* ---- A18 — declared facts checked against the repository ---------------- */
{
  const fidelity = check('fidelity');
  const source = fs.readFileSync(path.join(ROOT, 'shared/repo-layers.js'), 'utf8');
  /* re-derive from the shared source, not from the drawing */
  const spineIds = [...source.matchAll(/\{ id: "([a-z-]+)", label: "[^"]+", layer:/g)].map((m) => m[1]);
  const authorityIds = [...source.matchAll(/\n      id: "([a-z-]+)", band: "/g)].map((m) => m[1]);
  const contract = fs.readFileSync(path.join(REPO, 'AGENTS.md'), 'utf8');
  const declaredSpine = 'capability → epic/task → ADR/spec → PR →';
  gate('A18', 'the spine order comes from the operating contract itself', contract.includes(declaredSpine),
    contract.includes(declaredSpine) ? `AGENTS.md states "${declaredSpine} ledger"` : 'the contract sentence this figure quotes was not found in AGENTS.md');

  if (!fidelity) gate('A18', 'the drawn spine equals the declared spine', null, 'checks/fidelity.json absent');
  else {
    const orient = fidelity.filter((f) => f.page === 'portal-orient-model');
    const wrongOrder = orient.filter((f) => f.spine.join(',') !== spineIds.join(','));
    gate('A18', 'the drawn spine equals the declared spine, in every context', wrongOrder.length === 0,
      wrongOrder.length
        ? `${wrongOrder.length}/${orient.length} renders draw ${wrongOrder[0].spine.join(' → ')} instead of ${spineIds.join(' → ')}`
        : `${orient.length} renders, each drawing ${spineIds.join(' → ')} — the order W4 got wrong`);
    const merged = orient.filter((f) => new Set(f.spine).size !== spineIds.length);
    gate('A18', 'no two spine nodes share one drawn position', merged.length === 0,
      merged.length ? `${merged.length} renders draw ${new Set(merged[0].spine).size} distinct nodes, not ${spineIds.length}` : `${spineIds.length} distinct nodes in every render`);
    const shortAuth = orient.filter((f) => f.authorities.join(',') !== [...authorityIds].sort().join(','));
    gate('A18', 'every authority kind the contract declares is drawn and named', shortAuth.length === 0,
      shortAuth.length
        ? `${shortAuth.length} renders draw ${shortAuth[0].authorities.length} of ${authorityIds.length} kinds`
        : `all ${authorityIds.length} kinds drawn in every render — W4 collapsed them to 3`);
  }
}

/* ---- A20 — the reuse proof varies -------------------------------------- */
{
  const src = fs.readFileSync(path.join(ROOT, 'directions/c-recipes/index.html'), 'utf8');
  const reached = [...src.matchAll(/label: '[A-E] · [^']+', reached: (\d)/g)].map((m) => Number(m[1]));
  const varies = new Set(reached).size > 1;
  gate('A20', 'the second binding of a reused recipe varies', varies,
    varies ? `the direction binding spans reached ${Math.min(...reached)}–${Math.max(...reached)} across ${reached.length} instances`
      : `all ${reached.length} instances identical — a proof that the mechanism runs, not that it carries anything`);
}

/* ---- A1 — the answer is not in the visible text ------------------------- */
{
  const text = check('text');
  if (!text) gate('A1', 'no registered forbidden phrase in any render', null, 'checks/text.json absent');
  else {
    const hits = [];
    for (const c of registry.cases) {
      const renders = text.filter((t) => t.render.startsWith(`${c.id}-`) && !t.render.includes('baseline'));
      for (const r of renders) {
        const visible = r.visible.toLowerCase();
        for (const phrase of c.forbidden_phrases) if (visible.includes(phrase.toLowerCase())) hits.push(`${r.render}: "${phrase}"`);
        for (const p of c.forbidden_patterns) if (new RegExp(p.pattern, 'i').test(r.visible)) hits.push(`${r.render}: /${p.pattern}/`);
      }
    }
    gate('A1', 'no registered forbidden phrase or pattern in any candidate render', hits.length === 0,
      hits.length ? hits.slice(0, 5).join('; ') : `${registry.cases.length} cases checked against every candidate render`);
  }
}

/* ---- the collision gate (A-05) ----------------------------------------- */
{
  const collisions = check('collisions');
  if (!collisions) gate('A-05', 'no text overprints inside a figure', null, 'checks/collisions.json absent');
  else {
    const hits = collisions.filter((c) => c.hits.length);
    const total = hits.reduce((n, c) => n + c.hits.length, 0);
    gate('A-05', 'no text overprints inside a figure', total === 0,
      total ? `${total} across ${hits.length} renders — worst: ${hits[0].render} "${hits[0].hits[0].a}" over "${hits[0].hits[0].b}" at ${hits[0].hits[0].depth_css_px}px depth`
        : `${collisions.length} renders, no pair of text runs overlapping by more than 1.5px of depth`);
  }
}

/* ---- carried W4 machine gates ------------------------------------------ */
for (const [id, name, test, describe] of [
  ['A13', 'no figure text below the legibility floor', () => (check('type') ?? []).flatMap((t) => t.belowFloor), (b) => `${b.length} runs below ${registry.thresholds.legibility_floor_px}px`],
  ['A7', 'every render carries what its page contains', () => (check('frame') ?? []).filter((f) => f.hiddenOverflow.length || f.outsideViewBox.length || f.documentOverflow > 0), (b) => `${b.length} renders clipping or overflowing`],
  ['A12', 'every drawn state is keyed', () => (check('keys') ?? []).flatMap((k) => k.figures.flatMap((f) => f.describedMissing)), (b) => `${b.length} drawn states with no legend entry`],
  ['A5', 'no motion at runtime', () => (check('motion') ?? []).filter((m) => m.running > 0 || m.named.length), (b) => `${b.length} renders with animation`],
  ['ACC', 'zero axe violations (partial evidence, never proof)', () => (check('axe') ?? []).flatMap((a) => a.violations), (b) => `${b.length} violations`],
  ['NET', 'no request off the study origin', () => (check('network') ?? []).flatMap((n) => n.remote), (b) => `${b.length} remote requests`],
  ['LOG', 'no console or page error', () => (check('console') ?? []).flatMap((c) => c.errors), (b) => `${b.length} errors`],
]) {
  const bad = test();
  gate(id, name, bad.length === 0, bad.length ? describe(bad) : 'clean');
}

/* ---- A10 — one shell, byte-identical ----------------------------------- */
{
  const shell = check('shell');
  if (!shell) gate('A10', 'one shell served to every page', null, 'checks/shell.json absent');
  else {
    const digests = new Set(shell.map((s) => s.sha256));
    gate('A10', 'one shell, byte-identical across every render', digests.size === 1,
      digests.size === 1 ? `sha256 ${[...digests][0].slice(0, 16)}… on all ${shell.length} renders` : `${digests.size} different shells served`);
  }
}

/* ---- what this run does NOT decide ------------------------------------- */
const human = [
  'G1 governing idea', 'G2 task question', 'G3 plain-baseline differential', 'G4 form match',
  'G5 primary-form inventory', 'G6 interchangeability', 'G7 no-box', 'G8 idea survival',
  'G9 expressive reach', 'G10 extension cost', 'G11 channel legibility in context', 'G12 the answer is correct',
];

const fail = results.filter((r) => r.verdict === 'FAIL');
const notRun = results.filter((r) => r.verdict === 'not run');
fs.writeFileSync(path.join(CHECKS, 'verify.json'), `${JSON.stringify({
  gates: results,
  human_gates_not_decided_here: human,
  amendments: amendments.amendments.map((a) => a.id),
  summary: { pass: results.length - fail.length - notRun.length, fail: fail.length, not_run: notRun.length },
  what_a_clean_run_means:
    'the board is internally consistent, accessible to the automated checks, reproducible and admissible. It does not mean any composition communicates. On W4 a clean run passed a figure that stated a repository fact incorrectly; A18 exists because of that.',
}, null, 2)}\n`);

for (const r of results) {
  const mark = r.verdict === 'pass' ? '  ok  ' : r.verdict === 'FAIL' ? ' FAIL ' : ' ---- ';
  console.log(`${mark}${r.id.padEnd(6)} ${r.what}\n         ${r.detail}`);
}
console.log(`\n${results.length - fail.length - notRun.length} pass · ${fail.length} fail · ${notRun.length} not run`);
console.log(`human gates this session may not run: ${human.length}`);
process.exit(fail.length ? 1 : 0);
