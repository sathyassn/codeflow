import { DOCUMENT_ROOT_ID } from "./contracts";
import { enhanceDocument } from "./enhance";
import styles from "./styles.css";
import figureStyles from "./figure.css";
import { installFonts } from "./fonts";
import { frameLoad, sizeStage, stageSource, type StageMeasurement } from "./export-stages";

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
  .map((frame): { frame: HTMLIFrameElement; source: string } => ({ frame, source: frame.srcdoc }));
const measurementsTemplate = document.querySelector<HTMLTemplateElement>("#cf-stage-measurements");
const measurements = JSON.parse(measurementsTemplate?.content.textContent ?? "[]") as StageMeasurement[];
let generation = 0;
const styleStages = async (): Promise<void> => {
  const current = ++generation;
  delete document.documentElement.dataset.cfReady;
  await Promise.all(stages.map(async (stage) => {
    const fresh = stage.frame.cloneNode(false) as HTMLIFrameElement;
    fresh.setAttribute("scrolling", "no");
    fresh.srcdoc = stageSource(stage.source, document.documentElement);
    const loaded = frameLoad(fresh);
    stage.frame.replaceWith(fresh);
    stage.frame = fresh;
    sizeStage(fresh, measurements);
    await loaded;
    sizeStage(fresh, measurements);
  }));
  await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  if (current === generation) document.documentElement.dataset.cfReady = "true";
};
new ResizeObserver(() => {
  for (const stage of stages) sizeStage(stage.frame, measurements);
}).observe(documentRoot);
const requestedMode = document.documentElement.dataset.cfMode ?? "system";
if (requestedMode === "system") {
  const preference = matchMedia("(prefers-color-scheme: dark)");
  const applyPreference = (): void => {
    document.documentElement.dataset.cfModeResolved = preference.matches ? "dark" : "light";
    void styleStages();
  };
  applyPreference();
  preference.addEventListener("change", applyPreference);
} else {
  document.documentElement.dataset.cfModeResolved = requestedMode === "dark" ? "dark" : "light";
  void styleStages();
}
enhanceDocument(documentRoot, true);
