---
id: FB-{{NNN}}
uid: {{UID}}              # hidden record identity, written once by `new`; never edit
title: {{TITLE_YAML}}
topic: {{TOPIC}}          # one of `[feedback] topics` in .codeflow/project.toml
also: []                  # other topics, for cross-reference only
source: {{SOURCE}}        # chat | pr | review | issue
status: received          # received | placed | closed | declined | superseded; change it with `codeflow feedback status`
placed_in: []             # TSK-NNN, EPC-NNN or a repository path; `feedback status placed --in` adds one
supersedes: null          # the FB-NNN this item replaces
superseded_by: null       # the FB-NNN that replaced it; `feedback status superseded --by` writes it
confirmed_by: null        # `operator` once the operator confirms a decline
external_refs: []         # an imported entry's old id, or opaque links; never mirror external status
created: {{DATE}}
---

# FB-{{NNN}}: {{TITLE}}

## Verbatim

<!-- The operator's words, quoted exactly as given, typos kept. Never edit
     them; a correction of the understanding goes in Reading. -->

## Reading

<!-- What the agent understood, kept apart from the quote. A later
     correction is added with a dated note; the quote stays as it was. -->

## Placement

<!-- Where it goes in the plan's order and why there: the task, epic, rule
     or plan line that carries it. A standing rule's line cites this id. -->

## Closure

<!-- The evidence it was acted on, or the reason the operator confirmed for
     a decline. `codeflow feedback status` adds a line here when it closes,
     declines or supersedes the item. -->
