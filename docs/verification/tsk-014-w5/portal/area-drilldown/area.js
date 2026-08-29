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
 *
 * ---------------------------------------------------------------------------
 * What W4 got wrong here, and what changed
 *
 * D05. At 344px the six column heads were rotated 48 degrees into one another
 * and became a single illegible block. F07. When those heads failed, the grid
 * failed with them: the columns carried the identity, so six anonymous dot
 * positions were all that survived. And the "no owner" state — the finding this
 * family exists to surface — was a colour change on a hairline column rule,
 * which the blind observer could not perceive at any width.
 *
 * Two changes. The narrow band stops being a matrix: a matrix spends an axis a
 * phone does not have on a distinction that can simply be named, so each area
 * names its own contracts, owned first. And "no owner" gets a mark of its own —
 * a broken bracket, drawn at the foot of its column at the wide band and beside
 * its name at the narrow one — so the state is carried by a shape rather than
 * by the absence of one.
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
    owns: { means: 'this area owns the contract', r: 7, fill: 'var(--fg-accent)', stroke: 'none', dash: null, overlay: null },
    consumes: { means: 'this area consumes it', r: 5, fill: 'none', stroke: 'var(--fg-accent)', dash: null, overlay: null },
    /* `migrating` and `consumes` were an open ring and a dashed open ring: same
     * shape, same empty interior, separated by the dash alone. One channel, and
     * the pair collapses if it fails. The migration now carries a mark of its
     * own — a chevron pointing out of the ring, which is what a migration is. */
    migrating: { means: 'consuming an old version; a migration is open', r: 5, fill: 'none', stroke: 'var(--fg-warn)', dash: '2 2', overlay: 'chevron' },
  };

  /* the chevron that says "on its way off this version" */
  function chevron(parent, cx, cy, r) {
    parent.append(el('path', {
      /* 10px tall: the mark floor applies to this chevron as much as to the ring
       * it sits beside, and the first cut drew it at 8 */
      d: `M${cx + r + 1} ${cy - 5} l5 5 l-5 5`,
      fill: 'none', stroke: 'var(--fg-warn)', 'stroke-width': 1.8, 'stroke-linecap': 'round', 'stroke-linejoin': 'round',
    }));
  }

  /* one link mark, with every channel it carries inside the element that names
   * its state — form, interior, edge and overlay all measurable together */
  function linkMark(parent, kind, cx, cy) {
    const spec = LINK[kind];
    const g = el('g', { 'data-state': kind, 'data-distinction': 'contract-ownership' });
    g.append(el('circle', { cx, cy, r: spec.r, fill: spec.fill, stroke: spec.stroke, 'stroke-width': 1.8, 'stroke-dasharray': spec.dash }));
    if (spec.overlay === 'chevron') chevron(g, cx, cy, spec.r);
    parent.append(g);
    return g;
  }

  /* The mark for "no area is the authority for this": a bracket with a visible
   * break in it. It is a shape, at the mark floor, and it is keyed. W4 carried
   * this state on a hairline's colour alone. */
  function brokenMark(parent, cx, cy) {
    const g = el('g', { 'data-state': 'unowned', 'data-distinction': 'contract-ownership' });
    const arm = 6;
    g.append(el('path', {
      d: `M${cx - arm} ${cy - arm} H${cx + arm} M${cx - arm} ${cy - arm} V${cy - 2} M${cx + arm} ${cy - arm} V${cy - 2}`,
      fill: 'none', stroke: 'var(--fg-stop)', 'stroke-width': 1.8,
    }));
    g.append(el('path', {
      d: `M${cx - arm} ${cy + arm} H${cx + arm} M${cx - arm} ${cy + arm} V${cy + 2} M${cx + arm} ${cy + arm} V${cy + 2}`,
      fill: 'none', stroke: 'var(--fg-stop)', 'stroke-width': 1.8,
    }));
    /* struck through: nobody is the authority for it. Without this the mark
     * differed from an open ring on its form alone. */
    g.append(el('line', {
      x1: cx - arm, y1: cy, x2: cx + arm, y2: cy,
      stroke: 'var(--fg-stop)', 'stroke-width': 1.8,
    }));
    parent.append(g);
    return g;
  }

  function legendInto(svg, width, keyY, keyCols) {
    const entries = [
      ...Object.entries(LINK).map(([id, spec]) => ({ id, spec, means: spec.means })),
      { id: 'unowned', spec: null, means: 'no area owns it — nobody is the authority for it' },
    ];
    entries.forEach((entry, index) => {
      const kx = 10 + (index % keyCols) * ((width - 20) / keyCols);
      const ky = keyY + Math.floor(index / keyCols) * 20;
      const g = el('g', { 'data-legend-state': entry.id });
      if (entry.spec) {
        g.append(el('circle', { cx: kx + 8, cy: ky - 3, r: entry.spec.r, fill: entry.spec.fill, stroke: entry.spec.stroke, 'stroke-width': 1.8, 'stroke-dasharray': entry.spec.dash }));
        if (entry.spec.overlay === 'chevron') chevron(g, kx + 8, ky - 3, entry.spec.r);
      } else {
        /* the key draws the mark; it must not also register as a drawn instance
         * of the state, or the channel probe would measure the legend */
        brokenMark(g, kx + 8, ky - 3).removeAttribute('data-state');
      }
      g.append(el('text', { x: kx + 24, y: ky + 1, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, entry.means));
      svg.append(g);
    });
    return keyY + Math.ceil(entries.length / keyCols) * 20;
  }

  /* --------------------------------------------------------------- narrow --- */
  function renderNarrow(fixture, width) {
    const { areas, contracts } = fixture;
    const nameOf = new Map(contracts.map((c) => [c.id, c.name]));
    const ownerOf = (contract) => areas.find((a) => a.links.some((l) => l.contract === contract.id && l.kind === 'owns'));
    const unowned = contracts.filter((c) => !ownerOf(c));

    const blocks = areas.map((area) => ({
      area,
      owns: area.links.filter((l) => l.kind === 'owns'),
      uses: area.links.filter((l) => l.kind !== 'owns'),
    }));
    const blockH = (b) => 62 + (b.owns.length + b.uses.length) * 17 + 15;
    const tops = [];
    let cursor = 84;
    for (const b of blocks) { tops.push(cursor); cursor += blockH(b); }
    const footTop = cursor + 6;
    const keyY = footTop + 40 + unowned.length * 18 + 18;
    const height = keyY + (Object.keys(LINK).length + 1) * 20 + 8;

    const svg = el('svg', {
      viewBox: `0 0 ${width} ${height}`, width: '100%', role: 'img',
      'aria-labelledby': 'area-t area-d',
      style: `max-width:${width}px;height:auto;display:block`,
    });
    svg.dataset.emittedBy = 'portal:area-drilldown';
    svg.dataset.band = 'narrow';
    svg.append(
      el('title', { id: 'area-t' }, 'Which area owns which shared contract, and which contract has no owner — declared fixture, not this repository'),
      el('desc', { id: 'area-d' },
        `A declared monorepo fixture, not CodeFlow's own content graph. ${areas.length} areas against ${contracts.length} shared contracts. `
        + 'At this width each area names the contracts it touches rather than holding a position in a grid: what it owns first, then what it uses. '
        + Object.entries(LINK).map(([id, l]) => `${id}: ${l.means}`).join('; ')
        + `. The contracts no area owns are listed at the foot behind a broken bracket, and there ${unowned.length === 1 ? 'is one' : `are ${unowned.length}`} of those.`),
    );

    svg.append(el('line', { x1: 12, y1: 44, x2: width - 12, y2: 44, stroke: 'var(--fg-accent)', 'stroke-width': 4, 'stroke-linecap': 'round' }));
    ['repository-wide: purpose, shared', 'journeys, governance, vocabulary'].forEach((line, k) => {
      svg.append(el('text', { x: 12, y: 16 + k * 14, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, line));
    });
    svg.append(el('text', { x: 12, y: 62, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'areas below, entered by what they own'));

    blocks.forEach((b, index) => {
      const y = tops[index];
      svg.append(el('text', { x: 12, y, 'font-size': 12.5, 'font-weight': 620, fill: 'var(--sh-ink)' }, b.area.name));
      svg.append(el('text', { x: 12, y: y + 14, 'font-size': 11, 'font-family': 'var(--sh-mono)', fill: 'var(--sh-ink-muted)' }, b.area.root));
      let ly = y + 32;
      const group = (label, links) => {
        svg.append(el('text', { x: 16, y: ly, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, label));
        ly += 15;
        for (const link of links) {
          linkMark(svg, link.kind, 28, ly - 4);
          svg.append(el('text', { x: 48, y: ly, 'font-size': 11.5, fill: 'var(--sh-ink)' },
            `${nameOf.get(link.contract)}${link.kind === 'migrating' ? ' — migration open' : ''}`));
          ly += 17;
        }
      };
      group(b.owns.length ? 'owns' : 'owns nothing', b.owns);
      group('uses', b.uses);
    });

    svg.append(el('line', { x1: 12, y1: footTop, x2: width - 12, y2: footTop, stroke: 'var(--sh-rule)', 'stroke-width': 1 }));
    svg.append(el('text', { x: 12, y: footTop + 20, 'font-size': 12, 'font-weight': 620, fill: 'var(--sh-ink)' }, 'no area owns these'));
    unowned.forEach((contract, index) => {
      const y = footTop + 42 + index * 18;
      brokenMark(svg, 22, y - 4);
      svg.append(el('text', { x: 42, y, 'font-size': 11.5, fill: 'var(--sh-ink)' }, contract.name));
    });

    legendInto(svg, width, keyY, 1);
    return svg;
  }

  /* ------------------------------------------------------- wide and middle --- */
  function renderGrid(fixture, width, narrowHeads) {
    const { areas, contracts } = fixture;
    const ownerOf = (contract) => areas.find((a) => a.links.some((l) => l.contract === contract.id && l.kind === 'owns'));

    const labelWidth = 210;
    const gridLeft = labelWidth + 16;
    const colStep = Math.max(56, (width - gridLeft - 22) / contracts.length);
    /* The column heads are rotated, so the head room they need is a function of
     * the longest label, not a constant. A constant clipped the longest one at
     * every width. The " · no owner" suffix is gone from the head: that state is
     * now a mark at the column's foot, so the head carries only the name. */
    const longest = Math.max(...contracts.map((c) => c.name.length));
    /* the head room is the rotation's rise PLUS the two caption lines that sit
     * above it; W4 counted only the rise, so the longest head printed through
     * the caption once the '· no owner' suffix stopped padding the estimate */
    const top = Math.round(longest * 6.1 * Math.sin((48 * Math.PI) / 180)) + 62;
    const rowStep = 44;
    const gridBottom = top + areas.length * rowStep - rowStep / 2;
    const ownerRowY = gridBottom + 22;
    const keyCols = width < 900 ? 2 : 3;
    const keyY = ownerRowY + 40;
    const height = keyY + Math.ceil((Object.keys(LINK).length + 1) / keyCols) * 20 + 12;

    const svg = el('svg', {
      viewBox: `0 0 ${width} ${height}`, width: '100%', role: 'img',
      'aria-labelledby': 'area-t area-d',
      style: `max-width:${width}px;height:auto;display:block`,
    });
    svg.dataset.emittedBy = 'portal:area-drilldown';
    svg.append(
      el('title', { id: 'area-t' }, 'Which area owns which shared contract, and which contract has no owner — declared fixture, not this repository'),
      el('desc', { id: 'area-d' },
        `A declared monorepo fixture, not CodeFlow's own content graph. ${areas.length} areas against ${contracts.length} shared contracts. `
        + Object.entries(LINK).map(([id, l]) => `${id}: ${l.means}`).join('; ')
        + `. Beneath the grid an owner row names the area that owns each column, and a column no area owns carries a broken bracket there instead of a name; there ${fixture.unownedCount === 1 ? 'is one' : `are ${fixture.unownedCount}`} of those.`),
    );

    contracts.forEach((contract, index) => {
      const x = gridLeft + index * colStep + colStep / 2;
      const owner = ownerOf(contract);
      svg.append(el('line', {
        x1: x, y1: top - 14, x2: x, y2: gridBottom,
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
      }, contract.name));
    });

    areas.forEach((area, index) => {
      const y = top + index * rowStep;
      svg.append(el('text', { x: 12, y: y - 4, 'font-size': 12.5, 'font-weight': 620, fill: 'var(--sh-ink)' }, area.name));
      svg.append(el('text', { x: 12, y: y + 10, 'font-size': 11, 'font-family': 'var(--sh-mono)', fill: 'var(--sh-ink-muted)' }, area.root));
      contracts.forEach((contract, k) => {
        const link = area.links.find((l) => l.contract === contract.id);
        if (!link) return;
        const x = gridLeft + k * colStep + colStep / 2;
        linkMark(svg, link.kind, x, y);
      });
    });

    /* the owner row: the second channel for "unowned", and a useful fact anyway */
    svg.append(el('text', { x: 12, y: ownerRowY + 4, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, 'owned by'));
    contracts.forEach((contract, index) => {
      const x = gridLeft + index * colStep + colStep / 2;
      const owner = ownerOf(contract);
      if (owner) {
        svg.append(el('text', { x, y: ownerRowY + 4, 'font-size': 11, 'text-anchor': 'middle', fill: 'var(--sh-ink)' },
          owner.name.length > 11 ? `${owner.name.slice(0, 10)}…` : owner.name));
      } else {
        brokenMark(svg, x, ownerRowY);
      }
    });

    /* the global band: what sits above every area, drawn as the span it is */
    svg.append(el('line', { x1: 12, y1: 32, x2: width - 12, y2: 32, stroke: 'var(--fg-accent)', 'stroke-width': 4, 'stroke-linecap': 'round' }));
    svg.append(el('text', { x: 12, y: 12, 'font-size': 11, fill: 'var(--sh-ink-muted)' },
      'repository-wide: purpose, shared journeys, governance, vocabulary — above every area, authored once'));
    svg.append(el('text', { x: 12, y: 44, 'font-size': 11, fill: 'var(--sh-ink-muted)' },
      'areas below, each entered by what it owns rather than by where its folder sits'));

    legendInto(svg, width, keyY, keyCols);
    return svg;
  }

  function render(fixture, { width, mode }) {
    const svg = mode === 'narrow' ? renderNarrow(fixture, width) : renderGrid(fixture, width);
    svg.dataset.band = mode;
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
