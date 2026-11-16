## Coverage

The scenario classes are stated once in the workflow discipline rules, "Prove
it at every surface". Derive the cases from the contract: for each decision
the change adds or alters, the input or state that would make it wrong; each
limit from both sides; each error path through its recovery rather than its
message; and for a library, parser, state machine or CLI, one failure the
contract does not state, unless a regression test already encodes it.

Tests must be capable of failing for a material regression in the behavior they
claim to protect. Reject tautological assertions, expectations copied from the
implementation under test, a second implementation of the same production
algorithm used as its oracle, mock-only call-wiring checks with no observable
contract, weakened assertions, test-only production branches, or unreachable
code added merely to raise coverage. Fixtures and constants may be explicit
when they represent an independently stated contract or boundary. Coverage
measures exercised lines; it never proves test integrity.

Where the stack supports line coverage, aggregate coverage of production code
and project tooling is a hard floor of **80%** and the normal target is **90% or higher**. New or changed
critical logic should be covered at 90% or better when measurable.

Project tooling is code under this rule: the scripts, build helpers, code
generators, release and CI helpers and hook scripts a project runs or ships
have tests that run in a test target, and count toward the floor where their
language can be measured. Point the coverage tool at the tooling source, so a
script no test loads shows as uncovered instead of dropping out of the
report. Test a generator through its output with golden-file or snapshot
tests. Where a language has no dependable line coverage, such as shell, its
tests still run in a target and exercise each path the script takes, and the
measurement gap is recorded with an owner, reason and removal condition; the
script itself is not excluded.

Generated, vendor, fixture, and test code may be excluded only with a
recorded owner, reason, and removal condition; tooling is not on that list.
Meeting a percentage never excuses a missing risk scenario. A repository may
enforce a stronger floor; CodeFlow itself uses its configured 90% aggregate
Rust gate, so 80% is not sufficient for this repository.
