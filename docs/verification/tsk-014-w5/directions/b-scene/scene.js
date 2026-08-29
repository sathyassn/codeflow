/* DIRECTION B — bounded declarative scene grammar.
 *
 * The mechanism this direction is demonstrated by. A document carries a scene:
 * named scales over declared domains, a closed vocabulary of marks positioned
 * through those scales, guides, per-band recomposition, states with non-colour
 * channels, a legend and an accessible description. This compiler turns that
 * into SVG in the parent document, where it inherits the document's tokens and
 * resolved mode.
 *
 * There is no imperative hook a document can reach. A scene cannot supply a
 * function, a coordinate the compiler did not derive from a scale, a mark kind
 * outside the closed set, or a state that is not declared and keyed. Every one
 * of those is a refusal with a named reason, which is the whole difference
 * between a grammar and an escape hatch.
 *
 * What it deliberately does NOT have: any layout algorithm. There is no force
 * layout, no treemap, no packing, no automatic label de-collision. A scene that
 * wants one must state the positions itself, at which point the document
 * computed the layout and the model did not — that is direction B's negative
 * artefact, and it is demonstrated rather than described.
 */
(() => {
  const { el, figure, CHANNELS, cell, hatchDefs, legend, legendRows } = window.w4svg;

  const MARK_KINDS = ['segment', 'tick', 'path', 'label', 'region', 'rule', 'cell', 'marker'];

  class Refusal extends Error {}

  /* ---- scales ---------------------------------------------------------- */

  function buildScale(name, spec) {
    if (!['linear', 'band'].includes(spec.type)) {
      throw new Refusal(`scale "${name}": unknown type "${spec.type}" (linear, band)`);
    }
    if (!['x', 'y'].includes(spec.axis)) {
      throw new Refusal(`scale "${name}": axis must be x or y`);
    }
    const [r0, r1] = spec.range;
    if (spec.type === 'linear') {
      const [d0, d1] = spec.domain;
      return {
        name, ...spec,
        at(value) {
          if (typeof value !== 'number') throw new Refusal(`scale "${name}" is linear; got ${JSON.stringify(value)}`);
          return r0 + ((value - d0) / (d1 - d0)) * (r1 - r0);
        },
        size(span) { return Math.abs((span / (d1 - d0)) * (r1 - r0)); },
        thickness: 0,
      };
    }
    const step = (r1 - r0) / spec.domain.length;
    const pad = (spec.pad ?? 0.2) * step;
    return {
      name, ...spec,
      at(value) {
        const index = spec.domain.indexOf(value);
        if (index < 0) throw new Refusal(`scale "${name}": "${value}" is not in its declared domain`);
        return r0 + index * step + step / 2;
      },
      edge(value, side) {
        const centre = this.at(value);
        return side === 'start' ? centre - step / 2 + pad / 2 : centre + step / 2 - pad / 2;
      },
      thickness: step - pad,
      step,
    };
  }

  /* ---- positions ------------------------------------------------------- */

  function resolve(position, scales, axis) {
    const names = Object.keys(position).filter((key) => scales[key] && scales[key].axis === axis);
    if (names.length === 0) throw new Refusal(`position ${JSON.stringify(position)} names no ${axis} scale`);
    if (names.length > 1) throw new Refusal(`position ${JSON.stringify(position)} names ${names.length} ${axis} scales`);
    const scale = scales[names[0]];
    let value = scale.at(position[names[0]]);
    const nudge = position[axis === 'x' ? 'dx' : 'dy'];
    if (typeof nudge === 'number') {
      if (Math.abs(nudge) > 24) throw new Refusal('a positional nudge over 24 units is hand placement, not a scale');
      value += nudge;
    }
    return value;
  }

  const xy = (position, scales) => [resolve(position, scales, 'x'), resolve(position, scales, 'y')];

  /* ---- marks ----------------------------------------------------------- */

  function drawMark(mark, scales, declaredStates) {
    if (!MARK_KINDS.includes(mark.kind)) {
      throw new Refusal(`mark kind "${mark.kind}" is outside the grammar (${MARK_KINDS.join(', ')})`);
    }
    if (mark.state && !declaredStates.includes(mark.state)) {
      throw new Refusal(`mark uses state "${mark.state}", which the scene does not declare`);
    }
    const channel = mark.state ? CHANNELS[mark.state] : null;
    if (mark.state && !channel && !['cell'].includes(mark.kind)) {
      throw new Refusal(`state "${mark.state}" has no non-colour channel`);
    }

    switch (mark.kind) {
      case 'segment': {
        const [x1, y1] = xy(mark.from, scales);
        const [x2, y2] = xy(mark.to, scales);
        return el('line', { x1, y1, x2, y2, fill: 'none', ...channel, 'data-state': mark.state });
      }
      case 'tick': {
        const [x, y] = xy(mark.at, scales);
        const half = (mark.size ?? 9) / 2;
        const across = mark.along === 'x'
          ? { x1: x - half, y1: y, x2: x + half, y2: y }
          : { x1: x, y1: y - half, x2: x, y2: y + half };
        return el('line', { ...across, ...channel, 'data-state': mark.state });
      }
      case 'path': {
        const points = mark.through.map((p) => xy(p, scales));
        const d = points
          .map(([x, y], index) => {
            if (index === 0) return `M${x} ${y}`;
            if (mark.curve !== 'ortho') return `L${x} ${y}`;
            const [px] = points[index - 1];
            return `L${px} ${y}L${x} ${y}`;
          })
          .join('');
        return el('path', { d, fill: 'none', ...channel, 'data-state': mark.state });
      }
      case 'label': {
        const [x, y] = xy(mark.at, scales);
        if (mark.rotate !== undefined && Math.abs(mark.rotate) > 90) {
          throw new Refusal('a label rotation beyond 90 degrees is not in the grammar');
        }
        return el('text', {
          x, y,
          transform: mark.rotate ? `rotate(${mark.rotate} ${x} ${y})` : null,
          'text-anchor': mark.anchor ?? 'start',
          'font-size': mark.size ?? 12,
          'font-weight': mark.weight ?? 400,
          'font-family': mark.mono ? 'var(--sh-mono)' : 'inherit',
          fill: mark.muted ? 'var(--sh-ink-muted)' : 'var(--sh-ink)',
          'dominant-baseline': 'middle',
        }, mark.text);
      }
      case 'region': {
        const [x1, y1] = xy(mark.from, scales);
        const [x2, y2] = xy(mark.to, scales);
        return el('rect', {
          x: Math.min(x1, x2), y: Math.min(y1, y2),
          width: Math.abs(x2 - x1), height: Math.abs(y2 - y1),
          fill: mark.state === 'notrun' ? 'url(#w4-hatch)' : 'var(--fg-fill)',
          stroke: 'var(--fg-hatch)', 'stroke-width': 1,
          'stroke-dasharray': mark.open ? '4 3' : null,
          'data-state': mark.state,
        });
      }
      case 'rule': {
        const scale = scales[mark.scale];
        if (!scale) throw new Refusal(`rule names unknown scale "${mark.scale}"`);
        const at = scale.at(mark.at);
        const [e0, e1] = mark.extent;
        return scale.axis === 'x'
          ? el('line', { x1: at, y1: e0, x2: at, y2: e1, stroke: 'var(--fg-line-soft)', 'stroke-width': 1 })
          : el('line', { x1: e0, y1: at, x2: e1, y2: at, stroke: 'var(--fg-line-soft)', 'stroke-width': 1 });
      }
      case 'cell': {
        const [x, y] = xy(mark.at, scales);
        const size = mark.size ?? 13;
        return cell(x - size / 2, y - size / 2, size, mark.state);
      }
      case 'marker': {
        const [x, y] = xy(mark.at, scales);
        const r = mark.size ?? 4;
        return mark.shape === 'square'
          ? el('rect', { x: x - r, y: y - r, width: r * 2, height: r * 2, ...channel, 'data-state': mark.state })
          : el('circle', { cx: x, cy: y, r, ...channel, 'data-state': mark.state });
      }
      default:
        throw new Refusal(`unreachable mark kind ${mark.kind}`);
    }
  }

  /* ---- compile --------------------------------------------------------- */

  function merge(base, patch) {
    if (!patch) return base;
    const out = { ...base };
    for (const [key, value] of Object.entries(patch)) {
      if (Array.isArray(value)) out[key] = value;
      else if (value && typeof value === 'object') out[key] = merge(base[key] ?? {}, value);
      else out[key] = value;
    }
    return out;
  }

  function compile(document_, band = 'wide') {
    const scene = merge(document_, document_.bands?.[band]);
    if (!scene.title || !scene.desc) throw new Refusal('a scene must carry a title and a description');
    const states = scene.states ?? [];
    const scales = {};
    for (const [name, spec] of Object.entries(scene.scales)) scales[name] = buildScale(name, spec);

    const legendHeight = scene.legend?.length ? legendRows(scene.legend.length, scene.legendColumns ?? 3) + 14 : 0;
    const svg = figure({
      width: scene.frame.width,
      height: scene.frame.height + legendHeight,
      title: scene.title,
      desc: scene.desc,
      id: scene.id,
    });
    svg.dataset.compiledBy = 'b-scene';
    svg.dataset.band = band;
    svg.append(hatchDefs());

    const plot = el('g', { 'data-plot': 'true' });
    for (const mark of scene.marks) plot.append(drawMark(mark, scales, states));
    svg.append(plot);

    if (scene.legend?.length) {
      const drawn = new Set(scene.marks.filter((m) => m.state).map((m) => m.state));
      const keyed = new Set(scene.legend.map((entry) => entry.state));
      for (const state of drawn) {
        if (!keyed.has(state)) throw new Refusal(`state "${state}" is drawn and not keyed — A12`);
      }
      svg.append(legend(scene.legend, {
        width: scene.frame.width,
        y: scene.frame.height + 12,
        columns: scene.legendColumns ?? 3,
        swatch: (state) => {
          const g = el('g');
          if (['executed', 'bounded', 'notrun', 'na', 'notclaimed'].includes(state)) {
            g.append(cell(0, -6, 12, state));
          } else if (state === 'backed' || state === 'absent') {
            g.append(el('circle', { cx: 6, cy: 0, r: 4.5, ...CHANNELS[state] }));
          } else if (state === 'cap' || state === 'stopped') {
            /* Both are drawn as a tick across their rail; a horizontal swatch
             * would key a mark the figure never draws. */
            g.append(el('line', { x1: 13, y1: -8, x2: 13, y2: 8, ...CHANNELS[state] }));
          } else {
            g.append(el('line', { x1: 0, y1: 0, x2: 26, y2: 0, ...CHANNELS[state] }));
          }
          return g;
        },
      }));
    }
    return svg;
  }

  /* The runtime mounts a scene and owns recomposition. The document does not
   * script; it declares bands, and this picks one and re-picks on resize. */
  function mount(host, document_, breakpoints) {
    const pick = () => {
      const w = window.innerWidth;
      if (w <= breakpoints.narrow) return 'narrow';
      if (w <= breakpoints.wide) return 'intermediate';
      return 'wide';
    };
    const paint = () => {
      const band = pick();
      host.replaceChildren(compile(document_, band));
      host.dataset.band = band;
    };
    paint();
    window.addEventListener('resize', paint);
    return paint;
  }

  window.w4scene = { compile, mount, Refusal, MARK_KINDS };
})();
