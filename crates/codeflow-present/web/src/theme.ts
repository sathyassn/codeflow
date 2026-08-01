import type { AppearanceMode, ResolvedMode, UtilityTheme } from "./contracts";

const MODE_KEY = "cf-present-mode";
const THEME_KEY = "cf-present-theme";
const THEMES: readonly UtilityTheme[] = ["editorial", "technical"];
const MODES: readonly AppearanceMode[] = ["system", "light", "dark"];

export interface AppearanceState {
  readonly theme: UtilityTheme;
  readonly mode: AppearanceMode;
}

export function initialAppearance(): AppearanceState {
  const theme = readChoice(readStorage(THEME_KEY), THEMES) ?? "editorial";
  const mode = readChoice(readStorage(MODE_KEY), MODES) ?? "system";
  return { theme, mode };
}

export function applyAppearance(theme: UtilityTheme, mode: AppearanceMode): void {
  const resolvedMode = resolveMode(mode);
  document.documentElement.dataset.cfTheme = theme;
  document.documentElement.dataset.cfMode = mode;
  document.documentElement.dataset.cfModeResolved = resolvedMode;
}

export function persistAppearance(theme: UtilityTheme, mode: AppearanceMode): void {
  try {
    localStorage.setItem(THEME_KEY, theme);
    localStorage.setItem(MODE_KEY, mode);
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
