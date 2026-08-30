# Skill descriptions (trigger, not documentation)

The YAML `description` is always loaded. It is how the agent decides whether
to open the skill. The body is loaded only after that match.

Write it in the third person. It must say **what** the skill does and **when**
to use it, using words a real request would contain. Add **do not use** when
two skills could collide. Keep "use when" in the description; a body "When to
Use" section does not substitute for the scent.

The description is a load trigger, not a procedure dump. Do not name a TTY
host, an environment variable (`HERDR_ENV`), a launch flag, or a sibling
overlay skill there. That routing lives in the body: consult and delegate
load `cf-herdr` after they have already matched. Only `cf-herdr`'s own
description should scent on Herdr.

Claude's limit is 1024 characters and no XML. Prefer a slightly pushy trigger
over a vague one: models under-trigger more often than they over-trigger.

After changing a description, add or update should-trigger / should-not-trigger
prompts under `evals/skill-triggers/`. Grade those from the description text,
not from the agent's self-report.
