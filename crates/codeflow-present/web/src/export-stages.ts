import archivo from "./fonts/archivo-latin-wght-normal.woff2";
import inter from "./fonts/inter-latin-wght-normal.woff2";
import plex from "./fonts/ibm-plex-sans-latin-wght-normal.woff2";

export interface StageMeasurement {
  stage_id: string;
  revision: number;
  content_width: number;
  contract: string;
  height: number;
  theme: string;
  mode: string;
}

export const FRAME_WAIT_MS = 5_000;

/** The identical document is used in the live measuring host and export. */
export function stageSource(source: string, root: HTMLElement): string {
  const computed = root.ownerDocument.defaultView!.getComputedStyle(root);
  const carried = root.ownerDocument.createElement("div").style;
  for (const name of computed) {
    if (name.startsWith("--cf-")) carried.setProperty(name, computed.getPropertyValue(name));
  }
  for (const name of ["color", "color-scheme", "font-family", "font-size", "line-height", "font-feature-settings", "font-synthesis", "text-rendering"]) {
    carried.setProperty(name, computed.getPropertyValue(name));
  }
  const css = carried.cssText.replaceAll("<", "\\3c ");
  const fonts = [["Archivo", archivo, "100 900"], ["Inter", inter, "100 900"], ["IBM Plex Sans", plex, "100 700"]]
    .map(([family, url, weight]) => `@font-face{font-family:"${family}";src:url("${url}");font-weight:${weight};font-style:normal}`).join("");
  return `<!doctype html><meta charset="utf-8"><style>${fonts}:root{${css}}</style>${source}`;
}

/** Install listeners before attachment: srcdoc may load in the same turn. */
export function frameLoad(frame: HTMLIFrameElement, timeout = FRAME_WAIT_MS): Promise<boolean> {
  return new Promise((resolve) => {
    const finish = (loaded: boolean): void => {
      clearTimeout(timer);
      frame.removeEventListener("load", loadedEvent);
      frame.removeEventListener("error", failedEvent);
      frame.dataset.cfStageStatus = loaded ? "loaded" : "failed";
      resolve(loaded);
    };
    const loadedEvent = (): void => finish(true);
    const failedEvent = (): void => finish(false);
    const timer = setTimeout(failedEvent, timeout);
    frame.addEventListener("load", loadedEvent, { once: true });
    frame.addEventListener("error", failedEvent, { once: true });
  });
}

export function sizeStage(frame: HTMLIFrameElement, measurements: StageMeasurement[]): void {
  const id = frame.closest<HTMLElement>("[data-cf-block-id]")?.dataset.cfBlockId;
  const root = document.documentElement;
  const width = frame.getBoundingClientRect().width;
  const match = measurements.find((item) => item.stage_id === id && item.content_width === width
    && item.theme === root.dataset.cfTheme && item.mode === root.dataset.cfModeResolved);
  frame.style.setProperty("--cf-export-stage-height", match ? `${Math.max(24, match.height)}px` : "28rem");
  const notice = frame.parentElement?.querySelector<HTMLElement>("[data-cf-stage-notice]");
  if (notice) {
    const failed = frame.dataset.cfStageStatus === "failed";
    notice.hidden = !!match && !failed;
    notice.textContent = failed ? "Stage did not load; reopen this capture or open the session."
      : "height not measured at this width; open the session";
  }
}
