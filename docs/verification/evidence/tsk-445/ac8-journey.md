# Install and upgrade journey for the anchored presentation skill

Both journeys passed on 2026-11-03: a fresh standard install and a project
updated from 3.1.1 carry the new skill text and example, every relative
Markdown link in the skill resolves, and the example passes `present check`
with no faults or warnings.

| Input | Value |
|---|---|
| Candidate | `codeflow 4.0.0`, source `9e0ba03e266842b65290518ad1a16c5d9a9b2e08`, `dirty=false`, SHA-256 `2af8ab99879e4ef33d12f369e1c43e5b4016261b7ea2d14e36201d1997ffa4ba` |
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
