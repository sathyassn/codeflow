import { renderFigure, validateDeclaration } from "./figure-grammar.mjs";
import { readFileSync, writeFileSync } from "node:fs";
const out = {};
let n = 1;
for (const f of ["01-flow","02-structure","03-layering","04-sequence","05-state","06-coverage"]) {
  const decl = JSON.parse(readFileSync(`${f}.json`, "utf8"));
  validateDeclaration(decl);
  out[f] = renderFigure(decl, { number: n++, idPrefix: "lab" });
}
writeFileSync("rendered.json", JSON.stringify(out));
console.log(Object.entries(out).map(([k,v]) => `${k}: ${v.length} bytes, marks ${(v.match(/data-state=/g)||[]).length}`).join("\n"));
