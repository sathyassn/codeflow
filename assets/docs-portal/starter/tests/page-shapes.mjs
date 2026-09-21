// The Markdown shapes the composition fixtures are built from: one page that
// carries its class, and the ways a page falls short of it. Both composition
// suites author their sources from here so the compliant and noncompliant
// shapes cannot drift apart.

// The Concept panel opens with a table, a carrier the adapter actually
// renders, so the fixture proves the rule rather than a stand-in for it.
export const COMPOSED_PAGE = [
  "# Composed page", "",
  "## Concept", "",
  "One governing claim, then the figure that carries it.", "",
  "| Stage | Meaning |", "|---|---|", "| First | What the reader is looking at |", "",
  "## Architecture", "",
  "The structure behind the claim.", "",
  "## Technical", "",
  "| Command | Effect |", "|---|---|", "| `run` | Does the thing |", "",
].join("\n");

// The trio, with the Concept carrier removed: prose under a tablist.
export const PROSE_TRIO_PAGE = COMPOSED_PAGE.replace("| Stage | Meaning |\n|---|---|\n| First | What the reader is looking at |\n", "");
export const MISSING_TECHNICAL_PAGE = COMPOSED_PAGE.slice(0, COMPOSED_PAGE.indexOf("## Technical"));
export const MISSING_CONCEPT_PAGE = ["# Composed page", "", COMPOSED_PAGE.slice(COMPOSED_PAGE.indexOf("## Architecture"))].join("\n");

// The failure the gate exists to catch: a source re-rendered under the shell.
export const SHELL_PAGE = "# Plain page\n\nA paragraph re-rendered under the docs shell, and nothing else.\n";
