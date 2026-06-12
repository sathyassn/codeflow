# cf-dogfood — capabilities

<!-- WHAT layer: the registry of what the system does. The agent's index —
     consult it before building anything ("does this exist? what does it
     touch?"). Updated in the same PR that ships the work; at full tier a FEAT
     epic cannot close without its entry (codeflow validate blocks).

     One entry per user-meaningful capability. Statuses:
     planned → building → shipped → deprecated (deprecate, never delete).
     At ~15 entries, graduate to docs/capabilities/CAP-*.md with this file as
     the index.

     Entry format — a heading, a yaml block with exactly these fields, and one
     paragraph of prose:

     ## CAP-001 — <name>

     ```yaml
     id: CAP-001
     name: <name>
     area: <area from project.toml areas[]>
     status: planned          # planned | building | shipped | deprecated
     verified_by: []          # test tags proving it works
     epics: []                # EPC-### that built or changed it
     adrs: []                 # ADR ids that shaped it
     ```

     One paragraph: what the capability does, from the outside.
-->
