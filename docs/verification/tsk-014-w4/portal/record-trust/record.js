/* Portal page family 3 — Record, as a trust surface.
 *
 * The subject is SPC-004 as a portal record page, from shared/record-authority.js,
 * whose every frontmatter value, heading, numbered behaviour and quotation is
 * checked against the real record by the W3 verifier.
 *
 * The composition answers the two questions a reader needs to trust or distrust
 * a record, and it answers them with geometry rather than links:
 *
 *   reach    — each governing decision is a span covering exactly the claims it
 *              reaches, so a decision that supersedes two mechanisms and leaves
 *              the rest standing is visibly shorter than the record.
 *   evidence — every claim keeps an evidence position at its own eye level, and
 *              a claim whose evidence does not exist keeps that position,
 *              reserved and visibly unfilled, so a deliberate absence is not an
 *              empty cell.
 *
 * The shipped portal renders this record as Markdown with a provenance line and
 * a "Record context" list. Neither question is answerable from it.
 */
(() => {
  const NS = 'http://www.w3.org/2000/svg';
  const el = (name, attrs = {}, text) => {
    const node = document.createElementNS(NS, name);
    for (const [k, v] of Object.entries(attrs)) if (v !== null && v !== undefined) node.setAttribute(k, String(v));
    if (text !== undefined) node.textContent = String(text);
    return node;
  };

  const EVIDENCE = {
    executed: { means: 'evidence ran and is claimed here', fill: 'var(--fg-accent)', stroke: 'none', dash: null },
    bounded: { means: 'evidence exists and is deliberately not promoted here', fill: 'none', stroke: 'var(--fg-accent)', dash: '3 2' },
    absent: { means: 'no evidence exists — the position is reserved, not empty', fill: 'none', stroke: 'var(--fg-stop)', dash: '2 2' },
  };
  /* Every relation the source actually carries is keyed. A third value —
   * `process` — was drawn and unkeyed on the first render; A12 caught it, which
   * is the gate W3 did not have when one candidate drew a fifth cell state that
   * appeared in neither its registry, its legend nor its description. */
  const RELATION = {
    governs: { means: 'the decision governs this claim', width: 7 },
    'supersedes-mechanism': { means: 'the decision replaces only this claim mechanism', width: 3 },
    process: { means: 'the decision constrains how the claim is produced, not what it says', width: 1.5 },
  };

  const wrap = (text, chars) => {
    const words = text.split(' ');
    const lines = [];
    let line = '';
    for (const word of words) {
      if (line && `${line} ${word}`.length > chars) { lines.push(line); line = word; } else line = line ? `${line} ${word}` : word;
    }
    if (line) lines.push(line);
    return lines;
  };

  function render(m, { width, mode }) {
    const narrow = mode === 'narrow';
    const claims = m.claims;
    const decisions = [...new Set(claims.flatMap((c) => c.authority.map((a) => a.record)))].sort();

    /* The decision labels are rotated, so the first column needs the width that
     * rotation costs to its left; anchoring at x=26 put ADR-0049 outside the frame. */
    const spanLeft = 46;
    const spanWidth = narrow ? 0 : 26 + decisions.length * 22;
    const textLeft = narrow ? 12 : spanLeft + spanWidth + 18;
    const marginWidth = narrow ? width - 24 : 210;
    const textWidth = narrow ? width - 24 : width - textLeft - marginWidth - 26;
    const chars = Math.max(28, Math.floor(textWidth / 6.4));

    /* A row is as tall as the taller of its two columns: the claim text and the
     * evidence band both wrap, and either can be the one that sets the height. */
    const detailChars = Math.floor((marginWidth - 34) / 5.6);
    const detailOf = (claim) => (claim.evidence.state === 'absent'
      ? (claim.evidence.why ?? claim.evidence.what ?? 'nothing recorded')
      : (claim.evidence.bound ?? claim.evidence.what ?? ''));
    const tops = [];
    let cursor = narrow ? 30 : 44;
    for (const claim of claims) {
      tops.push(cursor);
      const textLines = wrap(`${claim.id} · ${claim.claim}`, chars).length;
      const bandLines = wrap(detailOf(claim), detailChars).length;
      const textHeight = textLines * 16 + 24;
      const bandHeight = 20 + bandLines * 13;
      cursor += narrow ? textHeight + bandHeight + 30 : Math.max(66, textHeight, bandHeight + 16);
    }
    const bottom = cursor;
    const keyRows = narrow ? 5 : 2;
    const height = bottom + 12 + keyRows * 20;

    const svg = el('svg', {
      viewBox: `0 0 ${width} ${height}`, width: '100%', role: 'img',
      'aria-labelledby': 'record-t record-d',
      style: `max-width:${width}px;height:auto;display:block`,
    });
    svg.dataset.emittedBy = 'portal:record-trust';
    svg.dataset.band = mode;
    svg.append(
      el('title', { id: 'record-t' }, `${m.subject.id} — which decision governs this record, how far it reaches, and what stands behind each claim`),
      el('desc', { id: 'record-d' },
        `The ${claims.length} claims of ${m.subject.id} in reading order. `
        + (narrow
          ? 'Each claim carries the decisions that reach it as a bar beneath its own text, and its evidence in a band beneath that; the band keeps its full height when the evidence does not exist. '
          : `Down the left, each of ${decisions.join(', ')} is drawn as a span covering exactly the claims it reaches — a thick span where it governs the claim and a thin one where it replaces only that claim's mechanism — so a decision that reaches part of the record is visibly shorter than the record. Down the right, every claim keeps an evidence position at its own eye level. `)
        + Object.entries(EVIDENCE).map(([id, e]) => `${id}: ${e.means}`).join('; ') + '.'),
    );

    if (!narrow) {
      decisions.forEach((decision, index) => {
        const x = spanLeft + 14 + index * 22;
        const rows = claims.map((claim, k) => ({ k, link: claim.authority.find((a) => a.record === decision) })).filter((r) => r.link);
        if (!rows.length) return;
        const first = tops[rows[0].k];
        const last = tops[rows[rows.length - 1].k] + 12;
        svg.append(el('line', { x1: x, y1: 28, x2: x, y2: bottom - 8, stroke: 'var(--fg-line-soft)', 'stroke-width': 1, 'stroke-dasharray': '1 4' }));
        svg.append(el('text', { x, y: 20, 'font-size': 11, 'text-anchor': 'end', fill: 'var(--sh-ink-muted)', transform: `rotate(-58 ${x} 20)` }, decision));
        for (const row of rows) {
          const spec = RELATION[row.link.relation] ?? RELATION.governs;
          svg.append(el('line', {
            x1: x, y1: tops[row.k] - 4, x2: x, y2: tops[row.k] + 16,
            stroke: 'var(--fg-accent)', 'stroke-width': spec.width, 'stroke-linecap': 'butt',
            'data-state': row.link.relation,
          }));
        }
        svg.append(el('line', { x1: x, y1: first - 4, x2: x, y2: last + 4, stroke: 'var(--fg-accent)', 'stroke-width': 1, opacity: 0.55 }));
      });
    }

    claims.forEach((claim, index) => {
      const y = tops[index];
      const lines = wrap(`${claim.id} · ${claim.claim}`, chars);
      lines.forEach((line, k) => {
        svg.append(el('text', {
          x: textLeft, y: y + 6 + k * 16, 'font-size': 12.5,
          'font-weight': k === 0 ? 600 : 400, fill: 'var(--sh-ink)',
        }, line));
      });
      svg.append(el('text', { x: textLeft, y: y + 6 + lines.length * 16, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, claim.where));

      const spec = EVIDENCE[claim.evidence.state];
      const mx = narrow ? textLeft : width - marginWidth - 12;
      const my = narrow ? y + lines.length * 16 + 30 : y + 2;
      /* The band is sized to its own text. Slicing it to a fixed height was a
       * silent crop — the same defect class the W3 audit found in p3-b. */
      const detailLines = wrap(
        claim.evidence.state === 'absent'
          ? (claim.evidence.why ?? claim.evidence.what ?? 'nothing recorded')
          : (claim.evidence.bound ?? claim.evidence.what ?? ''),
        Math.floor((marginWidth - 34) / 5.6),
      );
      const bandHeight = Math.max(30, 20 + detailLines.length * 13);
      svg.append(el('rect', {
        x: mx, y: my - 12, width: marginWidth, height: bandHeight, rx: 3,
        fill: 'none', stroke: 'var(--fg-line-soft)', 'stroke-width': 1, 'stroke-dasharray': claim.evidence.state === 'absent' ? '3 3' : null,
      }));
      const g = el('g', { 'data-state': claim.evidence.state });
      g.append(el('circle', { cx: mx + 12, cy: my, r: 5, fill: spec.fill, stroke: spec.stroke, 'stroke-width': 1.6, 'stroke-dasharray': spec.dash }));
      detailLines.forEach((line, k) => {
        g.append(el('text', { x: mx + 24, y: my + 3 + k * 13, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, line));
      });
      svg.append(g);
    });

    const keyY = bottom + 6;
    const entries = [
      ...Object.entries(EVIDENCE).map(([id, e]) => ({ id, means: e.means, kind: 'evidence' })),
      ...(narrow ? [] : Object.entries(RELATION).map(([id, r]) => ({ id, means: r.means, kind: 'relation', width: r.width }))),
    ];
    entries.forEach((entry, index) => {
      const kx = narrow ? 8 : 12 + (index % 2) * (width / 2);
      const ky = keyY + (narrow ? index * 20 : Math.floor(index / 2) * 20);
      const g = el('g', { 'data-legend-state': entry.id });
      if (entry.kind === 'evidence') {
        const spec = EVIDENCE[entry.id];
        g.append(el('circle', { cx: kx + 7, cy: ky - 3, r: 5, fill: spec.fill, stroke: spec.stroke, 'stroke-width': 1.6, 'stroke-dasharray': spec.dash }));
      } else {
        g.append(el('line', { x1: kx + 7, y1: ky - 11, x2: kx + 7, y2: ky + 5, stroke: 'var(--fg-accent)', 'stroke-width': entry.width }));
      }
      g.append(el('text', { x: kx + 22, y: ky + 1, 'font-size': 11, fill: 'var(--sh-ink-muted)' }, entry.means));
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

  window.w4record = { render, mount, EVIDENCE, RELATION };
})();
