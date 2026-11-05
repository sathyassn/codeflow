import { postJson } from "./http";
import { frameLoad, stageSource, type StageMeasurement } from "./export-stages";

interface MeasurementConfig {
  revision: number;
  contract: string;
  html: string;
  widths: number[];
}

/** A separate viewport reproduces export layout, unaffected by the review rail. */
export async function measureExportStages(): Promise<void> {
  const template = document.querySelector<HTMLTemplateElement>("#cf-stage-measuring");
  if (!template) return;
  const config = JSON.parse(template.content.textContent ?? "{}") as MeasurementConfig;
  for (const width of config.widths) {
    for (const theme of ["graphite", "slate", "sage"]) {
      for (const mode of ["light", "dark"]) {
        const host = document.createElement("iframe");
        // Readable only in this trusted live host. Scripts remain forbidden;
        // the saved export never receives this permission.
        host.sandbox.add("allow-same-origin");
        host.setAttribute("scrolling", "no");
        host.setAttribute("aria-hidden", "true");
        host.inert = true;
        host.style.cssText = `position:fixed;left:-100000px;top:0;width:1200px;height:1px;border:0;visibility:hidden;pointer-events:none`;
        host.srcdoc = config.html;
        const loaded = frameLoad(host);
        document.body.append(host);
        try {
          if (!await loaded) continue;
          const page = host.contentDocument;
          if (!page) continue;
          // Expose nested stages in their ordinary column before measuring.
          for (const details of page.querySelectorAll("details")) details.open = true;
          for (const hidden of page.querySelectorAll<HTMLElement>("[hidden]")) hidden.hidden = false;
          page.documentElement.dataset.cfTheme = theme;
          page.documentElement.dataset.cfMode = mode;
          page.documentElement.dataset.cfModeResolved = mode;
          for (const original of page.querySelectorAll<HTMLIFrameElement>("iframe[data-cf-export-stage]")) {
            const stageId = original.closest<HTMLElement>("[data-cf-block-id]")?.dataset.cfBlockId;
            if (!stageId) continue;
            const frame = original.cloneNode(false) as HTMLIFrameElement;
            frame.sandbox.add("allow-same-origin");
            frame.setAttribute("scrolling", "no");
            frame.style.setProperty("--cf-export-stage-height", "1px");
            // The stage's viewport, including authored media queries, is the
            // ladder width. Export fonts and padding remain identical.
            frame.style.setProperty("inline-size", `${width}px`, "important");
            frame.style.setProperty("max-inline-size", "none", "important");
            frame.srcdoc = stageSource(original.srcdoc, page.documentElement);
            const ready = frameLoad(frame);
            original.replaceWith(frame);
            if (!await ready || !frame.contentDocument) continue;
            const content = frame.contentDocument;
            await Promise.all([...content.fonts].map((font) => font.load()));
            await content.fonts.ready;
            // With a one-pixel viewport scrollHeight reports content, not the
            // old 28rem minimum. Include the body's bottom margin.
            const height = Math.max(24, Math.ceil(content.documentElement.scrollHeight));
            const report: StageMeasurement = {
              stage_id: stageId, revision: config.revision, contract: config.contract,
              content_width: frame.getBoundingClientRect().width, height, theme, mode,
            };
            await postJson("/app/api/stages/measure", report);
          }
        } finally {
          host.remove();
        }
      }
    }
  }
  document.documentElement.dataset.cfStagesMeasured = "true";
}
