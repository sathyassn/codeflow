import { fileURLToPath } from "node:url";
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import rawConfig from "./portal.config.json" with { type: "json" };
import { validatePortalConfig } from "./scripts/lib.mjs";
import { prePaintScript } from "./scripts/runtime-scripts.mjs";
import { asIsMarkdownIntegration } from "./scripts/as-is-markdown.mjs";

const config = validatePortalConfig(rawConfig);
const base = config.base;

export default defineConfig({
  base,
  integrations: [
    // Illustrated and pass-through sources render as they are; this step
    // resolves their links and keeps their raw HTML inert (as-is-markdown.mjs).
    asIsMarkdownIntegration({ linksPath: fileURLToPath(new URL("./.portal/generated/as-is-links.json", import.meta.url)) }),
    starlight({
      title: config.title,
      description: config.description,
      customCss: ["./src/styles/utility-tokens.css", "./src/styles/portal.css", "./src/styles/figure-roles.css", "./src/styles/figure.css", "./.portal/generated/project-tokens.css"],
      components: {
        ThemeSelect: "./src/components/PortalDisplay.astro",
      },
      head: [
        {
          // Pre-paint display state: the configured theme is the fallback
          // (signal -> instrument skin, folio -> ink + plex), stored Display
          // choices win. Inline so there is no wrong-skin flash; its text
          // lives in scripts/runtime-scripts.json, where the gate reads it.
          tag: "script",
          content: prePaintScript(config.theme),
        },
        { tag: "script", attrs: { src: `${base}portal-preview.js`, defer: true } },
        { tag: "script", attrs: { src: `${base}portal-tabs.js`, defer: true } },
      ],
      sidebar: config.layers.map((layer) => ({ label: layer.label, items: [{ autogenerate: { directory: layer.id } }] })),
      favicon: "/favicon.svg",
      social: [],
      lastUpdated: true,
      pagination: true,
    }),
  ],
});
