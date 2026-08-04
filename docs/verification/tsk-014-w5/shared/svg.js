/* Low-level SVG plumbing, shared by directions A, B and C.
 *
 * Declared honestly: this is *not* shared composition authority. It creates
 * elements, measures text and draws a legend. It knows nothing about waves,
 * claims, records, scales or marks. What differs between A, B and C is the
 * contract the document presents and who decides the composition — not which
 * function calls createElementNS. Sharing this file is what lets the board
 * compare contracts rather than compare two people's SVG habits.
 */
(() => {
  const NS = 'http://www.w3.org/2000/svg';

  const el = (name, attrs = {}, text) => {
    const node = document.createElementNS(NS, name);
    for (const [key, value] of Object.entries(attrs)) {
      if (value === undefined || value === null) continue;
      node.setAttribute(key, String(value));
    }
    if (text !== undefined) node.textContent = String(text);
    return node;
  };

  /* A figure root with the accessible equivalent attached, never optional. */
  const figure = ({ width, height, title, desc, id }) => {
    const svg = el('svg', {
      viewBox: `0 0 ${width} ${height}`,
      width: '100%',
      role: 'img',
      'aria-labelledby': `${id}-t ${id}-d`,
      style: `max-width:${width}px;height:auto;display:block`,
    });
    svg.append(el('title', { id: `${id}-t` }, title), el('desc', { id: `${id}-d` }, desc));
    return svg;
  };

  /* Non-colour channels. Every state that a figure draws must resolve here, and
   * every one of these carries a channel that is not colour. A12 compares the
   * set a page draws against the set the registry declares, the set the legend
   * keys and the set the description names. */
  const CHANNELS = {
    run: { stroke: 'var(--fg-line)', 'stroke-width': 4, 'stroke-linecap': 'round' },
    room: { stroke: 'var(--fg-warn)', 'stroke-width': 2, 'stroke-dasharray': '5 3' },
    cap: { stroke: 'var(--fg-warn)', 'stroke-width': 3, 'stroke-linecap': 'butt' },
    blocked: { stroke: 'var(--fg-line-mid)', 'stroke-width': 1.75, 'stroke-dasharray': '4 3' },
    reached: { stroke: 'var(--fg-line)', 'stroke-width': 4, 'stroke-linecap': 'round' },
    stopped: { stroke: 'var(--fg-stop)', 'stroke-width': 3 },
    reserved: { stroke: 'var(--fg-line-soft)', 'stroke-width': 1.5, 'stroke-dasharray': '2 4' },
    governed: { stroke: 'var(--fg-accent)', 'stroke-width': 6, 'stroke-linecap': 'butt' },
    ungoverned: { stroke: 'var(--fg-line-soft)', 'stroke-width': 1.5, 'stroke-dasharray': '1 4' },
    backed: { fill: 'var(--fg-accent)', stroke: 'none' },
    absent: { fill: 'none', stroke: 'var(--fg-stop)', 'stroke-width': 1.5, 'stroke-dasharray': '3 2' },
  };

  /* Cell states carry shape, not colour: a solid square, an outline, a hatch,
   * an empty slot and a cross are five distinguishable marks in greyscale. */
  const cell = (x, y, size, state) => {
    const g = el('g', { 'data-state': state });
    const box = { x, y, width: size, height: size, rx: 1.5 };
    if (state === 'executed') {
      g.append(el('rect', { ...box, fill: 'var(--fg-line)' }));
    } else if (state === 'bounded') {
      g.append(el('rect', { ...box, fill: 'none', stroke: 'var(--fg-line)', 'stroke-width': 1.4, 'stroke-dasharray': '2.5 1.8' }));
    } else if (state === 'notrun') {
      g.append(el('rect', { ...box, fill: 'url(#w4-hatch)', stroke: 'var(--fg-hatch)', 'stroke-width': 1 }));
    } else if (state === 'na') {
      g.append(el('rect', { ...box, fill: 'none', stroke: 'var(--fg-rule, var(--fg-line-soft))', 'stroke-width': 1, opacity: 0.5 }));
    } else if (state === 'notclaimed') {
      g.append(
        el('rect', { ...box, fill: 'none', stroke: 'var(--fg-line-soft)', 'stroke-width': 1 }),
        /* The cross is what distinguishes this state, so IT is the mark the floor
           applies to — not the box around it, which every state shares. A fixed
           2px inset left it 8px inside a 12px cell, and 6.6px once the figure
           scaled down: under the 9px floor at every band, and invisible to the
           gate because the measurement stopped at the group. The inset is a
           fraction of the cell now, so the cross keeps its share of whatever
           size the cell actually renders at. */
        el('path', { d: `M${x + size * 0.06} ${y + size * 0.06}L${x + size * 0.94} ${y + size * 0.94}M${x + size * 0.94} ${y + size * 0.06}L${x + size * 0.06} ${y + size * 0.94}`, stroke: 'var(--fg-stop)', 'stroke-width': 1.4 }),
      );
    } else {
      throw new Error(`unkeyed cell state: ${state}`);
    }
    return g;
  };

  const hatchDefs = () => {
    const defs = el('defs');
    const pattern = el('pattern', { id: 'w4-hatch', width: 5, height: 5, patternUnits: 'userSpaceOnUse', patternTransform: 'rotate(45)' });
    pattern.append(el('line', { x1: 0, y1: 0, x2: 0, y2: 5, stroke: 'var(--fg-hatch)', 'stroke-width': 1.4 }));
    defs.append(pattern);
    return defs;
  };

  /* The visible legend. A12 requires every drawn state to appear here, so the
   * legend is emitted from the same state set the figure was drawn from rather
   * than written by hand beside it. */
  const legend = (entries, { width, y, columns = 3, swatch }) => {
    const g = el('g', { 'data-legend': 'true' });
    const colWidth = width / columns;
    entries.forEach((entry, index) => {
      const cx = (index % columns) * colWidth;
      const cy = y + Math.floor(index / columns) * 20;
      const item = el('g', { transform: `translate(${cx} ${cy})`, 'data-legend-state': entry.state });
      item.append(swatch(entry.state));
      item.append(el('text', { x: 34, y: 4, 'font-size': 11.5, fill: 'var(--sh-ink-muted, #5f5a53)' }, entry.means));
      g.append(item);
    });
    return g;
  };

  const legendRows = (count, columns = 3) => Math.ceil(count / columns) * 20;

  window.w4svg = { NS, el, figure, CHANNELS, cell, hatchDefs, legend, legendRows };
})();
