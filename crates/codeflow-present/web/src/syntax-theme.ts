export const semanticSyntaxTheme = {
  name: "cf-semantic",
  type: "light" as const,
  fg: "#17191f",
  bg: "#ffffff",
  colors: {
    "editor.foreground": "#17191f",
    "editor.background": "#ffffff",
  },
  settings: [
    { settings: { foreground: "#17191f" } },
    { scope: ["comment", "punctuation.definition.comment"], settings: { foreground: "#667085", fontStyle: "italic" } },
    { scope: ["keyword", "storage", "variable.language"], settings: { foreground: "#7c2d5d" } },
    { scope: ["string", "constant.other.symbol"], settings: { foreground: "#176b49" } },
    { scope: ["constant.numeric", "constant.language"], settings: { foreground: "#a13d00" } },
    { scope: ["entity.name.function", "support.function"], settings: { foreground: "#1458a5" } },
    { scope: ["entity.name.type", "support.type", "support.class"], settings: { foreground: "#6246a5" } },
    { scope: ["variable.other.property", "support.variable.property"], settings: { foreground: "#0b6574" } },
    { scope: ["invalid", "invalid.illegal"], settings: { foreground: "#9b1c1c", fontStyle: "underline" } },
  ],
};

export const syntaxClassByColor: Readonly<Record<string, string>> = {
  "#17191f": "syntax-base",
  "#667085": "syntax-comment",
  "#7c2d5d": "syntax-keyword",
  "#176b49": "syntax-string",
  "#a13d00": "syntax-number",
  "#1458a5": "syntax-function",
  "#6246a5": "syntax-type",
  "#0b6574": "syntax-property",
  "#9b1c1c": "syntax-invalid",
};
