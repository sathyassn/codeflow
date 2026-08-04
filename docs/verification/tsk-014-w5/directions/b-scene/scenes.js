/* Scene documents for the direction-B demonstration.
 *
 * This file is the *document* side of the contract: it turns real repository
 * facts into scenes. It contains no geometry primitive, no SVG call and no
 * coordinate that scene.js did not derive from a declared scale. Everything it
 * emits is data, and the only thing that can draw it is the compiler.
 *
 * This is what an authoring agent would write. Nothing here is product code.
 */
(() => {
  const round = (n) => Math.round(n * 100) / 100;

  /* ---------------------------------------------------------------- P1 --- */

  function planScene(model) {
    const open = model.open
      .slice()
      .sort((a, b) => model.wave(a.id) - model.wave(b.id) || a.id.localeCompare(b.id));
    const ids = open.map((t) => t.id);
    const short = (t) => {
      const words = t.title.split(' ');
      let out = '';
      for (const word of words) {
        if (out && `${out} ${word}`.length > 30) break;
        out = out ? `${out} ${word}` : word;
      }
      return out.length < t.title.length ? `${out}…` : out;
    };
    const maxWave = model.maxWave;
    const roomOf = (id) => model.room[id] ?? 0;
    const maxRoom = Math.max(...ids.map(roomOf));

    /* ---- wide: waves on x, one row per task ---- */
    const rowDomain = ['head', ...ids];
    const step = 44;
    const plotTop = 26;
    const height = plotTop + rowDomain.length * step + 14;
    const wide = {
      id: 'scene-p1',
      title: 'The unfinished EPC-005 tasks on a wave axis',
      desc:
        'Each unfinished task occupies one row. Horizontal position marks the earliest wave the task could occupy. A heavy unbroken line threads the tasks it passes through. Where a task could start later, a dashed track extends rightward from its own row and ends in a cap on the wave that limits it; where it could not, the cap sits against its own wave.',
      frame: { width: 1160, height },
      states: ['run', 'room', 'cap'],
      legend: [
        { state: 'run', means: 'one unbroken run' },
        { state: 'room', means: 'how much later it could start' },
        { state: 'cap', means: 'the wave that limits it' },
      ],
      scales: {
        gutter: { type: 'linear', axis: 'x', domain: [0, 1], range: [14, 250] },
        wave: { type: 'linear', axis: 'x', domain: [1, maxWave + 1], range: [270, 1140] },
        row: { type: 'band', axis: 'y', domain: rowDomain, range: [plotTop, plotTop + rowDomain.length * step], pad: 0.3 },
      },
      marks: [],
    };

    for (let w = 1; w <= maxWave + 1; w += 1) {
      wide.marks.push({ kind: 'rule', scale: 'wave', at: w, extent: [plotTop + step * 0.55, plotTop + rowDomain.length * step] });
    }
    for (let w = 1; w <= maxWave; w += 1) {
      wide.marks.push({ kind: 'label', at: { wave: w + 0.5, row: 'head' }, text: `wave ${w}`, anchor: 'middle', size: 12, muted: true });
    }
    wide.marks.push({ kind: 'label', at: { gutter: 0, row: 'head' }, text: 'open now', anchor: 'start', size: 11.5, muted: true });
    wide.marks.push({ kind: 'label', at: { wave: maxWave + 1, row: 'head' }, text: 'epic finishes', anchor: 'end', size: 11.5, muted: true });

    for (const task of open) {
      wide.marks.push({ kind: 'label', at: { gutter: 0, row: task.id, dy: -7 }, text: task.id, anchor: 'start', size: 12.5, mono: true, weight: 620 });
      wide.marks.push({ kind: 'label', at: { gutter: 0, row: task.id, dy: 8 }, text: short(task), anchor: 'start', size: 11.5, muted: true });
      const w = model.wave(task.id);
      const r = roomOf(task.id);
      if (r > 0) {
        wide.marks.push({ kind: 'segment', from: { wave: w + 0.5, row: task.id }, to: { wave: w + 0.5 + r, row: task.id }, state: 'room' });
      }
      wide.marks.push({ kind: 'tick', at: { wave: w + 0.5 + r, row: task.id }, along: 'y', size: 22, state: 'cap' });
    }
    wide.marks.push({
      kind: 'path',
      curve: 'ortho',
      through: model.chain.map((id) => ({ wave: model.wave(id) + 0.5, row: id })),
      state: 'run',
    });
    /* ---- intermediate: two registers against the same wave axis ---- */
    const runIds = ids.filter((id) => roomOf(id) === 0);
    const roomIds = ids.filter((id) => roomOf(id) > 0);
    const midDomain = ['head', 'runhead', ...runIds, 'divider', 'roomhead', ...roomIds];
    const midStep = 46;
    const midHeight = plotTop + midDomain.length * midStep + 14;
    const intermediate = {
      frame: { width: 860, height: midHeight },
      desc:
        'The field splits into two registers against the same horizontal wave axis, separated by a drawn divider. The unbroken run sits above the divider; everything that can start later sits below it with its own track. Position along the wave axis and the extent of a later start both survive.',
      scales: {
        gutter: { type: 'linear', axis: 'x', domain: [0, 1], range: [12, 210] },
        wave: { type: 'linear', axis: 'x', domain: [1, maxWave + 1], range: [230, 845] },
        row: { type: 'band', axis: 'y', domain: midDomain, range: [plotTop, plotTop + midDomain.length * midStep], pad: 0.3 },
      },
      marks: [],
    };
    for (let w = 1; w <= maxWave + 1; w += 1) {
      intermediate.marks.push({ kind: 'rule', scale: 'wave', at: w, extent: [plotTop + midStep * 0.55, plotTop + midDomain.length * midStep] });
    }
    for (let w = 1; w <= maxWave; w += 1) {
      intermediate.marks.push({ kind: 'label', at: { wave: w + 0.5, row: 'head' }, text: `wave ${w}`, anchor: 'middle', size: 12, muted: true });
    }
    intermediate.marks.push({ kind: 'label', at: { gutter: 0, row: 'runhead' }, text: 'the run', anchor: 'start', size: 11.5, muted: true });
    intermediate.marks.push({ kind: 'label', at: { gutter: 0, row: 'roomhead' }, text: 'and what can start later', anchor: 'start', size: 11.5, muted: true });
    intermediate.marks.push({ kind: 'rule', scale: 'row', at: 'divider', extent: [12, 845] });
    for (const id of [...runIds, ...roomIds]) {
      const task = model.byId.get(id);
      const w = model.wave(id);
      const r = roomOf(id);
      intermediate.marks.push({ kind: 'label', at: { gutter: 0, row: id, dy: -7 }, text: id, anchor: 'start', size: 12.5, mono: true, weight: 620 });
      intermediate.marks.push({ kind: 'label', at: { gutter: 0, row: id, dy: 8 }, text: short(task), anchor: 'start', size: 11.5, muted: true });
      if (r > 0) {
        intermediate.marks.push({ kind: 'segment', from: { wave: w + 0.5, row: id }, to: { wave: w + 0.5 + r, row: id }, state: 'room' });
      }
      intermediate.marks.push({ kind: 'tick', at: { wave: w + 0.5 + r, row: id }, along: 'y', size: 22, state: 'cap' });
    }
    intermediate.marks.push({
      kind: 'path',
      curve: 'ortho',
      through: model.chain.map((id) => ({ wave: model.wave(id) + 0.5, row: id })),
      state: 'run',
    });
    /* ---- narrow: waves become row groups, room becomes a per-row length ----
     * Declared in registry.json before it was drawn. Extent is measured inside
     * each task's own row against one shared room scale drawn at the head, so
     * no two tasks can ever occupy one column. */
    const narrowDomain = ['head'];
    for (let w = 1; w <= maxWave; w += 1) {
      narrowDomain.push(`w${w}`);
      for (const id of ids) if (model.wave(id) === w) narrowDomain.push(id);
    }
    const nStep = 40;
    const nHeight = 30 + narrowDomain.length * nStep + 10;
    const narrow = {
      frame: { width: 352, height: nHeight },
      desc:
        'Waves become groups down the page and the unbroken run descends through them. How much later a task could start is measured horizontally inside that task own row, against one shared scale drawn once at the head, and ends in a cap. A task with no room carries its cap at zero on that scale.',
      legendColumns: 1,
      scales: {
        gutter: { type: 'linear', axis: 'x', domain: [0, 1], range: [10, 186] },
        room: { type: 'linear', axis: 'x', domain: [0, maxRoom], range: [212, 330] },
        row: { type: 'band', axis: 'y', domain: narrowDomain, range: [30, 30 + narrowDomain.length * nStep], pad: 0.28 },
      },
      marks: [],
    };
    narrow.marks.push({ kind: 'label', at: { gutter: 0, row: 'head' }, text: 'earliest wave', anchor: 'start', size: 11, muted: true });
    for (let k = 0; k <= maxRoom; k += 1) {
      narrow.marks.push({ kind: 'rule', scale: 'room', at: k, extent: [22, 30 + narrowDomain.length * nStep] });
      narrow.marks.push({ kind: 'label', at: { room: k, row: 'head' }, text: k === 0 ? 'later start  0' : `${k}`, anchor: k === 0 ? 'end' : 'middle', size: 11, muted: true });
    }
    for (let w = 1; w <= maxWave; w += 1) {
      narrow.marks.push({ kind: 'label', at: { gutter: 0, row: `w${w}` }, text: `wave ${w}`, anchor: 'start', size: 11.5, weight: 620, muted: true });
    }
    for (const id of ids) {
      const task = model.byId.get(id);
      const r = roomOf(id);
      narrow.marks.push({ kind: 'label', at: { gutter: 0.06, row: id, dy: -6 }, text: id, anchor: 'start', size: 12, mono: true, weight: 620 });
      narrow.marks.push({ kind: 'label', at: { gutter: 0.06, row: id, dy: 7 }, text: short(task), anchor: 'start', size: 11, muted: true });
      if (r > 0) narrow.marks.push({ kind: 'segment', from: { room: 0, row: id }, to: { room: r, row: id }, state: 'room' });
      narrow.marks.push({ kind: 'tick', at: { room: r, row: id }, along: 'y', size: 18, state: 'cap' });
    }
    narrow.marks.push({
      kind: 'path',
      curve: 'ortho',
      through: model.chain.map((id) => ({ gutter: 0.02, row: id })),
      state: 'run',
    });
    return { ...wide, bands: { intermediate, narrow } };
  }

  /* ---------------------------------------------------------------- P2 --- */

  function evidenceScene(facts) {
    const claims = facts.claims;
    const stages = ['produced', 'artifact', 'acceptance', 'exact candidate'];
    const lanes = facts.lanes;
    const rowDomain = ['head', ...claims.map((c) => c.id)];
    const step = 52;
    /* The lane heads are rotated, so the head row needs the height that rotation
     * costs. Reserving it here is why the label is inside the frame at every
     * width instead of being cut by it. */
    const top = 104;
    const height = top + rowDomain.length * step + 10;

    /* `reaches` is 0..4 on the derivation scale; the stop is the last stage it
     * actually reached, clamped into the stage index space. Both come from the
     * shared fact registry, which verify.mjs binds to the qualification record. */
    const stopOf = (claim) => Math.max(0, Math.min(stages.length - 1, facts.reachOf(claim) - 1));
    const causeOf = (claim) => {
      const id = facts.stops[claim.id];
      const cause = facts.causes.find((c) => c.id === id);
      return cause ? cause.label ?? cause.id : null;
    };

    const wide = {
      id: 'scene-p2',
      title: 'How far each claim travelled, and what it covers',
      desc:
        'Each claim runs along its own rail through four derivation stages. The rail is heavy up to the point it stops and stays drawn, open and dashed, across the stages it never reached. Beside each rail head, one cell per platform lane carries a distinct shape for an executed run, a bounded run, no run, an inapplicable lane and a lane the claim does not reach.',
      frame: { width: 1160, height },
      states: ['reached', 'stopped', 'reserved', 'executed', 'bounded', 'notrun', 'na', 'notclaimed'],
      legendColumns: 4,
      legend: [
        { state: 'reached', means: 'stages the evidence reached' },
        { state: 'stopped', means: 'where it stops' },
        { state: 'reserved', means: 'stages it did not reach' },
        { state: 'executed', means: 'an executed run on that lane' },
        { state: 'bounded', means: 'exists, not promoted here' },
        { state: 'notrun', means: 'no run at all' },
        { state: 'na', means: 'the lane does not apply' },
        { state: 'notclaimed', means: 'the claim does not reach this lane' },
      ],
      scales: {
        gutter: { type: 'linear', axis: 'x', domain: [0, 1], range: [14, 290] },
        lane: { type: 'band', axis: 'x', domain: lanes, range: [306, 466] },
        stage: { type: 'band', axis: 'x', domain: stages, range: [520, 1140] },
        row: { type: 'band', axis: 'y', domain: rowDomain, range: [top, top + rowDomain.length * step], pad: 0.3 },
      },
      marks: [],
    };

    stages.forEach((stage) => {
      wide.marks.push({ kind: 'label', at: { stage, row: 'head' }, text: stage, anchor: 'middle', size: 11.5, muted: true });
    });
    lanes.forEach((lane) => {
      wide.marks.push({ kind: 'label', at: { lane, row: 'head', dy: 6 }, text: lane, anchor: 'start', size: 11, muted: true, rotate: -48 });
    });

    claims.forEach((claim) => {
      const stop = stopOf(claim);
      const cause = causeOf(claim);
      wide.marks.push({ kind: 'label', at: { gutter: 0, row: claim.id, dy: -6 }, text: claim.claim, anchor: 'start', size: 12.5, weight: 600 });
      claim.cells.forEach((cellState, index) => {
        wide.marks.push({ kind: 'cell', at: { lane: lanes[index], row: claim.id }, state: cellState.state, size: 14 });
      });
      wide.marks.push({ kind: 'segment', from: { stage: stages[0], row: claim.id }, to: { stage: stages[stop], row: claim.id }, state: 'reached' });
      if (stop < stages.length - 1) {
        wide.marks.push({ kind: 'segment', from: { stage: stages[stop], row: claim.id }, to: { stage: stages[stages.length - 1], row: claim.id }, state: 'reserved' });
        wide.marks.push({ kind: 'tick', at: { stage: stages[stop], row: claim.id }, along: 'y', size: 20, state: 'stopped' });
        if (cause) {
          wide.marks.push({ kind: 'label', at: { stage: stages[stop], row: claim.id, dy: 17 }, text: cause, anchor: 'start', size: 11, muted: true });
        }
      }
      stages.forEach((stage, index) => {
        if (index <= stop) wide.marks.push({ kind: 'marker', at: { stage, row: claim.id }, shape: 'dot', size: 5, state: 'reached' });
      });
    });

    /* narrow: one bounded lane per claim; the rail runs down inside it */
    const nDomain = [];
    claims.forEach((c) => nDomain.push(`${c.id}-head`, `${c.id}-cov`, ...stages.map((s) => `${c.id}-${s}`), `${c.id}-foot`));
    const nStep = 21;
    const nHeight = 16 + nDomain.length * nStep + 10;
    const narrow = {
      frame: { width: 352, height: nHeight },
      legendColumns: 1,
      desc:
        'Each claim becomes a bounded lane with its own edge. The rail runs down inside that lane, inset from both edges, heavy to the point it stops and dashed below it to the lane foot. The stop is a short cross-tick on the rail and its cause sits inside the same lane, indented to the rail. The coverage cells form the lane head row.',
      scales: {
        edge: { type: 'linear', axis: 'x', domain: [0, 1], range: [8, 344] },
        rail: { type: 'linear', axis: 'x', domain: [0, 1], range: [26, 40] },
        text: { type: 'linear', axis: 'x', domain: [0, 1], range: [48, 236] },
        lane: { type: 'band', axis: 'x', domain: lanes, range: [248, 344] },
        row: { type: 'band', axis: 'y', domain: nDomain, range: [16, 16 + nDomain.length * nStep], pad: 0.05 },
      },
      marks: [],
    };
    claims.forEach((claim) => {
      const stop = stopOf(claim);
      const cause = causeOf(claim);
      narrow.marks.push({ kind: 'rule', scale: 'row', at: `${claim.id}-head`, extent: [8, 344] });
      narrow.marks.push({ kind: 'label', at: { text: 0, row: `${claim.id}-head`, dy: -3 }, text: claim.claim, anchor: 'start', size: 11.5, weight: 620 });
      claim.cells.forEach((cellState, index) => {
        narrow.marks.push({ kind: 'cell', at: { lane: lanes[index], row: `${claim.id}-cov` }, state: cellState.state, size: 13 });
      });
      narrow.marks.push({ kind: 'segment', from: { rail: 0.5, row: `${claim.id}-${stages[0]}` }, to: { rail: 0.5, row: `${claim.id}-${stages[stop]}` }, state: 'reached' });
      if (stop < stages.length - 1) {
        narrow.marks.push({ kind: 'segment', from: { rail: 0.5, row: `${claim.id}-${stages[stop]}` }, to: { rail: 0.5, row: `${claim.id}-${stages[stages.length - 1]}` }, state: 'reserved' });
        narrow.marks.push({ kind: 'tick', at: { rail: 0.5, row: `${claim.id}-${stages[stop]}` }, along: 'x', size: 13, state: 'stopped' });
      }
      stages.forEach((stage, index) => {
        narrow.marks.push({ kind: 'label', at: { text: 0, row: `${claim.id}-${stage}` }, text: stage, anchor: 'start', size: 11, muted: true });
        if (index <= stop) narrow.marks.push({ kind: 'marker', at: { rail: 0.5, row: `${claim.id}-${stage}` }, shape: 'dot', size: 5, state: 'reached' });
      });
      if (stop < stages.length - 1 && cause) {
        narrow.marks.push({ kind: 'label', at: { text: 0, row: `${claim.id}-foot` }, text: cause, anchor: 'start', size: 11, muted: true });
      }
    });

    return { ...wide, bands: { intermediate: { frame: { width: 860, height }, scales: { gutter: { type: 'linear', axis: 'x', domain: [0, 1], range: [12, 240] }, lane: { type: 'band', axis: 'x', domain: lanes, range: [252, 350] }, stage: { type: 'band', axis: 'x', domain: stages, range: [392, 845] }, row: { type: 'band', axis: 'y', domain: rowDomain, range: [top, top + rowDomain.length * step], pad: 0.3 } } }, narrow } };
  }

  /* ------------------------------------------------- the negative case --- */

  function forceLayoutAttempt(model) {
    /* The grammar has no layout operator. To draw a dependency graph it must be
     * given coordinates, and the only place those can come from is this file —
     * which means the document computed the layout, not the model. The scene
     * below does exactly that, with hand-chosen positions on an anonymous
     * 0..1 grid, so the failure is visible instead of described. */
    const ids = model.open.map((t) => t.id);
    const placed = {
      'TSK-014': [0.12, 0.5], 'TSK-013': [0.12, 0.86], 'TSK-011': [0.4, 0.28],
      'TSK-009': [0.4, 0.66], 'TSK-007': [0.68, 0.3], 'TSK-010': [0.9, 0.55],
    };
    const scene = {
      id: 'scene-negative',
      title: 'A dependency graph the grammar cannot lay out',
      desc:
        'The same six unfinished tasks drawn as a graph. Every position here was chosen by hand in the document, because the grammar has no operator that computes a layout from a set of edges. Nothing on this figure is derived from the subject.',
      frame: { width: 700, height: 300 },
      states: ['blocked'],
      legend: [{ state: 'blocked', means: 'a remaining dependency' }],
      scales: {
        gx: { type: 'linear', axis: 'x', domain: [0, 1], range: [40, 660] },
        gy: { type: 'linear', axis: 'y', domain: [0, 1], range: [30, 250] },
      },
      marks: [],
    };
    for (const id of ids) {
      for (const dep of model.byId.get(id).deps) {
        if (!placed[dep] || !placed[id]) continue;
        scene.marks.push({
          kind: 'segment',
          from: { gx: placed[dep][0], gy: placed[dep][1] },
          to: { gx: placed[id][0], gy: placed[id][1] },
          state: 'blocked',
        });
      }
    }
    for (const id of ids) {
      if (!placed[id]) continue;
      scene.marks.push({ kind: 'label', at: { gx: placed[id][0], gy: placed[id][1] }, text: id, anchor: 'middle', size: 12, mono: true, weight: 620 });
    }
    return scene;
  }

  window.w4scenes = { planScene, evidenceScene, forceLayoutAttempt, round };
})();
