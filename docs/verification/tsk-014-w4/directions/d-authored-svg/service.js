/* DIRECTION D — authored SVG under a runtime service contract.
 *
 * The mechanism this direction is demonstrated by. Authorship stays with the
 * agent, and the `html` escape's frame is replaced by a real typed block: the
 * document supplies inline SVG only — no HTML, no CSS beyond a declared token
 * vocabulary — and the runtime supplies exactly what the sandboxed escape
 * cannot:
 *
 *   - token substitution, so the drawing follows the document's palette;
 *   - mode resolution, so it follows light and dark;
 *   - a declared responsive variant set, so a narrow context gets the variant
 *     the author drew for it rather than a scaled copy;
 *   - viewBox-based sizing, so the block sizes its own frame without script;
 *   - an enforced legend and accessible description.
 *
 * What it does NOT supply is any judgement about whether the drawing is right.
 * The service can refuse a forbidden element or an unknown token. It cannot
 * refuse a bad encoding, an unkeyed mark or a collision, because it has no model
 * of what the figure means. That is the direction's central trade and it is
 * demonstrated on this page rather than argued.
 */
(() => {
  const TOKENS = new Set([
    'line', 'line-soft', 'fill', 'hatch', 'accent', 'accent-soft', 'warn', 'warn-soft', 'stop', 'ink', 'ink-muted',
  ]);
  const FORBIDDEN = ['script', 'foreignObject', 'a', 'image', 'animate', 'animateMotion', 'animateTransform', 'set', 'use'];

  class Refused extends Error {}

  function qualify(source) {
    const doc = new DOMParser().parseFromString(source, 'image/svg+xml');
    if (doc.querySelector('parsererror')) throw new Refused('the supplied SVG does not parse');
    for (const tag of FORBIDDEN) {
      if (doc.querySelector(tag)) throw new Refused(`<${tag}> can navigate, animate, execute or load a resource`);
    }
    for (const node of doc.querySelectorAll('*')) {
      for (const attribute of node.attributes) {
        const name = attribute.name.toLowerCase();
        if (name.startsWith('on')) throw new Refused(`event attribute ${attribute.name} is not allowed`);
        /* A namespace declaration is a name, not a fetch. Treating it as a
         * resource reference refuses every valid SVG document. */
        if (name === 'xmlns' || name.startsWith('xmlns:')) continue;
        const value = attribute.value;
        const token = value.match(/^token\(([a-z-]+)\)$/);
        if (token && !TOKENS.has(token[1])) throw new Refused(`token(${token[1]}) is not in the declared vocabulary`);
        if (/url\((?!#)/i.test(value) || /https?:|data:/i.test(value)) throw new Refused(`${attribute.name} references an external resource`);
      }
    }
    return doc.documentElement;
  }

  function substitute(root) {
    for (const node of root.querySelectorAll('*')) {
      for (const attribute of [...node.attributes]) {
        const token = attribute.value.match(/^token\(([a-z-]+)\)$/);
        if (token) node.setAttribute(attribute.name, `var(--fg-${token[1]}, var(--sh-${token[1]}))`);
      }
    }
    return root;
  }

  function mount(host, block, breakpoints) {
    const paint = () => {
      const w = window.innerWidth;
      const mode = w <= breakpoints.narrow ? 'narrow' : w <= breakpoints.wide ? 'intermediate' : 'wide';
      const variant = block.variants[mode] ?? block.variants.wide;
      try {
        if (!block.title || !block.description) throw new Refused('an authored figure must carry a title and a description');
        if (!block.legend || !block.legend.length) throw new Refused('an authored figure must key its own marks');
        const root = substitute(qualify(variant));
        root.setAttribute('role', 'img');
        root.setAttribute('width', '100%');
        const vb = root.getAttribute('viewBox').split(/\s+/).map(Number);
        root.setAttribute('style', `max-width:${vb[2]}px;height:auto;display:block`);
        const titleNode = document.createElementNS('http://www.w3.org/2000/svg', 'title');
        titleNode.id = `${block.id}-t`;
        titleNode.textContent = block.title;
        const descNode = document.createElementNS('http://www.w3.org/2000/svg', 'desc');
        descNode.id = `${block.id}-d`;
        descNode.textContent = `${block.description} Keyed marks: ${block.legend.map((l) => `${l.mark} — ${l.means}`).join('; ')}.`;
        root.prepend(descNode);
        root.prepend(titleNode);
        root.setAttribute('aria-labelledby', `${block.id}-t ${block.id}-d`);
        const imported = document.importNode(root, true);
        imported.dataset.emittedBy = `d-authored-svg:${mode}`;
        host.replaceChildren(imported);
        host.dataset.band = mode;
      } catch (error) {
        host.replaceChildren(Object.assign(document.createElement('p'), { className: 'sh-note', textContent: `The service refused this figure: ${error.message}` }));
        host.dataset.refused = 'true';
      }
    };
    paint();
    window.addEventListener('resize', paint);
  }

  window.w4authored = { mount, qualify, TOKENS, FORBIDDEN, Refused };
})();
