import { createHighlighterCore, type HighlighterCore } from "@shikijs/core";
import { createJavaScriptRegexEngine } from "@shikijs/engine-javascript";
import { semanticSyntaxTheme, syntaxClassByColor } from "./syntax-theme";

const aliases: Readonly<Record<string, Language>> = {
  bash: "bash",
  sh: "bash",
  shell: "bash",
  diff: "diff",
  javascript: "javascript",
  js: "javascript",
  json: "json",
  python: "python",
  py: "python",
  rust: "rust",
  rs: "rust",
  toml: "toml",
  typescript: "typescript",
  ts: "typescript",
  yaml: "yaml",
  yml: "yaml",
};

const loaders = {
  bash: () => import("@shikijs/langs/bash"),
  diff: () => import("@shikijs/langs/diff"),
  javascript: () => import("@shikijs/langs/javascript"),
  json: () => import("@shikijs/langs/json"),
  python: () => import("@shikijs/langs/python"),
  rust: () => import("@shikijs/langs/rust"),
  toml: () => import("@shikijs/langs/toml"),
  typescript: () => import("@shikijs/langs/typescript"),
  yaml: () => import("@shikijs/langs/yaml"),
} as const;

type Language = keyof typeof loaders;
let highlighterPromise: Promise<HighlighterCore> | undefined;
const loaded = new Set<Language>();

export function supportedLanguage(value: string): Language | null {
  return aliases[value.trim().toLowerCase()] ?? null;
}

export async function highlightCode(element: HTMLElement, languageLabel: string): Promise<boolean> {
  const language = supportedLanguage(languageLabel);
  if (!language) {
    element.dataset.cfHighlight = "plain";
    return false;
  }
  const highlighter = await getHighlighter();
  if (!loaded.has(language)) {
    const module = await loaders[language]();
    await highlighter.loadLanguage(...module.default);
    loaded.add(language);
  }
  const code = element.textContent ?? "";
  const result = highlighter.codeToTokens(code, { lang: language, theme: semanticSyntaxTheme.name });
  const fragment = document.createDocumentFragment();
  result.tokens.forEach((line, lineIndex) => {
    line.forEach((token) => {
      const span = document.createElement("span");
      span.className = syntaxClassByColor[token.color?.toLowerCase() ?? ""] ?? "syntax-base";
      span.textContent = token.content;
      fragment.append(span);
    });
    if (lineIndex + 1 < result.tokens.length) fragment.append("\n");
  });
  element.replaceChildren(fragment);
  element.dataset.cfHighlight = "ready";
  return true;
}

async function getHighlighter(): Promise<HighlighterCore> {
  highlighterPromise ??= createHighlighterCore({
    engine: createJavaScriptRegexEngine(),
    themes: [semanticSyntaxTheme],
    langs: [],
  });
  return highlighterPromise;
}
