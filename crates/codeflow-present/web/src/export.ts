import { DOCUMENT_ROOT_ID } from "./contracts";
import { enhanceDocument } from "./enhance";
import styles from "./styles.css";
import figureStyles from "./figure.css";
import { installFonts } from "./fonts";

installFonts();

const documentRoot = document.getElementById(DOCUMENT_ROOT_ID);
if (!(documentRoot instanceof HTMLElement)) {
  throw new Error("cf-present export document root is required");
}

document.documentElement.dataset.cfExport = "true";
if (!document.querySelector("style[data-cf-present-export-style]")) {
  const style = document.createElement("style");
  style.dataset.cfPresentExportStyle = "true";
  style.textContent = `${styles}\n${figureStyles}`;
  document.head.append(style);
}
// A srcdoc document cannot inherit the parent page's custom properties.
// Carry the computed theme into the existing sandbox without adding permissions
// or duplicating the entire interface stylesheet in every exported stage.
const stages = [...documentRoot.querySelectorAll<HTMLIFrameElement>("iframe[data-cf-export-stage]")]
  .map(frame => ({ frame, source: frame.srcdoc }));
const styleStages = (): void => {
  const computed = getComputedStyle(document.documentElement);
  const carried = document.createElement("div").style;
  for (const name of computed) {
    if (name.startsWith("--cf-")) carried.setProperty(name, computed.getPropertyValue(name));
  }
  for (const name of ["color", "color-scheme", "font-family"]) {
    carried.setProperty(name, computed.getPropertyValue(name));
  }
  // CSS serialization keeps values intact; a less-than sign must never close
  // the generated style element, even in a project-provided font name.
  const css = carried.cssText.replaceAll("<", "\\3c ");
  for (const { frame, source } of stages) {
    frame.srcdoc = `<!doctype html><meta charset="utf-8"><style>:root{${css}}</style>${source}`;
  }
};
const requestedMode = document.documentElement.dataset.cfMode ?? "system";
if (requestedMode === "system") {
  const preference = matchMedia("(prefers-color-scheme: dark)");
  const applyPreference = (): void => {
    document.documentElement.dataset.cfModeResolved = preference.matches ? "dark" : "light";
    styleStages();
  };
  applyPreference();
  preference.addEventListener("change", applyPreference);
} else {
  document.documentElement.dataset.cfModeResolved = requestedMode === "dark" ? "dark" : "light";
  styleStages();
}
enhanceDocument(documentRoot, true);
