// The site's Markdown step for as-is regions (illustrated and pass-through
// sources). The generated page carries the source bytes unchanged between two
// marker comments, so `validate --portal` can prove them against the committed
// source. This step renders those bytes the way a composed page renders them:
// raw HTML becomes text, comments drop, and each link points at the
// destination the adapter resolved. When the adapter found a level-one
// heading left in the region, every heading renders one level lower, so the
// page title stays the only h1. Companion figures sit between their own
// markers; they are generated markup and pass through untouched.
//
// Astro renders Markdown with Sätteri, whose plugins visit nodes in document
// order, so the markers switch the region on and off as the walk passes them.
import { readFileSync } from "node:fs";

const BEGIN = /^<!-- codeflow-source-begin route=(\S+) /;
const BLOCK_PARENTS = new Set(["root", "listItem", "blockquote", "footnoteDefinition", "containerDirective"]);
// The link-table key that marks a region whose headings render one level
// lower. Link keys all start with link:, image: or reference:.
export const DEMOTE_HEADINGS = "headings:demote";

export function asIsMarkdownIntegration({ linksPath }) {
  return {
    name: "codeflow-as-is-markdown",
    hooks: {
      "astro:config:setup": ({ config }) => {
        const processor = config.markdown?.processor;
        if (processor?.name !== "satteri" || !Array.isArray(processor.options?.mdastPlugins)) {
          throw new Error(`codeflow-as-is-markdown: the portal renders as-is sources with the Sätteri processor, not ${processor?.name ?? "none"}`);
        }
        processor.options.mdastPlugins.push(asIsMarkdownPlugin(linksPath));
      },
    },
  };
}

export function asIsMarkdownPlugin(linksPath) {
  return () => {
    let links = null;
    let inRegion = false;
    let inCompanion = false;
    const active = () => inRegion && !inCompanion;
    const target = (kind, url) => links?.[`${kind}:${url}`];
    const sourceReference = (node, ctx, sourcePath) => ({ rawHtml: `<span class="portal-source-reference">${escapeHtml(ctx.textContent(node))} (<code>${escapeHtml(sourcePath)}</code>)</span>` });
    return {
      name: "codeflow-as-is",
      html(node, ctx) {
        const value = node.value.trim();
        const topLevel = ctx.parent(node)?.type === "root";
        if (topLevel && !inRegion) {
          const begin = value.match(BEGIN);
          if (begin !== null) {
            const table = JSON.parse(readFileSync(linksPath, "utf8"));
            if (!Object.hasOwn(table, begin[1])) throw new Error(`codeflow-as-is-markdown: no resolved links for ${begin[1]}`);
            links = table[begin[1]];
            inRegion = true;
          }
          return;
        }
        if (topLevel && value.startsWith("<!-- codeflow-source-end")) { inRegion = false; return; }
        if (topLevel && value.startsWith("<!-- codeflow-companion-begin")) { inCompanion = true; return; }
        if (topLevel && value.startsWith("<!-- codeflow-companion-end")) { inCompanion = false; return; }
        if (!active()) return;
        if (/^<!--[\s\S]*-->$/.test(value)) { ctx.removeNode(node); return; }
        const text = { type: "text", value: node.value };
        ctx.replaceNode(node, BLOCK_PARENTS.has(ctx.parent(node)?.type) ? { type: "paragraph", children: [text] } : text);
      },
      heading(node, ctx) {
        if (!active() || links?.[DEMOTE_HEADINGS] !== true) return;
        ctx.setProperty(node, "depth", Math.min(node.depth + 1, 6));
      },
      link(node, ctx) {
        if (!active()) return;
        const resolved = target("link", node.url);
        if (resolved?.code !== undefined) ctx.replaceNode(node, sourceReference(node, ctx, resolved.code));
        else if (resolved?.url !== undefined) ctx.setProperty(node, "url", resolved.url);
      },
      image(node, ctx) {
        if (!active()) return;
        const resolved = target("image", node.url);
        if (resolved?.url !== undefined) ctx.setProperty(node, "url", resolved.url);
      },
      definition(node, ctx) {
        if (!active()) return;
        const resolved = target("link", node.url) ?? target("image", node.url);
        if (resolved?.url !== undefined) ctx.setProperty(node, "url", resolved.url);
      },
      linkReference(node, ctx) {
        if (!active()) return;
        const resolved = links?.[`reference:${node.identifier}`];
        if (resolved?.code !== undefined) ctx.replaceNode(node, sourceReference(node, ctx, resolved.code));
      },
    };
  };
}

function escapeHtml(value) {
  return String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}
