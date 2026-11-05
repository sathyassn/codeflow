# Install and upgrade journey for the anchored presentation skill

Both journeys passed on 2026-11-04, at the review fix head: a fresh standard install and a project
updated from 3.1.1 carry the new skill text and example, every relative
Markdown link in the skill resolves, and the example passes `present check`
with no faults or warnings.

| Input | Value |
|---|---|
| Candidate | `codeflow 4.0.0`, source `c5de5a9743e1bc12cce1c0ea7f60a044106421a3`, `dirty=false`, SHA-256 `a896e6dd09a7b299a48ef5812e2f89f04a409577ed10ce72884957c96da9af54` |
| Baseline | `codeflow 3.1.1`, source `48104f48b4291ee3510a73c487eeb97e51fe9d2a`, `dirty=false`, the release asset, SHA-256 `b20efcaf432e24fccf1708850534b4c1b115ab3d6b0155adbb162279bd8f854d` |

| Check | Fresh install | Update from 3.1.1 |
|---|---|---|
| Starting point | candidate `init --standard --yes` | baseline `init --standard --yes`, scaffold version 3.1.1 confirmed, then candidate `update` |
| Scaffold version after | 4.0.0 | 4.0.0 |
| `.agents/skills/cf-present` files identical to source | 22 | 22 |
| `.claude/skills/cf-present` files identical to source | 22 | 22 |
| Relative Markdown link paths missing, both homes | 0 | 0 |
| `present check --file` on the installed example | `{"faults":0,"valid":true,"warnings":0}` | `{"faults":0,"valid":true,"warnings":0}` |

Not exercised: heading fragments and pointers in prose are not machine-checked,
and the installed example was not opened in a browser in this run.
