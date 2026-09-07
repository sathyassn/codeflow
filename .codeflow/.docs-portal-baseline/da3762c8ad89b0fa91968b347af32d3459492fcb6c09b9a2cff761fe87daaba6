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
      customCss: ["./src/styles/utility-tokens.css", "./src/styles/portal.css", "./.portal/generated/project-tokens.css"],
      components: {
        ThemeSelect: "./src/components/PortalDisplay.astro",
      },
      head: [
        {
          // Pre-paint display state: the configured theme is the fallback
          // (signal -> instrument skin, folio -> ink + plex), stored Display
          // choices win. Inline so there is no wrong-skin flash.
          tag: "script",
          content:
            `(function(){var t=${JSON.stringify(config.theme)},d=document.documentElement,r=function(k,a){try{var v=localStorage.getItem(k);return a.indexOf(v)>=0?v:null}catch(e){return null}};` +
            `d.dataset.portalTheme=t;` +
            `d.dataset.cfpSkin=r("cf-portal-skin",["instrument","editorial","ink"])||(t==="folio"?"ink":"instrument");` +
            `d.dataset.cfpTypeface=r("cf-portal-typeface",["instrument","editorial","plex"])||(t==="folio"?"plex":"instrument");` +
            `d.dataset.cfpScale=r("cf-portal-scale",["compact","default","large"])||"default";})();`,
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
