### Changed

<!-- codeflow:release-impact minor -->
- **A reviewer can comment on a named part of every stage the cf-present
  skill writes, and an unnamed part says so.** The skill steered agents to
  schema 1, where a click on an arrow showed the whole stage's sentence and
  the note moved onto the stage title after the next revision. After
  `codeflow update`, the skill and both example documents write
  `schema_version: 2` with a title, a caption and named nodes and arrows.
  `codeflow present check` adds advisory warnings, each line now carrying
  `severity` and the summary a `warnings` count: `framing` for a version 1
  stage without its title or caption, `entities` for shapes a reviewer can
  click that nothing names or named parts inside `role="img"`, and one
  `version` line; `faults`, `valid` and the exit status keep their meaning,
  and `present open` and `present update` print the warnings on stderr and
  proceed. A part nothing names is labelled `Unnamed part of <stage>`, its
  note keeps its crop but no quote, and it falls back to the block after
  the stage changes. A stored note on a drawn shape whose quote is the
  stage title now shows "Shown on the block" instead of "Moved" after a
  change; one quoting a wrapper's `aria-label` still re-anchors where that
  sentence is drawn. Stage entities get their label as `aria-label` and a
  group `role="group"`, so a keyboard stop is announced by name. Stored
  bytes, schemas and service routes are unchanged.
