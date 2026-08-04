/* Portal page family 1 — Orient, as a model rather than a list.
 *
 * The subject is the repository's own six layers, from shared/repo-layers.js,
 * which re-parses them out of AGENTS.md: the cadence order is the contract
 * table's own row order and every authority rule carries the contract sentence
 * that states it. Nothing here is invented.
 *
 * The composition is emitted by the generator, which is where a portal
 * composition has to live: raw HTML in a repository Markdown source is escaped
 * with no configuration bypass.
 *
 * ---------------------------------------------------------------------------
 * What W4 got wrong here, and what changed
 *
 * W4 bucketed the spine's nodes by the layer each belongs to and then drew the
 * buckets in the layers' cadence order. Two separate orderings were forced onto
 * one y-axis, and the drawing lost the argument: `capability` and `pr` both
 * belong to WHAT, so they collapsed into a single row, and the spine came out
 * as capability+PR → ADR/spec → epic/task → ledger. The contract declares
 * capability → epic/task → ADR/spec → PR → ledger. The source was right; the
 * drawing was wrong, and every deterministic gate passed it.
 *
 * So the spine now owns its own axis. It descends in its declared order, five
 * distinct positions, and each node reaches back to the layer it belongs to by
 * its own connector. The connectors cross, and the crossing is the point: it is
 * the visible evidence that trace order and volatility order are two different
 * orderings. A figure that cannot show them disagreeing cannot show either.
 *
 * W4 also drew three authority bands and never named the seven authority kinds
 * underneath them, so a reader saw a vocabulary of three where the contract
 * declares seven. All seven are now drawn and named, banded by the three, which
 * makes the banding a visible grouping rather than a silent substitution.
 *
 * And W4's narrow band dropped the spine entirely while its own description
 * claimed the spine was "listed beneath the layers". Nothing listed it. At
 * narrow the spine now leads: it becomes the primary descent and the layers
 * attach to it — the same relationship read from the other side.
 */
(() => {
  const NS = 'http://www.w3.org/2000/svg';
  const el = (name, attrs = {}, text) => {
    const node = document.createElementNS(NS, name);
    for (const [k, v] of Object.entries(attrs)) if (v !== null && v !== undefined) node.setAttribute(k, String(v));
    if (text !== undefined) node.textContent = String(text);
    return node;
  };

  /* Three bands, from the contract's own authority vocabulary.
   *
   * W5's first cut gave these an interior of {solid, solid, hatch} and an edge
   * of {solid, dashed, solid}. Read honestly that is not two channels: `yours`
   * and `conditional` had the SAME interior kind and differed only in the dash
   * and in their colour, and `yours` and `refuses` had the same edge and
   * differed only in the interior. Each of those pairs rested on one non-hue
   * channel, with hue quietly making up the difference — which is exactly the
   * claim A15 was written to refuse, made by the figure that also carried the
   * factual spine error.
   *
   * Three interiors and three edges now, all distinct, none of them hue:
   *   yours        solid interior     solid edge
   *   conditional  vertical rules     dashed edge
   *   refuses      diagonal hatch     dotted edge
   * Either channel alone separates all three, so losing one still leaves the
   * vocabulary readable. */
  const BAND = {
    yours: { means: 'you write it', fill: 'var(--fg-fill)', stroke: 'var(--fg-line)', width: 2, dash: null },
    conditional: { means: 'editable, but only in the ship flow', fill: 'url(#orient-rule)', stroke: 'var(--fg-accent)', width: 1.5, dash: '7 3' },
    refuses: { means: 'a tool owns these bytes; an edit is overwritten', fill: 'url(#orient-hatch)', stroke: 'var(--fg-hatch)', width: 1, dash: '2 3' },
  };

  const CHIP_H = 14; /* the mark floor is 9px; a named chip needs more than that */

  function defs() {
    const d = el('defs');
    const hatch = el('pattern', { id: 'orient-hatch', width: 6, height: 6, patternUnits: 'userSpaceOnUse', patternTransform: 'rotate(45)' });
    hatch.append(el('line', { x1: 0, y1: 0, x2: 0, y2: 6, stroke: 'var(--fg-hatch)', 'stroke-width': 1.6 }));
    /* upright rules, not rotated: a different structure from the 45-degree
     * hatch, so the two interiors separate on orientation and not on tone */
    const rule = el('pattern', { id: 'orient-rule', width: 5, height: 5, patternUnits: 'userSpaceOnUse' });
    rule.append(el('line', { x1: 0, y1: 0, x2: 0, y2: 5, stroke: 'var(--fg-accent)', 'stroke-width': 1.4 }));
    d.append(hatch, rule);
    return d;
  }

  /* One authority kind, drawn with its band's channel and its own name. Seven of
   * these exist in the contract and all seven reach the page. */
  function chip(m, authorityId, x, y) {
    const authority = m.authorities.find((a) => a.id === authorityId);
    const spec = BAND[authority.band];
    const label = authority.label;
    const w = 13 + label.length * 6.1;
    const g = el('g', { 'data-authority': authorityId, 'data-state': authority.band, 'data-distinction': 'write-authority' });
    g.append(el('rect', {
      x, y: y - CHIP_H + 3, width: w, height: CHIP_H, rx: 2,
      fill: spec.fill, stroke: spec.stroke, 'stroke-width': spec.width, 'stroke-dasharray': spec.dash,
    }));
    g.append(el('text', { x: x + 6, y: y - 2, 'font-size': 11, fill: 'var(--sh-ink)' }, label));
    return { node: g, width: w };
  }

  function chipRow(m, layer, x, y, max) {
    const g = el('g');
    let cursor = x;
    for (const id of m.authoritiesOf(layer)) {
      const { node, width } = chip(m, id, cursor, y);
      if (cursor + width > x + max) break;
      g.append(node);
      cursor += width + 6;
    }
    return g;
  }

  function legend(m, svg, x, y, narrow) {
    Object.entries(BAND).forEach(([id, spec], index) => {
      const kx = narrow ? x : x + index * 268;
      const ky = y + (narrow ? index * 19 : 0);
      const g = el('g', { 'data-legend-state': id });
      g.append(el('rect', { x: kx, y: ky - 9, width: 24, height: 13, rx: 2, fill: spec.fill, stroke: spec.stroke, 'stroke-width': spec.width, 'stroke-dasharray': spec.dash }));
      g.append(el('text', { x: kx + 31, y: ky + 1, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, spec.means));
      svg.append(g);
    });
    /* the tail has to fit the band it is drawn in, not the band it was written
     * for — at narrow one line of this runs off the right edge */
    const tail = y + (narrow ? 3 * 19 : 19);
    const lines = narrow
      ? [`each chip names one of the contract's ${m.authorities.length} authority kinds;`, 'its edge shows which band it sits in']
      : [`each chip names one of the contract's ${m.authorities.length} authority kinds; its edge shows which band it sits in`];
    lines.forEach((line, i) => {
      svg.append(el('text', { x, y: tail + i * 14, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, line));
    });
    return tail + (lines.length - 1) * 14;
  }

  /* ---------------------------------------------------------------- wide ---
   * Layers own the y-axis by cadence. The spine owns its own y-axis, in its own
   * declared order, and reaches back across a routing lane. */
  function renderWide(m, width) {
    const layers = m.byCadence;
    const maxCadence = Math.max(...layers.map((l) => l.cadence));
    const minCadence = Math.min(...layers.map((l) => l.cadence));

    const top = 52;
    const maxHomes = Math.max(...layers.map((l) => m.homesOf(l).length));
    const rowStep = 78 + maxHomes * 15;
    const laneLeft = 12;
    const laneWidth = Math.min(430, width * 0.38);
    /* The cadence text sits in a reserved column at a fixed x, never at the end
     * of a bar whose length is data. W4 hung it off barWidth and a short bar
     * pushed it onto an edge. */
    const shoulderX = laneLeft + laneWidth + 8;
    const shoulderW = 104;
    const routeLeft = shoulderX + shoulderW + 10;
    const spineX = Math.max(routeLeft + 96, width * 0.66);
    const barMax = laneWidth - 8;

    /* A row is as tall as its own content: bar, question, one line per home,
     * then the authority chips. The legend goes below the tallest last row,
     * measured — not at a guessed offset, which is how W4's key landed on top
     * of the layer that declares the most homes. */
    const rowContentH = (layer) => 58 + m.homesOf(layer).length * 15 + 12 + CHIP_H;
    const bodyH = (layers.length - 1) * rowStep;
    const lastBottom = top + bodyH + rowContentH(layers[layers.length - 1]);
    const legendY = lastBottom + 26;
    const height = legendY + 40;

    const svg = el('svg', {
      viewBox: `0 0 ${width} ${height}`, width: '100%', role: 'img',
      'aria-labelledby': 'orient-t orient-d',
      style: `max-width:${width}px;height:auto;display:block`,
    });
    svg.dataset.emittedBy = 'portal:orient-model';
    svg.append(
      el('title', { id: 'orient-t' }, 'Where this repository keeps each kind of knowledge, how each kind may change, and how a reader traces why something exists'),
      el('desc', { id: 'orient-d' },
        `Two orderings, drawn on two axes. On the left, the ${layers.length} layers of repository knowledge in the operating contract's own row order, from the layer that changes most rarely to the one that changes with nobody touching it; bar length carries that cadence. `
        + `Each layer names the paths it really occupies, and a path that is not a file in the checkout says so. Each layer also names its write-authority kinds as chips: the contract declares ${m.authorities.length} kinds, grouped into three bands — `
        + Object.entries(BAND).map(([id, b]) => `${id}, ${b.means}`).join('; ') + '. '
        + `On the right, the traceability spine descends in its own declared order: ${m.spine.nodes.map((n) => n.label).join(', then ')}. Connectors run from each spine node back to the layer that holds it. The connectors cross, because trace order and volatility order are not the same ordering.`),
      defs(),
    );

    /* the cadence axis, named at both ends */
    svg.append(el('text', { x: laneLeft, y: 16, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'changes rarely'));
    svg.append(el('text', { x: laneLeft + laneWidth, y: 16, 'text-anchor': 'end', 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'changes continuously'));
    svg.append(el('line', { x1: laneLeft, y1: 24, x2: laneLeft + laneWidth, y2: 24, stroke: 'var(--fg-line-soft)', 'stroke-width': 1 }));
    /* the header is sized against the room actually left of the right edge —
     * at the intermediate band the long form overruns it */
    const spineHead = 'the traceability spine — walk it upward to answer "why is this here"';
    const room = width - spineX + 10;
    svg.append(el('text', { x: spineX - 10, y: 16, 'font-size': 11, fill: 'var(--sh-ink-muted)' },
      room > spineHead.length * 5.6 ? spineHead : 'the traceability spine'));
    if (room <= spineHead.length * 5.6) {
      svg.append(el('text', { x: spineX - 10, y: 30, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'walk it upward to answer "why is this here"'));
    }

    const rowY = new Map();
    const barEnd = new Map();
    layers.forEach((layer, index) => {
      const y = top + index * rowStep;
      rowY.set(layer.id, y + 13);
      const share = (layer.cadence - minCadence) / (maxCadence - minCadence);
      const barWidth = 96 + share * (barMax - 96);
      barEnd.set(layer.id, laneLeft + barWidth);
      const bands = m.bandsOf(layer);
      const primary = bands.includes('yours') ? 'yours' : bands.includes('conditional') ? 'conditional' : 'refuses';
      const spec = BAND[primary];

      const g = el('g', { 'data-state': primary, 'data-layer': layer.id, 'data-distinction': 'write-authority' });
      g.append(el('rect', {
        x: laneLeft, y, width: barWidth, height: 26, rx: 3,
        fill: spec.fill, stroke: spec.stroke, 'stroke-width': spec.width, 'stroke-dasharray': spec.dash,
      }));
      g.append(el('text', { x: laneLeft + 12, y: y + 17, 'font-size': 13, 'font-weight': 650, fill: 'var(--sh-ink)' }, layer.name));
      g.append(el('text', { x: laneLeft + 2, y: y + 41, 'font-size': 11.5, fill: 'var(--sh-ink-muted)' }, layer.asks));
      svg.append(g);

      /* the reserved shoulder — fixed column, never hung off the bar's end */
      svg.append(el('text', { x: shoulderX, y: y + 12, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, layer.changes));
      if (bands.length > 1) {
        svg.append(el('text', { x: shoulderX, y: y + 25, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'split authority'));
      }

      m.homesOf(layer).forEach((home, k) => {
        const hy = y + 58 + k * 15;
        const inTree = home.kind === 'file' || home.kind === 'dir';
        svg.append(el('text', {
          x: laneLeft + 14, y: hy, 'font-size': 11, 'font-family': 'var(--sh-mono)',
          fill: inTree ? 'var(--sh-ink)' : 'var(--sh-ink-muted)',
        }, `${inTree ? '' : '· '}${home.path}${inTree ? '' : ' — nothing to open'}`));
      });

      svg.append(chipRow(m, layer, laneLeft + 2, y + 58 + m.homesOf(layer).length * 15 + 12, laneWidth + shoulderW));
    });

    /* the spine, on its own axis, in its own order */
    const spineTop = top + 10;
    const spineSpan = bodyH + 8;
    const step = spineSpan / (m.spine.nodes.length - 1);
    svg.append(el('line', {
      x1: spineX, y1: spineTop, x2: spineX, y2: spineTop + spineSpan,
      stroke: 'var(--fg-accent)', 'stroke-width': 3, 'stroke-linecap': 'round',
    }));

    m.spine.nodes.forEach((node, index) => {
      const y = spineTop + index * step;
      /* the connector back to the layer that holds this node. It is drawn under
       * the spine and softer than it, so the spine stays the primary descent. */
      const ly = rowY.get(node.layer);
      /* the lead-in stub: the connector has to visibly leave a bar, or the
       * reader is left inferring which layer a curve belongs to from its y */
      svg.append(el('line', {
        x1: barEnd.get(node.layer), y1: ly, x2: routeLeft, y2: ly,
        stroke: 'var(--fg-line)', 'stroke-width': 1.4, opacity: 0.55,
      }));
      svg.append(el('path', {
        d: `M ${routeLeft} ${ly} C ${(routeLeft + spineX) / 2} ${ly}, ${(routeLeft + spineX) / 2} ${y}, ${spineX - 6} ${y}`,
        fill: 'none', stroke: 'var(--fg-line)', 'stroke-width': 1.4, opacity: 0.55,
      }));
      const g = el('g', { 'data-spine-node': node.id, 'data-spine-index': index, 'data-spine-layer': node.layer });
      g.append(el('circle', { cx: spineX, cy: y, r: 6, fill: 'var(--fg-accent)' }));
      g.append(el('text', { x: spineX + 14, y: y - 1, 'font-size': 12, 'font-weight': 650, fill: 'var(--sh-ink)' }, `${index + 1}. ${node.label}`));
      g.append(el('text', { x: spineX + 14, y: y + 13, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, node.holds));
      svg.append(g);
    });

    legend(m, svg, laneLeft, legendY, false);
    return svg;
  }

  /* -------------------------------------------------------------- narrow ---
   * The spine leads. Each spine node is a row, and the layer that holds it
   * attaches to that row. The two layers that are not on the spine are drawn
   * below it as exactly that — a real fact, not an omission. */
  function renderNarrow(m, width) {
    const layers = m.byCadence;
    const maxCadence = Math.max(...layers.map((l) => l.cadence));
    const minCadence = Math.min(...layers.map((l) => l.cadence));
    const barOf = (layer) => 74 + ((layer.cadence - minCadence) / (maxCadence - minCadence)) * (width - 60 - 74);

    const layerById = new Map(layers.map((l) => [l.id, l]));
    const onSpine = new Set(m.spine.nodes.map((n) => n.layer));
    const offSpine = layers.filter((l) => !onSpine.has(l.id));

    const spineX = 16;
    const textX = spineX + 22;
    const seen = new Set();

    /* measured first, so the descent line and the rows cannot disagree */
    const rows = m.spine.nodes.map((node, index) => {
      const layer = layerById.get(node.layer);
      const repeat = seen.has(layer.id);
      seen.add(layer.id);
      return { node, index, layer, repeat, h: repeat ? 62 : 62 + m.homesOf(layer).length * 15 + 22 };
    });

    const top = 46;
    let y = top;
    const placed = rows.map((row) => {
      const at = y;
      y += row.h;
      return { ...row, y: at };
    });
    const offTop = y + 12;
    const offHeights = offSpine.map((l) => 46 + m.homesOf(l).length * 15 + 22);
    const height = offTop + 26 + offHeights.reduce((a, b) => a + b, 0) + 108;

    const svg = el('svg', {
      viewBox: `0 0 ${width} ${height}`, width: '100%', role: 'img',
      'aria-labelledby': 'orient-t orient-d',
      style: `max-width:${width}px;height:auto;display:block`,
    });
    svg.dataset.emittedBy = 'portal:orient-model';
    svg.append(
      el('title', { id: 'orient-t' }, 'The traceability spine, and the layer each of its links lives in'),
      el('desc', { id: 'orient-d' },
        `At this width the spine leads and the layers attach to it — the same relationship read from the other side. The spine descends in its declared order: ${m.spine.nodes.map((n) => n.label).join(', then ')}. `
        + `Beside each link is the layer that holds it, with the paths it occupies and its write-authority kinds; bar length still carries how often that layer changes, so the volatility axis survives even though the rows are no longer ordered by it. `
        + `${offSpine.map((l) => l.name).join(' and ')} are drawn below, off the spine, because the contract does not place them on it.`),
      defs(),
    );

    svg.append(el('text', { x: 12, y: 16, 'font-size': 11.5, 'font-weight': 650, fill: 'var(--sh-ink)' }, 'the traceability spine'));
    svg.append(el('text', { x: 12, y: 31, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'ordered by trace, not by cadence — bar length is still cadence'));

    const last = placed[placed.length - 1];
    svg.append(el('line', {
      x1: spineX, y1: top + 6, x2: spineX, y2: last.y + 6,
      stroke: 'var(--fg-accent)', 'stroke-width': 3, 'stroke-linecap': 'round',
    }));

    for (const row of placed) {
      const { node, index, layer, repeat, y: ry } = row;
      const g = el('g', { 'data-spine-node': node.id, 'data-spine-index': index, 'data-spine-layer': node.layer });
      g.append(el('circle', { cx: spineX, cy: ry + 6, r: 6, fill: 'var(--fg-accent)' }));
      g.append(el('text', { x: textX, y: ry + 10, 'font-size': 12, 'font-weight': 650, fill: 'var(--sh-ink)' }, `${index + 1}. ${node.label}`));
      g.append(el('text', { x: textX, y: ry + 24, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, node.holds));
      svg.append(g);

      const bands = m.bandsOf(layer);
      const primary = bands.includes('yours') ? 'yours' : bands.includes('conditional') ? 'conditional' : 'refuses';
      const spec = BAND[primary];
      const by = ry + 32;
      const bg = el('g', { 'data-state': primary, 'data-layer': layer.id, 'data-distinction': 'write-authority' });
      bg.append(el('rect', {
        x: textX, y: by, width: barOf(layer), height: 22, rx: 3,
        fill: spec.fill, stroke: spec.stroke, 'stroke-width': spec.width, 'stroke-dasharray': spec.dash,
      }));
      bg.append(el('text', { x: textX + 9, y: by + 15, 'font-size': 12, 'font-weight': 650, fill: 'var(--sh-ink)' }, layer.name));
      svg.append(bg);

      /* a layer that holds two spine links is named twice and detailed once —
       * the tie says which row already carried it, instead of repeating a block */
      if (repeat) {
        svg.append(el('text', { x: textX + barOf(layer) + 8, y: by + 15, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, '— same layer, above'));
        continue;
      }
      m.homesOf(layer).forEach((home, k) => {
        const inTree = home.kind === 'file' || home.kind === 'dir';
        svg.append(el('text', {
          x: textX + 2, y: by + 38 + k * 15, 'font-size': 11, 'font-family': 'var(--sh-mono)',
          fill: inTree ? 'var(--sh-ink)' : 'var(--sh-ink-muted)',
        }, `${inTree ? '' : '· '}${home.path}${inTree ? '' : ' — nothing to open'}`));
      });
      svg.append(chipRow(m, layer, textX, by + 38 + m.homesOf(layer).length * 15 + 12, width - textX - 8));
    }

    svg.append(el('text', { x: 12, y: offTop + 4, 'font-size': 11.5, 'font-weight': 650, fill: 'var(--sh-ink)' }, 'not on the spine'));
    svg.append(el('text', { x: 12, y: offTop + 18, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'the contract places no traceability link in these two'));
    let oy = offTop + 32;
    for (const layer of offSpine) {
      const bands = m.bandsOf(layer);
      const primary = bands.includes('yours') ? 'yours' : bands.includes('conditional') ? 'conditional' : 'refuses';
      const spec = BAND[primary];
      const g = el('g', { 'data-state': primary, 'data-layer': layer.id, 'data-distinction': 'write-authority' });
      g.append(el('rect', {
        x: textX, y: oy, width: barOf(layer), height: 22, rx: 3,
        fill: spec.fill, stroke: spec.stroke, 'stroke-width': spec.width, 'stroke-dasharray': spec.dash,
      }));
      g.append(el('text', { x: textX + 9, y: oy + 15, 'font-size': 12, 'font-weight': 650, fill: 'var(--sh-ink)' }, layer.name));
      svg.append(g);
      svg.append(el('text', { x: textX + 2, y: oy + 34, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, layer.asks));
      m.homesOf(layer).forEach((home, k) => {
        const inTree = home.kind === 'file' || home.kind === 'dir';
        svg.append(el('text', {
          x: textX + 2, y: oy + 49 + k * 15, 'font-size': 11, 'font-family': 'var(--sh-mono)',
          fill: inTree ? 'var(--sh-ink)' : 'var(--sh-ink-muted)',
        }, `${inTree ? '' : '· '}${home.path}${inTree ? '' : ' — nothing to open'}`));
      });
      svg.append(chipRow(m, layer, textX, oy + 49 + m.homesOf(layer).length * 15 + 10, width - textX - 8));
      oy += 46 + m.homesOf(layer).length * 15 + 22;
    }

    legend(m, svg, 12, oy + 14, true);
    return svg;
  }

  function render(m, { width, mode }) {
    const svg = mode === 'narrow' ? renderNarrow(m, width) : renderWide(m, width);
    svg.dataset.band = mode;
    return svg;
  }

  function mount(host, model, breakpoints) {
    const paint = () => {
      const w = window.innerWidth;
      const mode = w <= breakpoints.narrow ? 'narrow' : w <= breakpoints.wide ? 'intermediate' : 'wide';
      const width = mode === 'narrow' ? 344 : mode === 'intermediate' ? 840 : 1150;
      host.replaceChildren(render(model, { width, mode }));
      host.dataset.band = mode;
    };
    paint();
    window.addEventListener('resize', paint);
  }

  window.w4orient = { render, mount, BAND };
})();
