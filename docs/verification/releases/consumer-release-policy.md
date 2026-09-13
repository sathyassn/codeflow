# Consumer release-policy verification

## Scope

TSK-028 adds project-owned release guidance, not a release engine installed in
every consumer. Existing tools and version domains remain authoritative. The
`release-policy` evaluation pack tests seven bounded reasoning scenarios; it
does not qualify a model binding or prove publication safety by itself.

## Native diagnostic observations

Seven fresh interactive Claude sessions completed on the first candidate
(`76772f042`). Requested route: Fable at high effort. Completion receipts and
clean fixture state were retained outside the repository. Requested model and
effort are not independently attested runtime selectors in these receipts.
Codex independently read all seven raw responses against the case criteria.

| Scenario | Observed decision |
| --- | --- |
| Shipped safety guarantee reversed under a docs label | Major impact, migration and independent review required |
| Optional compatible CLI addition | Minor; watched paths do not prove a break; landed markers must agree |
| Test-only internal refactor | No release permitted by the fixture policy; independent review still required |
| Agent OS doctrine guarantee reversed | Major bundle; independent skill/schema/evaluation versions preserved |
| Existing Changesets monorepo | Existing calculator and independent package groups retained |
| New calendar-versioned bundle | Concrete opt-in process, candidate/no-op/retry tests, separate publication approval |
| Changed candidate and conflicting public asset | Stale approval rejected; no overwrite or forced tag; checksum is not a commit |

These were advice-only fixtures with supplied change descriptions, not live
publication exercises or a full three-trial qualification. The subjects
explicitly identified missing real diffs/build evidence rather than claiming
they had verified them. No files or hosting settings were changed by subjects.

The existing-package and calendar-version trials exposed an unconditional
commit-calculator claim in the shipped PR template. `739385fa7` corrects that
claim. It also removes duplicated routing summaries without removing the
full policy obligations. Both affected scenarios completed fresh, clean
interactive trials against that candidate; Codex read both results and found
the required semantic behaviors retained with the template contradiction gone.
The earlier observations remain earlier evidence. The revised CalVer response
still proposes a process, not a tested implementation for an absent product.

## Review and distribution

Native Claude approved the initial source diff, including exact mirror parity,
consumer authority and independent version domains. The supported scaffold
update also corrected six pre-existing stale manifest hashes: cf-method and
its project-organization reference, cf-plan, and the three planning templates.
Their content was unchanged; this is a bounded metadata repair, not a new
method or work-record change. CodeFlow's separately customized CI workflow was
preserved. Native Claude also approved the factoring/template delta. The strict
quick gate passed all nine targets: workspace tests, formatting, Clippy,
Rust documentation, gate parity, evaluation kit, skill triggers, transport
tests and document validation. The evaluation-kit suite passed 52 tests;
the artifact-budget contract passed all seven tests. No hosted publication
has been exercised by this diagnostic.
