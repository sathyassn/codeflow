# Declaring capability impact

A capability is something the project does that someone relies on: an
external user, an operator or contributor, or an agent working here. The
catalog lists each one as a record under `docs/capabilities/`
(`CAP-NNN-<slug>.md`), or as a fenced entry in a legacy
`docs/capabilities.md`. Managed by `codeflow update`.

Every pull request whose range touches the catalog, a product path, a watched
contract path or a behaviour path says, in one `## Capability impact`
section, which capabilities it adds, changes or improves, or that it changes
none and why. `codeflow ci` checks the section against the catalog as policy
rule `git.pr_capability_impact`.

## What you write

Put the section after Release impact, as a `##` heading. Each line names one
capability with its verb, its id, its catalog `name` and what changed for the
people who use it:

```text
## Capability impact

- Added CAP-018 impact-check: authors learn before merge when a declaration disagrees with the catalog.
- Changed CAP-003 git-floor: the floor now refuses a pull request whose declaration disagrees with the catalog.
- Improved CAP-004 quick-gate: the quick gate runs each suite once, entry unchanged.
```

When no capability changes:

```text
## Capability impact

- None: internal refactoring of the release script; no behaviour changes.
```

| Verb | Use it when | The record |
|---|---|---|
| `Added` | the pull request creates the capability | new in this branch |
| `Changed` | its promise, status, stage, evidence or links change | its frontmatter changes in this branch |
| `Improved` | it works better for its users and its record still describes it | frontmatter unchanged; the line says `entry unchanged` |
| `None` | no capability is added, changed or improved | no record changes; the line gives a reason |

- Edit the record in the same branch as the change it describes. A prose-only
  edit to a record may be declared `Changed` or `Improved`.
- Never delete a record. Set `status: deprecated` and declare it `Changed`.
- A rename of the record's file goes with a rename of its `name`, because the
  file's slug must match the name. Declare it `Changed` under the new name.
- Text in a code block, a quote or an HTML comment is not read, so examples
  and template comments never count as declarations.

## What the check compares

The check reads git objects only, never the working tree. It takes three
commits:

- the policy authority, normally the target branch, for the level, the
  product and watched paths and whether a catalog exists;
- the merge base of the target and the head, for the catalog before the
  change;
- the head, for the catalog after it.

It compares the two catalogs by each record's stable `uid` (a legacy entry's
`id`) and gets the net delta: added, changed or unchanged. The section must
match that delta in both directions. Every added or changed capability is
declared, and every declared `Added` or `Changed` is in the delta. A range
that holds only ordinary Markdown under `docs/` or `project-management/`,
outside every product, watched and agent-facing path, needs no section.

The check proves the section exists, every id and name exists, and the
declaration agrees with the delta. It cannot prove that `None` or `Improved`
is true, or that a record's prose matches the code. The reviewer judges
those.

## What passes and what fails

Worked examples against a catalog that holds `CAP-101 catalog`:

| The pull request | The section | Outcome |
|---|---|---|
| adds `CAP-101-catalog.md` | `- Added CAP-101 catalog: readers can list their capabilities.` | passes |
| changes `status` in CAP-101's frontmatter | `- Changed CAP-101 catalog: readers see the new status.` | passes |
| changes product code; CAP-101 unchanged | `- Improved CAP-101 catalog: readers get faster listings, entry unchanged.` | passes |
| changes product code only | `- None: internal refactoring preserves the catalog contract.` | passes |
| changes CAP-101's frontmatter | `- None: no user-visible change.` | fails: `None` with a catalog delta |
| changes CAP-101's frontmatter | `- Improved CAP-101 catalog: clearer, entry unchanged.` | fails: `Improved` needs unchanged frontmatter |
| changes product code | no section | fails: assessment is required |
| deletes CAP-101's record | any | fails: deprecate a capability, never delete it |
| changes product code | `- Changed CAP-999 missing: ...` | fails: unknown id |
| changes product code | `- Improved CAP-101 catalog: faster.` | fails: `Improved` must say `entry unchanged` |

An input the check cannot read, such as a missing pull request body, an
unreadable range or a catalog that does not parse, fails. It is never a pass.

## The level

The level is the policy key `git.pr_capability_impact` in
`.codeflow/policy.json`, read from the target, so a pull request cannot
lower it for itself.

| Level | Effect |
|---|---|
| `off` | not checked |
| `warn` | a disagreement is reported and the run continues |
| `block` | a disagreement fails `codeflow ci` |

`init` and `update` write `off` at the minimal tier and `warn` at standard
and full. These are defaults: set the level your project wants, and update
keeps it. A policy without the key is not checked. The guarantee that the
catalog never changes undeclared holds only at `block`, with `codeflow ci` a
required check.

While the rule is active and the catalog has no records, a pull request that
changes product code cannot declare `None`. The pull request that adds the
first records passes when each one is declared `Added`.

Check a body before you push it: `codeflow ci --pr-body-file <body.md>`.
