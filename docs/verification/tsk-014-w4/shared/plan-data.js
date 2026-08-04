// The P1 subject payload, lifted byte-for-byte out of the W3 candidate page so
// every W4 artefact that uses it uses one copy. Frontmatter-derived; verify.mjs
// checks each record against project-management/tasks/*.md.
window.planData = {
  "tasks": [
    { "id": "TSK-005", "title": "strengthen editorial and product design contracts", "status": "complete", "deps": [], "specs": ["SPC-003"], "target": "main" },
    { "id": "TSK-006", "title": "research and prototype the presentation experience", "status": "complete", "deps": ["TSK-005"], "specs": ["SPC-004"], "target": "integration/EPC-005-presentation-system" },
    { "id": "TSK-007", "title": "verify and qualify the presentation utility", "status": "blocked", "deps": ["TSK-011"], "specs": ["SPC-004"], "target": "integration/EPC-005-presentation-system" },
    { "id": "TSK-008", "title": "research and prototype documentation portals", "status": "complete", "deps": ["TSK-005"], "specs": ["SPC-005"], "target": "integration/EPC-005-presentation-system" },
    { "id": "TSK-009", "title": "build and qualify the documentation portal utility", "status": "blocked", "deps": ["TSK-008", "TSK-014"], "specs": ["SPC-005"], "target": "integration/EPC-005-presentation-system" },
    { "id": "TSK-010", "title": "integrate and release the CodeFlow system", "status": "todo", "deps": ["TSK-007", "TSK-009", "TSK-013", "TSK-014"], "specs": [], "target": "integration/EPC-005-presentation-system" },
    { "id": "TSK-011", "title": "build the interactive presentation utility", "status": "blocked", "deps": ["TSK-006", "TSK-014"], "specs": ["SPC-004"], "target": "integration/EPC-005-presentation-system" },
    { "id": "TSK-012", "title": "refine design exploration, sourcing, and revision contracts", "status": "cancelled", "deps": ["TSK-005"], "specs": ["SPC-006"], "target": "integration/EPC-005-presentation-system" },
    { "id": "TSK-013", "title": "prepare the v3 release candidate", "status": "in_progress", "deps": [], "specs": [], "target": "integration/EPC-005-presentation-system" },
    { "id": "TSK-014", "title": "recover the design method and utility directions", "status": "in_progress", "deps": ["TSK-005", "TSK-006", "TSK-008"], "specs": ["SPC-004", "SPC-005", "SPC-006"], "target": "integration/EPC-005-presentation-system" }
  ]
};
