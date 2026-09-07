import { DOCUMENT_ROOT_ID } from "./contracts";
import { enhanceDocument } from "./enhance";
import styles from "./styles.css";
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
  style.textContent = styles;
  document.head.append(style);
}
const requestedMode = document.documentElement.dataset.cfMode ?? "system";
if (requestedMode === "system") {
  const preference = matchMedia("(prefers-color-scheme: dark)");
  const applyPreference = (): void => {
    document.documentElement.dataset.cfModeResolved = preference.matches ? "dark" : "light";
  };
  applyPreference();
  preference.addEventListener("change", applyPreference);
} else {
  document.documentElement.dataset.cfModeResolved = requestedMode === "dark" ? "dark" : "light";
}
enhanceDocument(documentRoot, true);
