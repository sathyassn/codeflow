(() => {
  const root = document.documentElement;
  const themeKey = `codeflow-prototype-theme:${root.dataset.prototype}`;
  let storedTheme;

  try {
    storedTheme = localStorage.getItem(themeKey);
  } catch {
    // A storage policy must not prevent the document from rendering.
  }

  const prefersDark = matchMedia('(prefers-color-scheme: dark)').matches;
  root.dataset.theme = ['light', 'dark'].includes(storedTheme)
    ? storedTheme
    : prefersDark ? 'dark' : 'light';
})();
