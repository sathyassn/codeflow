import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import config from "./portal.config.json" with { type: "json" };

if (!["signal", "folio"].includes(config.theme)) throw new Error("portal.config.json: theme must be signal or folio");

export default defineConfig({
  base: config.base,
  integrations: [
    starlight({
      title: config.title,
      description: config.description,
      customCss: ["./src/styles/portal.css"],
      head: [{
        tag: "script",
        content: `document.documentElement.dataset.portalTheme=${JSON.stringify(config.theme)};`,
      }],
      sidebar: config.layers.map((layer) => ({ label: layer.label, items: [{ autogenerate: { directory: layer.id } }] })),
      favicon: `${config.base === "/" ? "/" : config.base}favicon.svg`,
      social: [],
      lastUpdated: true,
      pagination: true,
    }),
  ],
});
