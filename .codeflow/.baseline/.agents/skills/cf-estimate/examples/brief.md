# Fictional read-contract package

Add one documented read-only view over an existing repository abstraction.
The input format, owner, expected output and delivery boundary are settled.
Existing engine/store integration is consumed, not rebuilt or counted twice.

Acceptance includes correct normal output, empty data, invalid input, permission
failure and a simulated unavailable store. Preserve existing error conventions;
do not expose source secrets. Independent review must assess scope, clarity,
idiomatic structure and meaningful edge-case tests, not merely a green suite.
Run the real affected read path before accepting the package. No new security
boundary, production deployment or external side effect is authorized here.

Rubric anchors: S2 = 1 for one bounded behavior; all other S drivers = 0.
No additional constraint, coupling, test-surface or non-functional obligation
raises C/X/T/R for this example. Consumed integration still has nonzero delivery
time. Total 1 gives Easy under the candidate rubric, not a universal duration.
