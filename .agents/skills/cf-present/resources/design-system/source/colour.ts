/** D65 sRGB colour measurements. CIEDE2000: Sharma, Wu & Dalal (2005).
 * Deficiency matrices: Machado, Oliveira & Fernandes (2009), severity 1.0.
 * Simulation operates on linear RGB and clips to the display gamut. */
export type Triple = readonly [number, number, number];
export type Deficiency = 'protan' | 'deutan' | 'tritan';
import matrixValues from './matrices.json' with {type: 'json'};
export const matrices = matrixValues as unknown as Readonly<Record<Deficiency, readonly Triple[]>>;
export function rgb(hex: string): Triple {
  if (!/^#[0-9a-f]{6}$/i.test(hex)) throw new Error(`Expected opaque sRGB hex, received ${hex}`);
  return [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255) as unknown as Triple;
}
export function linear(c: number): number { return c <= .04045 ? c / 12.92 : ((c + .055) / 1.055) ** 2.4; }
export function linearRgb(hex: string): Triple { return rgb(hex).map(linear) as unknown as Triple; }
export function labFromLinear([r, g, b]: Triple): Triple {
  const f = (v: number): number => v > 216 / 24389 ? Math.cbrt(v) : (24389 / 27 * v + 16) / 116;
  const x = f((r * .4124564 + g * .3575761 + b * .1804375) / .95047);
  const y = f(r * .2126729 + g * .7151522 + b * .0721750);
  const z = f((r * .0193339 + g * .1191920 + b * .9503041) / 1.08883);
  return [116 * y - 16, 500 * (x - y), 200 * (y - z)];
}
export function lab(hex: string): Triple { return labFromLinear(linearRgb(hex)); }
export function simulatedLab(hex: string, kind: Deficiency): Triple {
  const c = linearRgb(hex);
  return labFromLinear(matrices[kind].map(row => Math.max(0, Math.min(1, row.reduce((sum, v, i) => sum + v * c[i]!, 0)))) as unknown as Triple);
}
export function contrast(a: string, b: string): number {
  const y = (s: string): number => { const [r, g, b] = linearRgb(s); return r * .2126 + g * .7152 + b * .0722; };
  const x = y(a), z = y(b); return (Math.max(x, z) + .05) / (Math.min(x, z) + .05);
}
const rad = (x: number): number => x * Math.PI / 180;
const hue = (a: number, b: number): number => (Math.atan2(b, a) * 180 / Math.PI + 360) % 360;
export function hueGap(a: string, b: string): number {
  const x = lab(a), y = lab(b), d = Math.abs(hue(x[1], x[2]) - hue(y[1], y[2])); return Math.min(d, 360 - d);
}
export function deltaE(a: Triple, b: Triple): number {
  const [l1, a1, b1] = a, [l2, a2, b2] = b;
  const seventh = (c: number): number => Math.sqrt(c ** 7 / (c ** 7 + 25 ** 7));
  const g = .5 * (1 - seventh((Math.hypot(a1, b1) + Math.hypot(a2, b2)) / 2));
  const ap1 = (1 + g) * a1, ap2 = (1 + g) * a2, c1 = Math.hypot(ap1, b1), c2 = Math.hypot(ap2, b2);
  const h1 = hue(ap1, b1), h2 = hue(ap2, b2), zero = c1 * c2 === 0;
  let dh = zero ? 0 : h2 - h1; if (dh > 180) dh -= 360; else if (dh < -180) dh += 360;
  const dH = 2 * Math.sqrt(c1 * c2) * Math.sin(rad(dh / 2));
  const L = (l1 + l2) / 2, C = (c1 + c2) / 2;
  let H = zero ? h1 + h2 : (h1 + h2) / 2; if (!zero && Math.abs(h1 - h2) > 180) H += H < 180 ? 180 : -180;
  const cos = (v: number): number => Math.cos(rad(v));
  const t = 1 - .17 * cos(H - 30) + .24 * cos(2 * H) + .32 * cos(3 * H + 6) - .20 * cos(4 * H - 63);
  const sl = 1 + .015 * (L - 50) ** 2 / Math.sqrt(20 + (L - 50) ** 2), sc = 1 + .045 * C, sh = 1 + .015 * C * t;
  const rt = -2 * seventh(C) * Math.sin(rad(60 * Math.exp(-(((H - 275) / 25) ** 2))));
  const dl = (l2 - l1) / sl, dc = (c2 - c1) / sc, dd = dH / sh;
  return Math.sqrt(dl * dl + dc * dc + dd * dd + rt * dc * dd);
}
export function separation(a: string, b: string, kind?: Deficiency): number {
  return deltaE(kind ? simulatedLab(a, kind) : lab(a), kind ? simulatedLab(b, kind) : lab(b));
}
