/** Allocate visible positive marks without changing the count labels.
 * Tiny marks consume their floor first; remaining space retains count ratios.
 * Refuse impossible layouts rather than overlapping or hiding positive marks. */

export function segments(counts                   , extent        , minimum        )            {
  if (!Number.isFinite(extent) || extent <= 0 || !Number.isFinite(minimum) || minimum <= 0)
    throw new Error('Chart extent and minimum must be positive finite numbers');
  if (counts.some(n => !Number.isSafeInteger(n) || n < 0)) throw new Error('Chart counts must be nonnegative safe integers');
  const positive = counts.filter(n => n > 0).length;
  if (positive * minimum > extent) throw new Error('Chart cannot fit every positive mark at its minimum size');
  const total = counts.reduce((a, b) => a + b, 0);
  if (!Number.isSafeInteger(total)) throw new Error('Chart total exceeds safe integer precision');
  let remaining = extent, weight = total;
  const sizes = counts.map(() => 0), pending = new Set(counts.flatMap((n, i) => n > 0 ? [i] : []));
  while (pending.size) {
    const small = [...pending].filter(i => counts[i]  / weight * remaining < minimum);
    if (!small.length) { for (const i of pending) sizes[i] = counts[i]  / weight * remaining; break; }
    for (const i of small) { sizes[i] = minimum; remaining -= minimum; weight -= counts[i] ; pending.delete(i); }
  }
  let start = 0;
  return sizes.map((size, i) => { const segment = { count: counts[i] , start, extent: size }; start += size; return segment; });
}
export function ringPath(startDegrees        , extentDegrees        , radius        )         {
  if (![startDegrees, extentDegrees, radius].every(Number.isFinite) || extentDegrees < 0 || extentDegrees > 360 || radius <= 0)
    throw new Error('Ring geometry is outside its finite range');
  if (extentDegrees === 0) return '';
  const point = (angle        )         => { const rad = (angle - 90) * Math.PI / 180; return `${radius * Math.cos(rad)} ${radius * Math.sin(rad)}`; };
  if (extentDegrees === 360) return `M ${point(startDegrees)} A ${radius} ${radius} 0 1 1 ${point(startDegrees + 180)} A ${radius} ${radius} 0 1 1 ${point(startDegrees + 360)}`;
  return `M ${point(startDegrees)} A ${radius} ${radius} 0 ${extentDegrees > 180 ? 1 : 0} 1 ${point(startDegrees + extentDegrees)}`;
}
