(() => {
  const themes = new Set(["graphite", "slate", "sage"]);
  const modes = new Set(["system", "light", "dark"]);
  const typefaces = new Set(["archivo", "inter", "plex"]);
  const scales = new Set(["compact", "default", "large"]);
  let storedTheme: string | null = null;
  let storedMode: string | null = null;
  let storedTypeface: string | null = null;
  let storedScale: string | null = null;
  try {
    storedTheme = localStorage.getItem("cf-present-theme");
    storedMode = localStorage.getItem("cf-present-mode");
    storedTypeface = localStorage.getItem("cf-present-typeface");
    storedScale = localStorage.getItem("cf-present-scale");
  } catch {
    // Defaults below remain usable when storage is unavailable.
  }
  const theme = storedTheme && themes.has(storedTheme) ? storedTheme : "graphite";
  const mode = storedMode && modes.has(storedMode) ? storedMode : "system";
  const typeface = storedTypeface && typefaces.has(storedTypeface) ? storedTypeface : "inter";
  const scale = storedScale && scales.has(storedScale) ? storedScale : "default";
  const resolved =
    mode === "system"
      ? matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : mode;
  const root = document.documentElement;
  root.dataset.cfTheme = theme;
  root.dataset.cfMode = mode;
  root.dataset.cfModeResolved = resolved;
  root.dataset.cfTypeface = typeface;
  root.dataset.cfScale = scale;
})();
