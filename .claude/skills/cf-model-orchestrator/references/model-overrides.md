# Project model overrides

If `.codeflow/model-selection.json` contains project overrides, run
`codeflow doctor --check model-bindings` and use only the effective qualified
role bindings it reports. An absent or empty file keeps the managed defaults;
an invalid or drifted active selection blocks preflight without partial
application or silent fallback. The project file may reference binding IDs
only; it never owns raw selectors, worker routes, or commands.
Harness adapters are not alternate sources of truth.

Schema 2 adds two optional blocks; at least one must be present, and any
invalid entry rejects the whole file:

- `design_authority` names roles inside the catalog's design owner seat:
  exactly one `owner`, which must be the seat's first line, and any later
  line of that seat as `co-owner` or `consultant`, with optional
  `approves` and `consulted_before` stage lists and a `designated` date and
  record. A co-owner designs after the owner without an override and gives
  the same-family `design-approval`; a consultant gives an advisory
  approval and never designs. Another family is rejected here and still
  needs a task-specific `OPERATOR_OVERRIDE`. The block confers authority
  only as committed on the task's integration target, read on the task's
  own branch once the task passes `codeflow work start`'s anchor rule; a
  working-tree or task-branch copy confers nothing.
- `standing_reviews` lists `{area, seat, mode, per, record}`: an extra-family
  catalog seat the review duties already route, `read-only`, per `unit` or
  `phase`. A `unit-review` or `body-review` resolved with `--area <area>`
  then owes that seat's participant as a standing assignment.

Neither block adds a selector, command, family, harness or designation,
and a same-family design approval never counts as the independent review.
