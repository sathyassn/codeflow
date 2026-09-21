// The Markdown shapes the composition fixtures are built from: one page that
// carries its class, and the ways a page falls short of it. Both composition
// suites author their sources from here so the compliant and noncompliant
// shapes cannot drift apart.

// The carriers the panels are held to, each one a shape the adapter actually
// renders: a stage figure, and a table.
export const STAGE_FIGURE = [
  "```cf-stage",
  "first | what the reader is looking at @accent",
  "->",
  "then | what it becomes",
  "caption: the shape this page explains",
  "```",
].join("\n");
export const ARCHITECTURE_TABLE = ["| Part | Role |", "|---|---|", "| First | What it holds together |"].join("\n");
export const TECHNICAL_TABLE = ["| Command | Effect |", "|---|---|", "| `run` | Does the thing |"].join("\n");

export const COMPOSED_PAGE = [
  "# Composed page", "",
  "## Concept", "",
  "One governing claim, then the figure that carries it.", "",
  STAGE_FIGURE, "",
  "## Architecture", "",
  "The structure behind the claim.", "",
  ARCHITECTURE_TABLE, "",
  "## Technical", "",
  "The controls, then the contract that authenticates them.", "",
  TECHNICAL_TABLE, "",
].join("\n");

// One panel left as prose, per panel: the carrier its altitude calls for is
// gone and the rest of the page is untouched.
export const PROSE_CONCEPT_PAGE = COMPOSED_PAGE.replace(`${STAGE_FIGURE}\n\n`, "");
export const PROSE_ARCHITECTURE_PAGE = COMPOSED_PAGE.replace(`${ARCHITECTURE_TABLE}\n\n`, "");
export const PROSE_TECHNICAL_PAGE = COMPOSED_PAGE.replace(`${TECHNICAL_TABLE}\n`, "");

// The carrier the Concept panel needs, rendered one panel too late: the page
// carries a stage, and not where the altitude calls for one.
export const WRONG_PANEL_CARRIER_PAGE = PROSE_CONCEPT_PAGE.replace(`${ARCHITECTURE_TABLE}\n\n`, `${STAGE_FIGURE}\n\n`);

export const MISSING_TECHNICAL_PAGE = COMPOSED_PAGE.slice(0, COMPOSED_PAGE.indexOf("## Technical"));
export const MISSING_CONCEPT_PAGE = ["# Composed page", "", COMPOSED_PAGE.slice(COMPOSED_PAGE.indexOf("## Architecture"))].join("\n");

// The failure the gate exists to catch: a source re-rendered under the shell.
export const SHELL_PAGE = "# Plain page\n\nA paragraph re-rendered under the docs shell, and nothing else.\n";
