// Compatibility affects only the selected axis; a skin never selects a font.
export function normalizeSkin(value: string | null): string | null {
  switch (value) {
    case "instrument": case "technical": return "graphite";
    case "editorial": return "slate";
    case "ink": return "sage";
    default: return value;
  }
}
export function normalizeTypeface(value: string | null): string | null {
  switch (value) {
    case "instrument": return "archivo";
    case "editorial": return "inter";
    default: return value;
  }
}
