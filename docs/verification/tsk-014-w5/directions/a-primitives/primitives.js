/* DIRECTION A — curated semantic primitives.
 *
 * The mechanism this direction is demonstrated by. The product ships a closed
 * set of named typed blocks, each modelling one *kind of subject relationship*
 * rather than one chart type. A document supplies typed data and no geometry at
 * all: no coordinate, no scale, no mark, no colour. The renderer owns
 * everything, so every instance of a primitive looks and behaves the same,
 * follows the document's mode, recomposes at the declared widths, keys its own
 * states and writes its own accessible description.
 *
 * Three primitives are implemented here, which is what a first shipped set
 * would plausibly contain:
 *
 *   extent_on_axis   — objects positioned on an ordered axis, each carrying a
 *                      length-proportional extent that ends in a limit, with a
 *                      highlighted run threading a named subset.
 *   travel_and_stop  — objects that travel through named ordered stages and
 *                      stop somewhere, with the untravelled remainder reserved.
 *   coverage_grid    — objects against a fixed set of lanes, each cell in one
 *                      of a closed state vocabulary.
 *
 * The direction's limit is structural and is demonstrated, not described: a
 * subject whose relationship none of these models has nowhere to go.
 */
(() => {
  const { el, figure, CHANNELS, cell, hatchDefs, legend, legendRows } = window.w4svg;

  class Unsupported extends Error {}

  const band = (count, top, step, pad = 0.3) => ({
    at: (i) => top + i * step + step / 2,
    bottom: top + count * step,
    thickness: step * (1 - pad),
  });

  /* ---- extent_on_axis --------------------------------------------------- */

  function extentOnAxis(data, { width, mode }) {
    const rows = data.rows;
    const axis = data.axis;
    const step = mode === 'narrow' ? 40 : 44;
    const gutterEnd = mode === 'narrow' ? 150 : mode === 'intermediate' ? 200 : 250;
    const plotLeft = gutterEnd + 20;
    const plotRight = width - 20;
    const top = 26;
    const rowBand = band(rows.length + 1, top, step);
    const height = rowBand.bottom + 14;
    const legendHeight = legendRows(3, mode === 'narrow' ? 1 : 3) + 14;

    const x = (value) => plotLeft + ((value - axis.from) / (axis.to - axis.from)) * (plotRight - plotLeft);

    const svg = figure({
      width, height: height + legendHeight, id: data.id,
      title: data.title,
      desc: `${data.description} Each ${data.unit} occupies one row; horizontal position marks its place on the ${axis.label} axis; a dashed track shows how far it can move and ends in a cap at its limit; a heavy line threads the ${data.runLabel}.`,
    });
    svg.dataset.emittedBy = 'a-primitives:extent_on_axis';
    svg.append(hatchDefs());
    const g = el('g');

    for (let t = axis.from; t <= axis.to; t += 1) {
      g.append(el('line', { x1: x(t), y1: top + step * 0.55, x2: x(t), y2: rowBand.bottom, stroke: 'var(--fg-line-soft)', 'stroke-width': 1 }));
      if (t < axis.to) {
        g.append(el('text', { x: x(t + 0.5), y: rowBand.at(0), 'text-anchor': 'middle', 'font-size': 12, fill: 'var(--sh-ink-muted)', 'dominant-baseline': 'middle' }, `${axis.tickPrefix} ${t}`));
      }
    }
    /* The end label and the last tick label compete for the same corner as the
     * frame narrows. They get their own row rather than a collision. */
    g.append(el('text', { x: 14, y: 12, 'font-size': 11.5, fill: 'var(--sh-ink-muted)', 'dominant-baseline': 'middle' }, axis.startLabel));
    g.append(el('text', { x: plotRight, y: 12, 'text-anchor': 'end', 'font-size': 11.5, fill: 'var(--sh-ink-muted)', 'dominant-baseline': 'middle' }, axis.endLabel));

    rows.forEach((row, index) => {
      const y = rowBand.at(index + 1);
      g.append(el('text', { x: 14, y: y - 7, 'font-size': 12.5, 'font-weight': 620, 'font-family': 'var(--sh-mono)', fill: 'var(--sh-ink)', 'dominant-baseline': 'middle' }, row.id));
      g.append(el('text', { x: 14, y: y + 8, 'font-size': 11.5, fill: 'var(--sh-ink-muted)', 'dominant-baseline': 'middle' }, row.label));
      if (row.extent > 0) {
        g.append(el('line', { x1: x(row.at + 0.5), y1: y, x2: x(row.at + 0.5 + row.extent), y2: y, ...CHANNELS.room, 'data-state': 'room' }));
      }
      g.append(el('line', { x1: x(row.at + 0.5 + row.extent), y1: y - 11, x2: x(row.at + 0.5 + row.extent), y2: y + 11, ...CHANNELS.cap, 'data-state': 'cap' }));
    });

    const runRows = data.run.map((id) => rows.findIndex((r) => r.id === id)).filter((i) => i >= 0);
    const d = runRows
      .map((index, k) => {
        const px = x(rows[index].at + 0.5);
        const py = rowBand.at(index + 1);
        if (k === 0) return `M${px} ${py}`;
        const prev = rows[runRows[k - 1]];
        return `L${x(prev.at + 0.5)} ${py}L${px} ${py}`;
      })
      .join('');
    g.append(el('path', { d, fill: 'none', ...CHANNELS.run, 'data-state': 'run' }));
    svg.append(g);

    svg.append(legend(
      [
        { state: 'run', means: data.runLabel },
        { state: 'room', means: data.extentLabel },
        { state: 'cap', means: data.capLabel },
      ],
      {
        width, y: height + 12, columns: mode === 'narrow' ? 1 : 3,
        swatch: (state) => {
          const s = el('g');
          if (state === 'cap') s.append(el('line', { x1: 13, y1: -8, x2: 13, y2: 8, ...CHANNELS.cap }));
          else s.append(el('line', { x1: 0, y1: 0, x2: 26, y2: 0, ...CHANNELS[state] }));
          return s;
        },
      },
    ));
    return svg;
  }

  /* ---- travel_and_stop -------------------------------------------------- */

  function travelAndStop(data, { width, mode }) {
    const stages = data.stages;
    const rows = data.rows;
    /* The primitive owns its own recomposition — that is the direction's claim.
     * Below the narrow breakpoint the stage axis rotates: each object becomes a
     * bounded block whose rail runs down inside it, so a stop can never span the
     * figure's width and a cause can never abut the next object's title. */
    if (mode === 'narrow') return travelAndStopNarrow(data, { width });
    const step = mode === 'narrow' ? 62 : 52;
    const gutterEnd = mode === 'narrow' ? width - 24 : mode === 'intermediate' ? 260 : 320;
    const plotLeft = mode === 'narrow' ? 24 : gutterEnd + 30;
    const plotRight = width - 20;
    const top = 34;
    const rowBand = band(rows.length + 1, top, step);
    const height = rowBand.bottom + 16;
    const legendHeight = legendRows(3, mode === 'narrow' ? 1 : 3) + 14;
    const sx = (index) => plotLeft + ((index + 0.5) / stages.length) * (plotRight - plotLeft);

    const svg = figure({
      width, height: height + legendHeight, id: data.id,
      title: data.title,
      desc: `${data.description} Each ${data.unit} runs along its own rail through the stages ${stages.join(', ')}. The rail is heavy up to the stage it reached and stays drawn, open and dashed, across the stages it never reached, with a tick where it stopped and the cause named beside it.`,
    });
    svg.dataset.emittedBy = 'a-primitives:travel_and_stop';
    const g = el('g');

    /* Long stage names run into each other once the plot is narrow. They stagger
     * onto two rows rather than overlapping — the primitive owns this, which is
     * the point: a document that never thought about it still gets it right. */
    const stagger = (plotRight - plotLeft) / stages.length < 150;
    stages.forEach((stage, index) => {
      const y = rowBand.at(0) + (stagger && index % 2 ? 15 : -3);
      g.append(el('text', { x: sx(index), y, 'text-anchor': 'middle', 'font-size': 11.5, fill: 'var(--sh-ink-muted)', 'dominant-baseline': 'middle' }, stage));
    });

    rows.forEach((row, index) => {
      const y = rowBand.at(index + 1);
      const labelY = mode === 'narrow' ? y - 22 : y - 7;
      g.append(el('text', { x: mode === 'narrow' ? 24 : 14, y: labelY, 'font-size': 12.5, 'font-weight': 600, fill: 'var(--sh-ink)', 'dominant-baseline': 'middle' }, row.label));
      const stop = Math.max(0, Math.min(stages.length - 1, row.reached - 1));
      g.append(el('line', { x1: sx(0), y1: y, x2: sx(stop), y2: y, ...CHANNELS.reached, 'data-state': 'reached' }));
      if (stop < stages.length - 1) {
        g.append(el('line', { x1: sx(stop), y1: y, x2: sx(stages.length - 1), y2: y, ...CHANNELS.reserved, 'data-state': 'reserved' }));
        g.append(el('line', { x1: sx(stop), y1: y - 10, x2: sx(stop), y2: y + 10, ...CHANNELS.stopped, 'data-state': 'stopped' }));
        if (row.cause) {
          g.append(el('text', { x: sx(stop) + 6, y: y + 17, 'font-size': 11, fill: 'var(--sh-ink-muted)', 'dominant-baseline': 'middle' }, row.cause));
        }
      }
      for (let k = 0; k <= stop; k += 1) {
        g.append(el('circle', { cx: sx(k), cy: y, r: 3.5, fill: 'var(--fg-line)' }));
      }
    });
    svg.append(g);

    svg.append(legend(
      [
        { state: 'reached', means: 'stages it reached' },
        { state: 'stopped', means: 'where it stops' },
        { state: 'reserved', means: 'stages it did not reach' },
      ],
      {
        width, y: height + 12, columns: mode === 'narrow' ? 1 : 3,
        swatch: (state) => {
          const s = el('g');
          if (state === 'stopped') s.append(el('line', { x1: 13, y1: -8, x2: 13, y2: 8, ...CHANNELS.stopped }));
          else s.append(el('line', { x1: 0, y1: 0, x2: 26, y2: 0, ...CHANNELS[state] }));
          return s;
        },
      },
    ));
    return svg;
  }

  function travelAndStopNarrow(data, { width }) {
    const stages = data.stages;
    const rows = data.rows;
    const stageStep = 21;
    const blockOf = (row) => 32 + stages.length * stageStep + (row.reached - 1 < stages.length - 1 && row.cause ? 18 : 4);
    let cursor = 12;
    const tops = rows.map((row) => { const t = cursor; cursor += blockOf(row); return t; });
    const height = cursor + 6;
    const legendHeight = legendRows(3, 1) + 14;

    const svg = figure({
      width, height: height + legendHeight, id: data.id,
      title: data.title,
      desc: `${data.description} Each ${data.unit} is a bounded block; its rail runs down inside the block through the stages ${stages.join(', ')}, heavy to the stage it reached and dashed below it, with a tick where it stopped and the cause inside the same block.`,
    });
    svg.dataset.emittedBy = 'a-primitives:travel_and_stop';
    const g = el('g');
    const railX = 26;
    const sy = (top, index) => top + 30 + index * stageStep;

    rows.forEach((row, index) => {
      const top = tops[index];
      const stop = Math.max(0, Math.min(stages.length - 1, row.reached - 1));
      g.append(el('line', { x1: 8, y1: top, x2: width - 8, y2: top, stroke: 'var(--fg-line-soft)', 'stroke-width': 1 }));
      g.append(el('text', { x: 8, y: top + 12, 'font-size': 11.5, 'font-weight': 620, fill: 'var(--sh-ink)', 'dominant-baseline': 'middle' }, row.label));
      g.append(el('line', { x1: railX, y1: sy(top, 0), x2: railX, y2: sy(top, stop), ...CHANNELS.reached, 'data-state': 'reached' }));
      if (stop < stages.length - 1) {
        g.append(el('line', { x1: railX, y1: sy(top, stop), x2: railX, y2: sy(top, stages.length - 1), ...CHANNELS.reserved, 'data-state': 'reserved' }));
        g.append(el('line', { x1: railX - 7, y1: sy(top, stop), x2: railX + 7, y2: sy(top, stop), ...CHANNELS.stopped, 'data-state': 'stopped' }));
      }
      stages.forEach((stage, k) => {
        if (k <= stop) g.append(el('circle', { cx: railX, cy: sy(top, k), r: 3, fill: 'var(--fg-line)' }));
        g.append(el('text', { x: railX + 16, y: sy(top, k), 'font-size': 11, fill: 'var(--sh-ink-muted)', 'dominant-baseline': 'middle' }, stage));
      });
      if (stop < stages.length - 1 && row.cause) {
        g.append(el('text', { x: railX + 16, y: sy(top, stages.length - 1) + 15, 'font-size': 11, fill: 'var(--sh-ink)', 'dominant-baseline': 'middle' }, row.cause));
      }
    });
    svg.append(g);
    svg.append(legend(
      [
        { state: 'reached', means: 'stages it reached' },
        { state: 'stopped', means: 'where it stops' },
        { state: 'reserved', means: 'stages it did not reach' },
      ],
      {
        width, y: height + 12, columns: 1,
        swatch: (state) => {
          const s = el('g');
          if (state === 'stopped') s.append(el('line', { x1: 13, y1: -8, x2: 13, y2: 8, ...CHANNELS.stopped }));
          else s.append(el('line', { x1: 0, y1: 0, x2: 26, y2: 0, ...CHANNELS[state] }));
          return s;
        },
      },
    ));
    return svg;
  }

  /* ---- coverage_grid ---------------------------------------------------- */

  const CELL_MEANS = {
    executed: 'an executed run on that lane',
    bounded: 'exists, not promoted here',
    notrun: 'no run at all',
    na: 'the lane does not apply',
    notclaimed: 'the claim does not reach this lane',
  };

  function coverageGrid(data, { width, mode }) {
    const lanes = data.lanes;
    const rows = data.rows;
    const step = mode === 'narrow' ? 34 : 32;
    const top = mode === 'narrow' ? 96 : 104;
    const laneLeft = mode === 'narrow' ? 190 : width - 24 - lanes.length * 26;
    const rowBand = band(rows.length, top, step);
    const height = rowBand.bottom + 14;
    const drawn = [...new Set(rows.flatMap((r) => r.cells))];
    const entries = Object.keys(CELL_MEANS).filter((state) => drawn.includes(state)).map((state) => ({ state, means: CELL_MEANS[state] }));
    const columns = mode === 'narrow' ? 1 : 3;
    const legendHeight = legendRows(entries.length, columns) + 14;

    const svg = figure({
      width, height: height + legendHeight, id: data.id,
      title: data.title,
      desc: `${data.description} Each ${data.unit} occupies one row and each of the lanes ${lanes.join(', ')} one column. A cell is drawn as one of: ${entries.map((e) => `${e.state} (${e.means})`).join('; ')}.`,
    });
    svg.dataset.emittedBy = 'a-primitives:coverage_grid';
    svg.append(hatchDefs());
    const g = el('g');

    lanes.forEach((lane, index) => {
      /* anchor at the end and rotate clockwise, so the label runs up and to the
       * LEFT of its column. Anchoring at the start would run it off the right
       * edge, which is what the containment probe caught. */
      const lx = laneLeft + index * 26 + 16;
      g.append(el('text', { x: lx, y: top - 10, 'font-size': 11, 'text-anchor': 'end', fill: 'var(--sh-ink-muted)', transform: `rotate(48 ${lx} ${top - 10})` }, lane));
    });
    rows.forEach((row, index) => {
      const y = rowBand.at(index);
      g.append(el('text', { x: 14, y, 'font-size': 12.5, 'font-weight': 600, fill: 'var(--sh-ink)', 'dominant-baseline': 'middle' }, row.label));
      row.cells.forEach((state, lane) => {
        if (!CELL_MEANS[state]) throw new Unsupported(`coverage_grid has no cell state "${state}"`);
        g.append(cell(laneLeft + lane * 26 + 6, y - 7, 14, state));
      });
    });
    svg.append(g);
    svg.append(legend(entries, {
      width, y: height + 12, columns,
      swatch: (state) => { const s = el('g'); s.append(cell(0, -7, 14, state)); return s; },
    }));
    return svg;
  }

  const PRIMITIVES = { extent_on_axis: extentOnAxis, travel_and_stop: travelAndStop, coverage_grid: coverageGrid };

  function render(block, options) {
    const primitive = PRIMITIVES[block.primitive];
    if (!primitive) {
      throw new Unsupported(
        `no shipped primitive models this subject. The catalogue is ${Object.keys(PRIMITIVES).join(', ')}; the document asked for "${block.primitive}".`,
      );
    }
    return primitive(block, options);
  }

  function mount(host, block, breakpoints) {
    const paint = () => {
      const w = window.innerWidth;
      const mode = w <= breakpoints.narrow ? 'narrow' : w <= breakpoints.wide ? 'intermediate' : 'wide';
      const width = mode === 'narrow' ? 344 : mode === 'intermediate' ? 840 : 1150;
      try {
        host.replaceChildren(render(block, { width, mode }));
        host.dataset.band = mode;
      } catch (error) {
        host.replaceChildren(
          Object.assign(document.createElement('p'), {
            className: 'sh-note',
            textContent: `The runtime refused this block: ${error.message}`,
          }),
        );
        host.dataset.band = mode;
        host.dataset.refused = 'true';
      }
    };
    paint();
    window.addEventListener('resize', paint);
  }

  window.w4primitives = { render, mount, PRIMITIVES, Unsupported };
})();
