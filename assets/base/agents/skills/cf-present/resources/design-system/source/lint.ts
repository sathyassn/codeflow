import namedColours from './named-colours.json' with { type: 'json' };
/** Lint authored sheets/templates. Generated sheets are checked by regeneration,
 * not exempted by a user-editable comment or filename pattern. */
export function styleLiterals(css: string): string[] {
  const source = css.replace(/\/\*[\s\S]*?\*\//g, '').replace(/url\([^)]*\)/g, '').replace(/\{\{[\w-]+\}\}/g, '');
  const hits = [...source.matchAll(/#[\da-f]{3,8}\b|\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(|(?<![\w-])-?(?:\d*\.\d+|\d+)(?:[eE][+-]?\d+)?(?:px|rem|em|ch|ex|cap|ic|lh|rlh|vw|vh|vi|vb|svw|svh|lvw|lvh|dvw|dvh|vmin|vmax|cm|mm|Q|in|pt|pc|%)/gi)].map(m => m[0]);
  hits.push(...[...source.matchAll(/--cf-colou?r-[0-9a-f]+\b/gi)].map(match => match[0]));
  // Intrinsic zero, currentColor, transparent and geometry supplied by data
  // are not authored palette/size choices. Named colour values still are.
  for (const [, block] of source.matchAll(/\{([^{}]*)\}/g)) for (const declaration of block!.split(';')) {
    const colon = declaration.indexOf(':'); if (colon < 0) continue;
    const value = declaration.slice(colon + 1).replace(/--[\w-]+/g, '');
    if (value.split(/[^a-zA-Z]+/).some(word => namedColours.includes(word.toLowerCase()))) hits.push(value.trim());
  }
  return hits;
}
