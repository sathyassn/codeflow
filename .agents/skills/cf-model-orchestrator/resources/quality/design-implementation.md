## Design and implementation quality

The smallest coherent solution, complexity mapped to a current requirement,
constraint or evidenced risk, and calibration to the operating context are
stated once in the workflow discipline rules, "Write only what earns its
keep". Reject cleverness that obscures control flow and architecture that
fights the repository's established patterns.

Before selecting a fix or a design, name the bounded impact set (callers,
consumers, inputs, effects, same-mechanism siblings, data, configuration,
tests, docs) and verify the adjacent behavior the change could disturb as well
as the changed path. If the path meets the trigger in
[performance](performance.md), read it.

Follow the language, framework, and repository idioms; keep business rules
single-sourced; use focused composable units, clear interfaces, and explicit
state and side effects. Do not hard-code supported variability, secrets, or
duplicated domain decisions; named stable invariants need not become
configuration. Unexplained hard-coding, duplicated business knowledge,
swallowed errors, or missing accepted edge/error handling is brittle
under-design and is `changes_requested`, even when the smaller diff passes;
so is an unmarked shortcut (the discipline rules' shortcut rule).

Typed code and typed boundary data are the default. New and changed code
is fully typed: every function, method and data shape has types the
project's type checker verifies. Every escape type or suppression (`any`,
`Any`, `object` or `interface{}` used to skip a type, an unchecked cast,
`# type: ignore`, `@ts-expect-error`, `# noqa`, `//nolint`, a ShellCheck
`disable`, or an equivalent) carries its reason inline, on its line or the
line above, scoped to one rule where the tool allows.

Data crossing any boundary is parsed into a typed model that also
validates it. Boundaries are files and configuration, network requests and
responses, process arguments, standard input and subprocess output,
environment variables, database rows, user input, and another module's
untyped output. A typed representation alone does not validate its input:
a plain Python dataclass, a `TypedDict`, a TypeScript `interface` or a cast
describes a shape and accepts any value at run time. Parse with the
typed-model library that is standard for the language, whether or not it
ships with the language: Pydantic models in Python, zod schemas in
TypeScript and checked JavaScript, serde types in Rust, structs decoded
with unknown fields refused plus a validator in Go, a `type` and a
`validation` block on each OpenTofu variable, and schemas for Rego `input`
and `data`. Where a JSON Schema exists, generate the types from it. Handle
and test invalid, absent and unexpected values. Keep one model per shape
and reuse it wherever that shape is read; a value already parsed into a
model is trusted and is not validated again. Proportionality decides how
many fields and checks a boundary model holds, never whether boundary data
is parsed or code is typed; add no wrapper layers or duplicate domain
models for this rule.

TypeScript is the default for new projects and new files. An existing
JavaScript codebase is type-checked in place instead of rewritten: `tsc`
with `checkJs` (or `// @ts-check` file by file) and JSDoc types on every
new or changed function and data shape. Adding the project's type checker,
linter and typed-model library is in scope for the change that needs them.
Until CodeFlow's relaxation register ships (planned for 4.2.0), a project
opts out of this default, whole or for one language or path, only by
stating the reason in the project-owned Stack standards of its
`AGENTS.md`; the default holds everywhere that statement does not name.

Both seats grade design proportionality before approval. Each actual executor
first-verifies its implementation for necessity, clarity, idiomatic structure,
maintainability, failure behavior, and security, and the responsible primary
inspects and accepts it. Material avoidable complexity is `changes_requested`,
even when tests pass. Primary responsibility and actual execution are
separate: a primary inspecting a worker return does not become its author.
Delegation, direct execution, and design authority follow
[capability-routing](../capability-routing.md).
