## Design and implementation quality

Approve the smallest coherent solution that fully satisfies the accepted
behavior, not the fewest lines. Every material abstraction, public interface,
configuration surface, dependency, compatibility path, and operational concept
must map to a current requirement, observed constraint, or evidenced risk; if it
does not, remove or simplify it. Reject speculative generality, duplicate or
dead paths, cleverness that obscures control flow, and architecture that fights
the repository's established patterns.

Coherence includes justified structure, not merely less structure. Follow the
language, framework, and repository idioms; keep business rules single-sourced;
use focused composable units, clear interfaces, and explicit state and side
effects. Prefer declarative or reactive composition when it is native to the
stack, not as a universal mandate. Do not hard-code supported variability,
secrets, or duplicated domain decisions; named stable invariants need not become
configuration. Current variants, repeated behavior, observed constraints, and
evidenced edge or failure cases may require abstraction, reuse, configuration,
or defensive code. Unexplained hard-coding, duplicated business knowledge,
swallowed errors, or missing accepted edge/error handling is brittle
under-design and is `changes_requested`, even when the smaller diff passes.

Use the existing stack's type system and checking tools to make domain states
and interface contracts explicit where they prevent material errors. Preserve
useful type information; justify unchecked casts, broad escape types,
suppressed checks, or equivalent bypasses at the affected boundary. Static
types do not validate external or runtime data: parse and validate untrusted
inputs at trust boundaries and handle invalid, absent, and unexpected values
explicitly. Reuse established schemas and parsers. Do not add wrapper layers,
duplicate domain models, validation everywhere, dependencies, stricter-compiler
or language migrations merely to satisfy this rule. Trusted internal values do
not need redundant runtime validation when their invariant is evidenced.

Calibrate structure to the accepted operating context: expected lifetime,
scale, rate and shape of change, contributor and integration breadth,
operational or security risk, and cost of reversal. No factor, especially size
alone, proves an abstraction. If missing context would materially change the
settled design, clarify it before approval; if clarification is unavailable,
state the assumption and prefer established safe practices with reversible
boundaries, without speculative generality.

Both seats grade design proportionality before approval. Each actual executor
first-verifies its implementation for necessity, clarity, idiomatic structure,
maintainability, failure behavior, and security; the responsible primary
inspects and accepts it, then the independent cross-lineage reviewer reviews that
unit independently. The directly invoked model qualified for the
`claude-judgment-primary` role reviews the settled design and actual integrated
diff and owns the final quality verdict; helpers may collect evidence but cannot
replace that judgment. Its integrated judgment is not independent review of a
unit it authored.
Material avoidable complexity is `changes_requested`, even when tests pass.
A fallback records reduced assurance and never claims that the selected Claude
judgment primary reviewed the work.

Primary responsibility and actual execution are separate. The recorded
executor first-verifies the unit, and a primary inspecting a worker return
does not become its author and must not secretly duplicate the
implementation. Delegation, direct execution, and design authority follow
[capability-routing](../capability-routing.md).
