import { normalizeSkin, normalizeTypeface } from "./appearance-values";
import type { AppearanceMode, ResolvedMode, TypeScale, Typeface, UtilityTheme } from "./contracts";

const MODE_KEY = "cf-present-mode";
const THEME_KEY = "cf-present-theme";
const TYPEFACE_KEY = "cf-present-typeface";
const SCALE_KEY = "cf-present-scale";
const THEMES: readonly UtilityTheme[] = ["graphite", "slate", "sage"];
const TYPEFACES: readonly Typeface[] = ["archivo", "inter", "plex"];
const SCALES: readonly TypeScale[] = ["compact", "default", "large"];
const MODES: readonly AppearanceMode[] = ["system", "light", "dark"];

export interface AppearanceState {
  readonly theme: UtilityTheme;
  readonly mode: AppearanceMode;
  readonly typeface: Typeface;
  readonly scale: TypeScale;
}

export function initialAppearance(): AppearanceState {
  const theme = readChoice(normalizeSkin(readStorage(THEME_KEY)), THEMES) ?? "graphite";
  const mode = readChoice(readStorage(MODE_KEY), MODES) ?? "system";
  const typeface = readChoice(normalizeTypeface(readStorage(TYPEFACE_KEY)), TYPEFACES) ?? "inter";
  const scale = readChoice(readStorage(SCALE_KEY), SCALES) ?? "default";
  return { theme, mode, typeface, scale };
}

export function applyAppearance(state: AppearanceState): void {
  const resolvedMode = resolveMode(state.mode);
  const root = document.documentElement;
  root.dataset.cfTheme = state.theme;
  root.dataset.cfMode = state.mode;
  root.dataset.cfModeResolved = resolvedMode;
  root.dataset.cfTypeface = state.typeface;
  root.dataset.cfScale = state.scale;
}

export function persistAppearance(state: AppearanceState): void {
  try {
    localStorage.setItem(THEME_KEY, state.theme);
    localStorage.setItem(MODE_KEY, state.mode);
    localStorage.setItem(TYPEFACE_KEY, state.typeface);
    localStorage.setItem(SCALE_KEY, state.scale);
  } catch {
    // Appearance still applies for this session when storage is unavailable.
  }
}

export function watchSystemMode(
  onChange: (resolved: ResolvedMode) => void,
): () => void {
  const query = matchMedia("(prefers-color-scheme: dark)");
  const listener = (): void => onChange(query.matches ? "dark" : "light");
  query.addEventListener("change", listener);
  return () => query.removeEventListener("change", listener);
}

function resolveMode(mode: AppearanceMode): ResolvedMode {
  if (mode !== "system") return mode;
  return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function readChoice<T extends string>(value: string | null, choices: readonly T[]): T | null {
  return value !== null && choices.includes(value as T) ? (value as T) : null;
}

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
