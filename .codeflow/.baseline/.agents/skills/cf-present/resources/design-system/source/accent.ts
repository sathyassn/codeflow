import { contrast } from './colour.ts';
function mix(a: string, b: string, portion: number): string {
  const channel = (s: string, i: number): number => parseInt(s.slice(i, i + 2), 16);
  return '#' + [1, 3, 5].map(i => Math.round(channel(a, i) * portion + channel(b, i) * (1 - portion)).toString(16).padStart(2, '0')).join('');
}
export function accentFamily(base: string, mode: string, surface: string, textFloor: number): Record<string, string> {
  // Mixing toward an achromatic endpoint retains the base's sRGB hue.
  const strong = mix(base, mode === 'light' ? '#000000' : '#ffffff', .8);
  let portion = .14;
  while (portion > 0 && contrast(base, mix(base, surface, portion)) < textFloor) portion = Math.max(0, portion - .01);
  const soft = mix(base, surface, portion);
  const on = contrast(base, '#ffffff') >= contrast(base, '#000000') ? '#ffffff' : '#000000';
  return { accent: base, 'accent-strong': strong, 'accent-soft': soft, focus: base, 'on-accent': on, 'fig-accent': base, 'fig-accent-soft': soft };
}
