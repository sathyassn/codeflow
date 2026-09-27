# Coverage

Scenario coverage comes first: happy paths, boundaries, malformed input,
timeouts, partial failure, authorization, concurrency/idempotency, recovery,
and regression cases as applicable.

Tests must be capable of failing for a material regression in the behavior they
claim to protect. Reject tautological assertions, expectations copied from the
implementation under test, a second implementation of the same production
algorithm used as its oracle, mock-only call-wiring checks with no observable
contract, weakened assertions, test-only production branches, or unreachable
code added merely to raise coverage. Fixtures and constants may be explicit
when they represent an independently stated contract or boundary. Coverage
measures exercised lines; it never proves test integrity.

Where the stack supports line coverage, aggregate production-code coverage is a
hard floor of **80%** and the normal target is **90% or higher**. New or changed
critical logic should be covered at 90% or better when measurable. Generated,
vendor, fixture, and test code may be excluded only with a recorded owner,
reason, and removal condition. Meeting a percentage never excuses a missing
risk scenario. A repository may enforce a stronger floor; CodeFlow itself uses
its configured 90% aggregate Rust gate, so 80% is not sufficient for this
repository.
