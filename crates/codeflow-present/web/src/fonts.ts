import archivo from "./fonts/archivo-latin-wght-normal.woff2";
import inter from "./fonts/inter-latin-wght-normal.woff2";
import plex from "./fonts/ibm-plex-sans-latin-wght-normal.woff2";
import archivoLicense from "./fonts/Archivo.LICENSE";
import interLicense from "./fonts/Inter.LICENSE";
import plexLicense from "./fonts/IBM-Plex-Sans.LICENSE";

/** Locally bundled faces: no network dependency and no authored CSS authority. */
export function installFonts(): void {
  if (document.querySelector("body > footer[data-cf-font-licenses]")) return;
  const faces = [
    ["Archivo", archivo, "100 900", archivoLicense],
    ["Inter", inter, "100 900", interLicense],
    ["IBM Plex Sans", plex, "100 700", plexLicense],
  ] as const;
  for (const [family, source, weight] of faces) {
    const face = new FontFace(family, `url("${source}")`, {
      style: "normal", weight, display: "swap",
    });
    document.fonts.add(face);
  }

  // The font-bearing renderer travels inside offline exports and the binary.
  // Keep its complete notices reachable there, outside authored/annotatable UI.
  const footer = document.createElement("footer");
  footer.dataset.cfFontLicenses = "true";
  footer.style.cssText = "max-inline-size:68rem;margin:1rem auto;padding:1rem;color:var(--cf-text-muted);font-size:var(--cf-fs-caption,0.8125rem)";
  const details = document.createElement("details");
  const summary = document.createElement("summary");
  summary.textContent = "Font licenses";
  const notices = document.createElement("pre");
  notices.style.whiteSpace = "pre-wrap";
  notices.style.overflowWrap = "anywhere";
  notices.textContent = faces.map(([family, , , license]) => `${family}\n\n${license}`).join("\n\n");
  details.append(summary, notices);
  footer.append(details);
  document.body.append(footer);
}
