import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import rawConfig from "./portal.config.json" with { type: "json" };
import { validatePortalConfig } from "./scripts/lib.mjs";

const config = validatePortalConfig(rawConfig);
const base = config.base;

export default defineConfig({
  base,
  integrations: [
    starlight({
      title: config.title,
      description: config.description,
      customCss: ["./src/styles/portal.css", "./.portal/generated/project-tokens.css"],
      head: [
        {
          tag: "script",
          content: `document.documentElement.dataset.portalTheme=${JSON.stringify(config.theme)};`,
        },
        { tag: "script", attrs: { src: `${base}portal-preview.js`, defer: true } },
      ],
      sidebar: config.layers.map((layer) => ({ label: layer.label, items: [{ autogenerate: { directory: layer.id } }] })),
      favicon: `${base}favicon.svg`,
      social: [],
      lastUpdated: true,
      pagination: true,
    }),
  ],
});
