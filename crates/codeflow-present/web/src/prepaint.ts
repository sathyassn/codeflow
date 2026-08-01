(() => {
  const themes = new Set(["editorial", "technical"]);
  const modes = new Set(["system", "light", "dark"]);
  let storedTheme: string | null = null;
  let storedMode: string | null = null;
  try {
    storedTheme = localStorage.getItem("cf-present-theme");
    storedMode = localStorage.getItem("cf-present-mode");
  } catch {
    // Defaults below remain usable when storage is unavailable.
  }
  const theme = storedTheme && themes.has(storedTheme) ? storedTheme : "editorial";
  const mode = storedMode && modes.has(storedMode) ? storedMode : "system";
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
})();
