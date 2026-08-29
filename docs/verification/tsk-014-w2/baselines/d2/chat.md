# D2 ordinary-chat baseline

What a competent assistant types when asked what governs SPC-004 and what backs
its central claim. Authored before either candidate.

---

`SPC-004` is the interactive presentation utility contract. It is `approved`,
created 2026-07-31, and it is not `implemented` because its consuming work,
`TSK-011`, is still `blocked` on `TSK-006` and `TSK-014`.

Its decision lineage is two ADRs accepted on 2026-08-01: `ADR-0049`, the bounded
`cf-present` runtime and renderer boundary, and `ADR-0050`, the versioned document
and session contract. A third ADR accepted the same day, `ADR-0052`, is what
governs the record now — but only partially. Its own words: "ADR-0049 and ADR-0050
remain historical decisions; this ADR supersedes only their same-tree runtime
assumption and post-create Windows DACL mechanism where those details conflict."
Everything else in those two ADRs still stands, `superseded_by` is `null` on all
three, and ADR-0052 states that it "changes no product outcome, task owner, graph,
or integration target". So SPC-004's behavioural envelope is unchanged; what moved
is where the browser profile lives and how Windows private files are created.

The central design claim is § Behavior 3: the selected default "must be modern,
elegant, aesthetically coherent, pleasing, functional, responsive, and suited to
focused explanation/review rather than resembling a generic model artifact or
dashboard template". That claim currently has no evidence behind it. `TSK-014`
exists precisely because the operator's real `cf-present` trial invalidated the
prior design acceptance, and the replacement rendered board's blind gates G1–G8
are not run — deterministic checks pass, which says the study is internally
consistent and accessible, not that any composition reads.

The evidence that does exist backs the runtime and lifecycle claims, and it is
bounded. The TSK-007 qualification record covers macOS arm64 only at the exact
candidate `826715510440df53fc999e68d77429d7b49a6d55`; Linux arm64 evidence exists
at an older revision and is explicitly retained as bounded diagnostic rather than
promoted; there is no native Windows or WSL2 run at all; and the Claude
judgment-primary verdict is blocked on `oauth_org_not_allowed`, which is an
authorization failure rather than a test failure.
