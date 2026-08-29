/* Portal page family 4 — monorepo area drill-down.
 *
 * The subject is a DECLARED FIXTURE, not this repository. CodeFlow is a single
 * project, so a monorepo family cannot be demonstrated against its real content
 * graph without inventing one — and inventing one silently is the failure this
 * whole task exists to correct. The fixture is named as a fixture on the page,
 * in the registry, and in the figure's own description.
 *
 * The composition answers what a folder-per-package navigation dump cannot: an
 * area that OWNS a shared contract, an area that CONSUMES one, and a contract
 * with NO owner are three visibly different shapes. The third is the one a
 * reader most needs and the one a folder tree can never show, because an
 * unowned contract has no folder.
 */
(() => {
  const NS = 'http://www.w3.org/2000/svg';
  const el = (name, attrs = {}, text) => {
    const node = document.createElementNS(NS, name);
    for (const [k, v] of Object.entries(attrs)) if (v !== null && v !== undefined) node.setAttribute(k, String(v));
    if (text !== undefined) node.textContent = String(text);
    return node;
  };

  const LINK = {
    owns: { means: 'this area owns the contract', r: 7, fill: 'var(--fg-accent)', stroke: 'none', dash: null },
    consumes: { means: 'this area consumes it', r: 5, fill: 'none', stroke: 'var(--fg-accent)', dash: null },
    migrating: { means: 'consuming an old version; a migration is open', r: 5, fill: 'none', stroke: 'var(--fg-warn)', dash: '2 2' },
  };

  function render(fixture, { width, mode }) {
    const narrow = mode === 'narrow';
    const areas = fixture.areas;
    const contracts = fixture.contracts;
    const ownerOf = (contract) => areas.find((a) => a.links.some((l) => l.contract === contract.id && l.kind === 'owns'));

    const labelWidth = narrow ? 118 : 210;
    const gridLeft = labelWidth + 16;
    const colStep = Math.max(narrow ? 30 : 56, (width - gridLeft - 22) / contracts.length);
    /* The column heads are rotated 48 degrees, so the head room they need is a
     * function of the longest label, not a constant. A constant clipped the
     * longest one at every width. */
    const longest = Math.max(...contracts.map((c) => (`${c.name}${ownerOf(c) ? '' : ' · no owner'}`).length));
    const top = Math.round(longest * 6.1 * Math.sin((48 * Math.PI) / 180)) + 46;
    const rowStep = narrow ? 40 : 44;
    const keyCols = narrow ? 1 : width < 900 ? 2 : 3;
    const keyRows = Math.ceil(Object.keys(LINK).length / keyCols);
    const height = top + areas.length * rowStep + 34 + keyRows * 20;

    const svg = el('svg', {
      viewBox: `0 0 ${width} ${height}`, width: '100%', role: 'img',
      'aria-labelledby': 'area-t area-d',
      style: `max-width:${width}px;height:auto;display:block`,
    });
    svg.dataset.emittedBy = 'portal:area-drilldown';
    svg.dataset.band = mode;
    svg.append(
      el('title', { id: 'area-t' }, 'Which area owns which shared contract, and which contract has no owner — declared fixture, not this repository'),
      el('desc', { id: 'area-d' },
        `A declared monorepo fixture, not CodeFlow's own content graph. ${areas.length} areas against ${contracts.length} shared contracts. `
        + Object.entries(LINK).map(([id, l]) => `${id}: ${l.means}`).join('; ')
        + `. A contract column with no owning area is drawn with a broken head, and there ${fixture.unownedCount === 1 ? 'is one' : `are ${fixture.unownedCount}`} of those.`),
    );

    contracts.forEach((contract, index) => {
      const x = gridLeft + index * colStep + colStep / 2;
      const owner = ownerOf(contract);
      svg.append(el('line', {
        x1: x, y1: top - 14, x2: x, y2: top + areas.length * rowStep - rowStep / 2,
        stroke: owner ? 'var(--fg-line-soft)' : 'var(--fg-stop)', 'stroke-width': 1,
        'stroke-dasharray': owner ? '1 4' : '4 3',
      }));
      /* Rotated clockwise from an end anchor, a head runs up and to the left, so
       * the LAST column is the one that can leave the frame on the right. The
       * anchor is pulled back inside for every column rather than only that one. */
      const hx = Math.min(x, width - 10);
      svg.append(el('text', {
        x: hx, y: top - 22, 'font-size': 11, 'text-anchor': 'end',
        fill: owner ? 'var(--sh-ink-muted)' : 'var(--sh-ink)',
        transform: `rotate(48 ${hx} ${top - 22})`,
      }, `${contract.name}${owner ? '' : ' · no owner'}`));
    });

    areas.forEach((area, index) => {
      const y = top + index * rowStep;
      svg.append(el('text', { x: 12, y: y - 4, 'font-size': 12.5, 'font-weight': 620, fill: 'var(--sh-ink)' }, area.name));
      svg.append(el('text', { x: 12, y: y + 10, 'font-size': 11, 'font-family': 'var(--sh-mono)', fill: 'var(--sh-ink-muted)' }, area.root));
      contracts.forEach((contract, k) => {
        const link = area.links.find((l) => l.contract === contract.id);
        if (!link) return;
        const spec = LINK[link.kind];
        const x = gridLeft + k * colStep + colStep / 2;
        const g = el('g', { 'data-state': link.kind });
        g.append(el('circle', { cx: x, cy: y, r: spec.r, fill: spec.fill, stroke: spec.stroke, 'stroke-width': 1.8, 'stroke-dasharray': spec.dash }));
        svg.append(g);
      });
    });

    /* the global band: what sits above every area, drawn as the span it is */
    svg.append(el('line', { x1: 12, y1: 32, x2: width - 12, y2: 32, stroke: 'var(--fg-accent)', 'stroke-width': 4, 'stroke-linecap': 'round' }));
    const bandCaption = narrow
      ? ['repository-wide: purpose, shared', 'journeys, governance, vocabulary']
      : ['repository-wide: purpose, shared journeys, governance, vocabulary — above every area, authored once'];
    bandCaption.forEach((line, k) => {
      svg.append(el('text', { x: 12, y: 12 + k * 13, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, line));
    });
    svg.append(el('text', { x: 12, y: 44, 'font-size': 11, fill: 'var(--sh-ink-muted)' },
      narrow ? 'areas below, entered by what they own' : 'areas below, each entered by what it owns rather than by where its folder sits'));

    const keyY = top + areas.length * rowStep + 18;
    Object.entries(LINK).forEach(([id, spec], index) => {
      const kx = 10 + (index % keyCols) * ((width - 20) / keyCols);
      const ky = keyY + Math.floor(index / keyCols) * 20;
      const g = el('g', { 'data-legend-state': id });
      g.append(el('circle', { cx: kx + 8, cy: ky - 3, r: spec.r, fill: spec.fill, stroke: spec.stroke, 'stroke-width': 1.8, 'stroke-dasharray': spec.dash }));
      g.append(el('text', { x: kx + 24, y: ky + 1, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, spec.means));
      svg.append(g);
    });
    return svg;
  }

  function mount(host, fixture, breakpoints) {
    const paint = () => {
      const w = window.innerWidth;
      const mode = w <= breakpoints.narrow ? 'narrow' : w <= breakpoints.wide ? 'intermediate' : 'wide';
      const width = mode === 'narrow' ? 344 : mode === 'intermediate' ? 840 : 1150;
      host.replaceChildren(render(fixture, { width, mode }));
      host.dataset.band = mode;
    };
    paint();
    window.addEventListener('resize', paint);
  }

  window.w4area = { render, mount, LINK };
})();
