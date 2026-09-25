// The Markdown shapes the composition fixtures are built from: one page that
// carries its class, and the ways a page falls short of it. Both composition
// suites author their sources from here so the compliant and noncompliant
// shapes cannot drift apart.
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

// The one carrier the Technical panel adds to its figure.
export const TECHNICAL_TABLE = ["| Command | Effect |", "|---|---|", "| `run` | Does the thing |"].join("\n");

// Every panel's figure is bound in the portal configuration (panelBindings
// below); the source itself carries the three sections and the table.
export const COMPOSED_PAGE = [
  "# Composed page", "",
  "## Concept", "",
  "One governing claim, then the figure that carries it.", "",
  "## Architecture", "",
  "The structure behind the claim.", "",
  "## Technical", "",
  "The controls, then the contract that authenticates them.", "",
  TECHNICAL_TABLE, "",
].join("\n");

// The Technical panel with its figure and no table.
export const NO_TABLE_TECHNICAL_PAGE = COMPOSED_PAGE.replace(`${TECHNICAL_TABLE}\n`, "");

export const MISSING_TECHNICAL_PAGE = COMPOSED_PAGE.slice(0, COMPOSED_PAGE.indexOf("## Technical"));
export const MISSING_CONCEPT_PAGE = ["# Composed page", "", COMPOSED_PAGE.slice(COMPOSED_PAGE.indexOf("## Architecture"))].join("\n");

// The failure the gate exists to catch: a source re-rendered under the shell.
export const SHELL_PAGE = "# Plain page\n\nA paragraph re-rendered under the docs shell, and nothing else.\n";

export const ALL_PANELS = Object.freeze(["concept", "architecture", "technical"]);

// One grammar figure per panel, each a specimen that holds all twelve rules,
// with its fact pointed at a committed fixture file.
const SPECIMENS = { concept: "03-layering.json", architecture: "02-structure.json", technical: "04-sequence.json" };
export const PANEL_DECLARATIONS = Object.freeze(Object.fromEntries(ALL_PANELS.map((panel) => [panel, `figures/${panel}.json`])));
export const FIGURE_FACTS_PATH = "figures/facts.md";
export const FIGURE_FACTS = "# Figure facts\n\n## Shape\n\nThe fixture figures draw the shape this page explains.\n";
const FIXTURE_FACT = { claim: "The fixture figures draw this page's shape", source: `${FIGURE_FACTS_PATH}#shape`, derive: "the Shape section", check: { kind: "contains", text: "draw the shape this page explains" }, value: true };

export function panelBindings(route, panels = ALL_PANELS) {
  return panels.map((panel) => ({ declaration: PANEL_DECLARATIONS[panel], route, panel }));
}

// Every specimen declaration, one per family plus a derived extent.
export async function specimens() {
  const directory = fileURLToPath(new URL("./fixtures/figures/", import.meta.url));
  const names = (await readdir(directory)).filter((name) => name.endsWith(".json")).sort();
  return Promise.all(names.map(async (name) => ({ name, declaration: await specimen(name) })));
}

export async function specimen(name) {
  return JSON.parse(await readFile(fileURLToPath(new URL(`./fixtures/figures/${name}`, import.meta.url)), "utf8"));
}

export async function writeFigureInputs(root) {
  await mkdir(path.join(root, "figures"), { recursive: true });
  await writeFile(path.join(root, FIGURE_FACTS_PATH), FIGURE_FACTS);
  for (const panel of ALL_PANELS) {
    const declaration = await specimen(SPECIMENS[panel]);
    declaration.figure.facts = [FIXTURE_FACT];
    await writeFile(path.join(root, PANEL_DECLARATIONS[panel]), `${JSON.stringify(declaration, null, 2)}\n`);
  }
}
