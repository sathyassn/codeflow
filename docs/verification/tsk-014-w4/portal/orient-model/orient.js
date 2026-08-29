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
 * The honest source-model gap: these facts are derivable *here*, by parsing one
 * repository's own operating contract in prose. They are not carried as declared
 * facts in the portal's source graph, so this family generalises to another
 * repository only once a layer model is declared in portal configuration. That
 * is a smaller gap than "the fact does not exist" and a real one.
 */
(() => {
  const NS = 'http://www.w3.org/2000/svg';
  const el = (name, attrs = {}, text) => {
    const node = document.createElementNS(NS, name);
    for (const [k, v] of Object.entries(attrs)) if (v !== null && v !== undefined) node.setAttribute(k, String(v));
    if (text !== undefined) node.textContent = String(text);
    return node;
  };

  /* Three bands, from the contract's own authority vocabulary. Each carries a
   * channel that is not colour: a solid edge, a dashed edge, a hatched fill. */
  const BAND = {
    yours: { means: 'you write it', fill: 'var(--fg-fill)', stroke: 'var(--fg-line)', width: 2, dash: null },
    conditional: { means: 'editable, but only in the ship flow', fill: 'var(--fg-accent-soft)', stroke: 'var(--fg-accent)', width: 1.5, dash: '7 3' },
    refuses: { means: 'a tool owns these bytes; an edit is overwritten', fill: 'url(#orient-hatch)', stroke: 'var(--fg-hatch)', width: 1, dash: null },
  };

  function render(m, { width, mode }) {
    const narrow = mode === 'narrow';
    const layers = m.byCadence;
    const maxCadence = Math.max(...layers.map((l) => l.cadence));
    const minCadence = Math.min(...layers.map((l) => l.cadence));

    const top = 40;
    /* The row has to hold the bar, the layer's question, and every home it
     * declares. A fixed step let a three-home layer run under the next bar. */
    const maxHomes = Math.max(...layers.map((l) => m.homesOf(l).length));
    const rowStep = 56 + maxHomes * 16;
    const laneLeft = 12;
    const laneWidth = narrow ? width - 24 : Math.min(520, width * 0.44);
    const spineX = laneLeft + laneWidth + 96;
    const keyRows = narrow ? 3 : 1;
    const height = top + layers.length * rowStep + 16 + keyRows * 20;

    const svg = el('svg', {
      viewBox: `0 0 ${width} ${height}`, width: '100%', role: 'img',
      'aria-labelledby': 'orient-t orient-d',
      style: `max-width:${width}px;height:auto;display:block`,
    });
    svg.dataset.emittedBy = 'portal:orient-model';
    svg.dataset.band = mode;
    svg.append(
      el('title', { id: 'orient-t' }, 'Where this repository keeps each kind of knowledge, and how each kind may change'),
      el('desc', { id: 'orient-d' },
        'Six layers of repository knowledge in the order the operating contract lists them, from the layer that changes most rarely to the one that changes with nobody touching it. '
        + 'A layer bar is drawn wider the more often the layer changes, and its edge and fill carry write authority: '
        + Object.entries(BAND).map(([id, b]) => `${id} — ${b.means}`).join('; ') + '. '
        + 'Each layer names the paths it actually occupies, and a path that is not a file in the checkout says so. '
        + (narrow ? 'The traceability spine is listed beneath the layers.' : 'To the right, the traceability spine descends through the layers, attached at the layer each of its links belongs to.')),
    );

    const defs = el('defs');
    const pattern = el('pattern', { id: 'orient-hatch', width: 6, height: 6, patternUnits: 'userSpaceOnUse', patternTransform: 'rotate(45)' });
    pattern.append(el('line', { x1: 0, y1: 0, x2: 0, y2: 6, stroke: 'var(--fg-hatch)', 'stroke-width': 1.6 }));
    defs.append(pattern);
    svg.append(defs);

    svg.append(el('text', { x: laneLeft, y: 14, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'changes rarely'));
    svg.append(el('text', { x: laneLeft + laneWidth, y: 14, 'text-anchor': 'end', 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'changes continuously'));
    svg.append(el('line', { x1: laneLeft, y1: 22, x2: laneLeft + laneWidth, y2: 22, stroke: 'var(--fg-line-soft)', 'stroke-width': 1 }));

    const spineNodesByLayer = new Map();
    for (const node of m.spine.nodes) {
      if (!spineNodesByLayer.has(node.layer)) spineNodesByLayer.set(node.layer, []);
      spineNodesByLayer.get(node.layer).push(node);
    }

    layers.forEach((layer, index) => {
      const y = top + index * rowStep;
      const share = (layer.cadence - minCadence) / (maxCadence - minCadence);
      /* The cadence label sits beside the bar, so the bar's own maximum has to
       * leave room for it. At the narrow band that reserve is most of the lane. */
      const reserve = narrow ? 116 : 100;
      const barWidth = 92 + share * (laneWidth - 92 - reserve);
      const bands = m.bandsOf ? m.bandsOf(layer) : [...new Set(m.authoritiesOf(layer).map((a) => m.bandOf(a)).filter(Boolean))];
      const primary = bands.includes('refuses') && bands.length === 1 ? 'refuses' : bands.includes('yours') ? 'yours' : bands[0] ?? 'refuses';
      const spec = BAND[primary];

      const g = el('g', { 'data-state': primary });
      /* The bar carries the layer name only. Its own question and its homes sit
       * beneath it, so a short bar cannot push its text under the cadence label. */
      g.append(el('rect', {
        x: laneLeft, y, width: barWidth, height: 26, rx: 3,
        fill: spec.fill, stroke: spec.stroke, 'stroke-width': spec.width, 'stroke-dasharray': spec.dash,
      }));
      g.append(el('text', { x: laneLeft + 12, y: y + 17, 'font-size': 13, 'font-weight': 650, fill: 'var(--sh-ink)' }, layer.name));
      g.append(el('text', { x: laneLeft + barWidth + 10, y: y + 12, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, layer.changes));
      if (bands.length > 1) {
        g.append(el('text', { x: laneLeft + barWidth + 10, y: y + 25, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'split authority'));
      }
      g.append(el('text', { x: laneLeft + 2, y: y + 40, 'font-size': 11.5, fill: 'var(--sh-ink-muted)' }, layer.asks));
      svg.append(g);

      /* the homes, as their real paths, with the ones that are not files marked */
      m.homesOf(layer).forEach((home, k) => {
        const hy = y + 56 + k * 16;
        const inTree = home.kind === 'file' || home.kind === 'dir';
        svg.append(el('text', {
          x: laneLeft + 14, y: hy, 'font-size': 11, 'font-family': 'var(--sh-mono)',
          fill: inTree ? 'var(--sh-ink)' : 'var(--sh-ink-muted)',
        }, `${inTree ? '' : '· '}${home.path}${inTree ? '' : ' — nothing to open'}`));
      });

      if (!narrow) {
        const nodes = spineNodesByLayer.get(layer.id) ?? [];
        svg.append(el('circle', { cx: spineX, cy: y + 13, r: nodes.length ? 5 : 2.5, fill: nodes.length ? 'var(--fg-accent)' : 'var(--fg-line-soft)' }));
        svg.append(el('line', { x1: laneLeft + barWidth, y1: y + 20, x2: spineX, y2: y + 13, stroke: 'var(--fg-line-soft)', 'stroke-width': 1, 'stroke-dasharray': nodes.length ? null : '2 3' }));
        nodes.forEach((node, k) => {
          svg.append(el('text', { x: spineX + 14, y: y + 17 + k * 15, 'font-size': 11.5, 'font-weight': 600, fill: 'var(--sh-ink)' }, node.label));
          svg.append(el('text', { x: spineX + 14 + 78, y: y + 17 + k * 15, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, node.holds));
        });
      }
    });

    if (!narrow) {
      svg.append(el('line', {
        x1: spineX, y1: top + 13, x2: spineX, y2: top + (layers.length - 1) * rowStep + 13,
        stroke: 'var(--fg-accent)', 'stroke-width': 3, 'stroke-linecap': 'round',
      }));
      svg.append(el('text', { x: spineX - 6, y: 14, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'the traceability spine — walk it upward to answer "why is this here"'));
    }

    const keyY = top + layers.length * rowStep + 12;
    Object.entries(BAND).forEach(([id, spec], index) => {
      const kx = narrow ? 8 : laneLeft + index * 260;
      const ky = keyY + (narrow ? index * 20 : 0);
      const g = el('g', { 'data-legend-state': id });
      g.append(el('rect', { x: kx, y: ky - 8, width: 24, height: 13, rx: 2, fill: spec.fill, stroke: spec.stroke, 'stroke-width': spec.width, 'stroke-dasharray': spec.dash }));
      g.append(el('text', { x: kx + 32, y: ky + 2, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, spec.means));
      svg.append(g);
    });
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
