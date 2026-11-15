# Tooling tests and framework choice: research for TSK-1031

This note is the research TSK-1031 AC-1 and AC-6 require before any
instruction changes. It records how project tooling is tested and measured
across the stacks CodeFlow serves, how projects choose a framework in
proportion, what CodeFlow's shipped guidance says today, and the scope the
task chose. It was written at the EPC-022 line tip `5405cdb1b` on
2026-11-15. Every external source below resolved when the note was written.

## Part 1: project tooling is code (AC-1)

### What counts as tooling

Tooling here means code a project writes and runs to build, check, generate,
release or operate itself, as opposed to the product it ships to users:

- scripts under `scripts/`, `tools/`, `bin/` or a `tools` package in a
  monorepo;
- build helpers: Rust `build.rs`, Makefiles, task runners (`just`, npm
  scripts, `invoke`, `mage`), and the scripts they call;
- code generators (`go generate` targets, schema or client generators) and
  the templates they read;
- release and CI helpers that a workflow calls;
- hook scripts (git hooks, harness hooks).

### How each stack tests and measures it

| Stack | Typical tooling | How it is tested | How coverage includes or excludes it |
|---|---|---|---|
| Python | `scripts/*.py`, a `tools/` package, release helpers | pytest or `unittest` against the script's functions, `tmp_path` fixtures for file effects | coverage.py measures only executed files unless `source` names the tree; with `source` set, an unexecuted file is reported at 0% ([coverage.py: specifying source files](https://coverage.readthedocs.io/en/latest/source.html)) |
| TypeScript and JavaScript | `scripts/*.mjs`, npm `scripts` entries, build plugins | Vitest, Jest or `node --test` | Istanbul-based tools (nyc, Vitest, Jest) report files tests load; an `include` or `collectCoverageFrom` list makes untouched files show as uncovered ([nyc](https://github.com/istanbuljs/nyc), [Vitest coverage](https://vitest.dev/guide/coverage), [Jest configuration](https://jestjs.io/docs/configuration)); npm lifecycle scripts are shell strings and are not measured ([npm scripts](https://docs.npmjs.com/cli/v10/using-npm/scripts)) |
| Go | `tools/` commands, `go generate` generators | `go test` in the tool's package; golden files compared with an `-update` flag for generators | `go test -cover` is per package; `-coverpkg` widens it, and Go 1.20+ can build a whole binary with coverage for integration tests ([Go: coverage for integration tests](https://go.dev/doc/build-cover), [the cover story](https://go.dev/blog/cover), [go generate](https://pkg.go.dev/cmd/go#hdr-Generate_Go_files_by_processing_source)) |
| Rust | `build.rs`, `xtask` crates, helper binaries | unit tests in the `xtask` crate; `build.rs` logic moved into a tested library function, since a build script runs at build time, not under `cargo test` ([Cargo build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html)) | cargo-llvm-cov measures workspace crates, so an `xtask` crate counts when it is a member ([cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov)); snapshot tests such as insta serve as golden files ([insta](https://insta.rs/docs/)) |
| Shell | CI helpers, hook scripts, install scripts | bats-core or ShellSpec run the script against fixtures and assert on output, exit status and files ([bats-core](https://github.com/bats-core/bats-core), [ShellSpec](https://github.com/shellspec/shellspec)); ShellCheck as lint ([ShellCheck](https://www.shellcheck.net/)) | line coverage exists through kcov or bashcov ([kcov](https://github.com/SimonKagstrom/kcov), [bashcov](https://github.com/infertux/bashcov)) but depends on the interpreter and platform, so most projects do not gate on it |
| OpenTofu and Terraform | modules and the helper scripts around them | `tofu test` / `terraform test` with run blocks and assertions ([OpenTofu test](https://opentofu.org/docs/cli/commands/test/), [Terraform tests](https://developer.hashicorp.com/terraform/language/tests)) | no line coverage; coverage means each module has tests |
| Rego (OPA) | policy bundles and their loaders | `opa test` with `_test.rego` files ([OPA policy testing](https://www.openpolicyagent.org/docs/policy-testing)) | `opa test --coverage` reports per-rule line coverage |

Cross-cutting findings:

- **Unmeasured is not the same as covered.** Every coverage tool above
  reports only what it is pointed at or what tests load. A script no test
  imports drops out of the report, so the aggregate looks healthy while the
  script has no test at all. Naming the tooling tree as a coverage source
  is what makes the gap visible.
- **Monorepos give tooling its own package.** A `tools/` or `scripts/`
  package with its own test target is the common shape in Nx, Turborepo,
  Go multi-module and Cargo workspace repositories. CodeFlow's own
  `monorepo-multi-target.json` template already carries a `tools-python`
  target with `cwd: tools` (`assets/base/testing/templates/monorepo-multi-target.json`).
- **Generators are tested through their output.** Golden-file or snapshot
  tests compare generated output against a checked-in expectation, with an
  explicit update step; the generated output itself stays excludable.
- **Build scripts move logic into testable units.** `build.rs`, Makefile
  recipes and task-runner entries stay thin and call a function or script
  that has tests.
- **Coverage guidance treats all executed code alike.** Google's published
  practice applies coverage to the code a team owns and changes, and warns
  that a percentage proves no test quality
  ([Code coverage best practices](https://testing.googleblog.com/2020/08/code-coverage-best-practices.html),
  [Software Engineering at Google, ch. 11](https://abseil.io/resources/swe-book/html/ch11.html)).
  Nothing in that practice exempts tooling.
- **Shell needs a different proof.** Where shell line coverage is not
  dependable, the accepted practice is behaviour tests that run the script
  against fixtures, a linter, and keeping branching logic small or moving it
  into a language that can be measured.

### What CodeFlow ships today (line tip `5405cdb1b`)

- `assets/base/agents/skills/cf-model-orchestrator/resources/quality/coverage.md:19-26`:
  the 80% floor applies to "aggregate production-code coverage"; generated,
  vendor, fixture and test code may be excluded with owner, reason and
  removal condition. Tooling is not mentioned, so an agent can read it as
  outside "production code".
- `assets/base/rules/workflow-discipline.md`, "Prove it at every surface":
  "Tests ship in the same change", with no word on tooling.
- `assets/base/agents/skills/cf-stack/SKILL.md:18-27`: one target per
  package; no tooling target and no word on languages a coverage tool
  cannot measure.
- `assets/base/agents/skills/cf-develop/SKILL.md:32-34`: the build step asks
  for tests through named interfaces, with no word on tooling.
- `assets/base/claude/agents/cf-reviewer.md:77`: "coverage against the
  project's configured floor, as measured", with no scope statement.
- `assets/base/testing/templates/`: nine templates. The monorepo template
  has a `tools-python` target; the single-stack templates have one target
  for the product suite.

### This repository's own tooling

`.codeflow/test-config.json` runs the `scripts/test_*.py` files in the
`gate-parity` and `release-automation` targets with no coverage block.
`scripts/changelog_split.py`, `scripts/gate-binary.py` and
`scripts/coverage-shards.py` have no test file. The Python scripts under
`assets/base/agents/skills/cf-evaluate-model/scripts/` and
`assets/base/agents/skills/cf-herdr/scripts/deliver.py` have tests in
`evals/` or beside them, but no coverage target. **Routed:** TSK-1119
("Every script CodeFlow ships or runs has tests and counts toward the
coverage floor") owns this inventory, the missing tests and the targets; it
depends on this task.

### Chosen scope for tooling

- **The rule** goes in two homes: `quality/coverage.md` (the floor and its
  exclusion list) and the discipline rules' "Prove it at every surface"
  (one sentence, self-contained, since the minimal tier installs the rules
  without the orchestrator skill). Tooling is code: it ships with tests,
  runs in a test target and counts toward the floor where its language can
  be measured. The excludable list stays generated, vendored, fixture and
  test code, under the existing owner, reason and removal condition.
- **`cf-stack`** gains a step that wires a tooling test target when the
  project has tooling, points the coverage tool at the tooling source, and
  says how to prove a language the coverage tool cannot measure (behaviour
  tests in the target, a linter, thin logic, and a recorded measurement
  gap).
- **`cf-develop`** names tooling in the build step so a builder adds its
  tests in the same change.
- **`cf-reviewer`** treats a tooling change with no test as it treats
  untested product code, in its coverage check.
- **Templates:** the monorepo template already wires `tools/`; its
  description now says so. The single-stack templates keep one target: a
  tooling target there would fail on every project without that folder,
  and `cf-stack` adds one when the project has tooling.
- **Out of scope:** the enforced floor for tooling targets (TSK-1006), the
  `defaults.coverage` fix (TSK-1014) and this repository's scripts
  (TSK-1119).

## Part 2: the standard framework, in proportion (AC-6)

### Current practice by project type

| Project type | What the ecosystem treats as standard | When plain stays better |
|---|---|---|
| Web application | A component framework or meta-framework: React (the React team recommends starting with a framework: [react.dev](https://react.dev/learn/creating-a-react-app)), Vue ([guide](https://vuejs.org/guide/introduction.html)), Svelte and SvelteKit ([docs](https://svelte.dev/docs/kit/introduction)), Angular ([overview](https://angular.dev/overview)), Solid ([site](https://www.solidjs.com/)), Next ([docs](https://nextjs.org/docs)), or a server-rendered stack (Rails, Django, Laravel, htmx over server templates: [htmx](https://htmx.org/)) | Rarely; even a small app with routing and shared state rebuilds framework parts by hand |
| Static site and documentation | A static-site or docs generator: Astro with islands for the few interactive parts ([Astro islands](https://docs.astro.build/en/concepts/islands/)), Starlight, Docusaurus, MkDocs, Hugo | A single page with no navigation and no interaction |
| Dashboard or internal tool | The organisation's existing app framework, or a lightweight one such as Preact ([Preact](https://preactjs.com/guide/v10/getting-started/)) | A one-page read-only report |
| Desktop and mobile | The platform's toolkit (SwiftUI, Jetpack Compose) or a cross-platform framework already adopted (Flutter, React Native, Tauri, Electron) | Not applicable: there is no plain equivalent |
| CLI with a UI | The CLI's language framework plus a TUI library | A command that prints text |
| Browser extension | The extension's chosen UI framework for popups and options pages with state | A single static popup |
| Embeddable widget | Web components, often through Lit ([Lit](https://lit.dev/docs/), [MDN Web Components](https://developer.mozilla.org/en-US/docs/Web/API/Web_components)) | This is the plain case: web components are the standard |

Findings:

- **Rendering choice is a trade-off, not a fashion.** Static and
  server-rendered output costs the user least; client-side frameworks pay
  for routing and state with bundle size and hydration
  ([Rendering on the web](https://web.dev/articles/rendering-on-the-web)).
  Islands and server rendering let a project use a framework and still ship
  little JavaScript.
- **Plain HTML, CSS and JavaScript stay the right call** for a one-page
  tool, a static page with no interaction, an embeddable widget, or code
  that must run with no build step and no install. Web components are the
  platform's own reuse model for that case.
- **The signals that a plain page has outgrown that** recur across the
  framework guides: a second view or route, state shared between
  components or kept between visits, client-side routing, forms with
  validation, data fetched and kept in sync, a second contributor, and code
  that starts rebuilding a router, state store or templating by hand.
- **Adjacent concerns:**
  - bundle size: Preact, Solid, Svelte and islands exist to keep it small;
  - accessibility: framework component libraries carry tested accessible
    primitives, while a hand-rolled widget must meet WCAG 2.2 AA on its own
    ([WCAG 2.2](https://www.w3.org/TR/WCAG22/));
  - testing and browser tooling: component testing and Playwright
    integration are built around the frameworks;
  - typed templates: TSX, Vue's typed templates and Svelte's checker catch
    template errors at build time;
  - supply chain: every framework adds dependencies to pin, audit and
    update ([GitHub on supply-chain security](https://github.blog/security/supply-chain-security/)),
    which is why adopting one is an owner decision.

### Projects with no user interface

The same rule applies with different frameworks. A non-trivial CLI uses its
language's standard argument parser rather than hand-parsing `argv`
(clap for Rust, [Cobra](https://github.com/spf13/cobra) for Go, Click or
[Typer](https://typer.tiangolo.com/) for Python, `argparse` for a small
script). A service uses its ecosystem's usual web framework. Every project
uses its language's standard test framework
([pytest](https://docs.pytest.org/en/stable/), Go `testing`, Vitest or
Jest, Rust's built-in harness). A library has no framework to adopt beyond
its test harness. Infrastructure and data pipelines use their tool's
own structure (OpenTofu modules, the orchestrator's DAG model). So the
guidance lives in `cf-stack`, which serves every project type, and
`cf-design` carries the user-interface part.

### What CodeFlow ships today

- `cf-stack` says "Do not mandate TypeScript, a validator library, redundant
  tooling, stricter compiler migration, or a language/stack change"
  (`assets/base/agents/skills/cf-stack/SKILL.md:33-35`). It says nothing
  about which framework a new non-trivial project should start on.
- `cf-design` section 9 says implementation "follows the established
  repository architecture"; `quality/ui-design.md` asks for
  "framework-native data flow". Neither says when a surface needs a
  framework or when it may stay plain.
- The design-system kits under `cf-present` and `cf-docs-portal`
  (`resources/design-system/README.md`) are plain HTML, CSS and ES2020 with
  no build step. Nothing says why, so an agent can take them as a model for
  product code.

### CodeFlow's own bounded runtimes

| Runtime | How it is built | Why |
|---|---|---|
| `cf-present` web runtime | Preact and TypeScript, bundled into the `codeflow` binary (`crates/codeflow-present/web/package.json`) | A multi-view interactive surface with state; Preact keeps the embedded bundle small |
| Docs portal | Astro Starlight (`docs-portal/package.json`) | The standard docs generator for a static, multi-page site |
| Design-system kits | Plain HTML, CSS and ES2020, no build step | They ship inside the binary and the scaffold, must open from a folder with no install or network, and are a reference that products in different stacks check themselves against |

Each is built as its own job requires, so no change is recommended. The
kits need one added statement: they are references for those two utility
surfaces, not a model for product code.

### Chosen scope for framework choice

- **`cf-stack`** gains a step: a non-trivial project uses its type's
  standard framework (or the one it already uses), with the criteria that
  make a project non-trivial, the proportionality rule for one-page tools,
  trivial scripts and static pages, the signals that they have outgrown
  plain code, and owner approval for a framework the project does not yet
  use. It covers projects with no user interface through the CLI, service
  and test framework examples.
- **`cf-design`** section 9 states the user-interface part briefly and
  points to `cf-stack` for the full rule.
- **`quality/ui-design.md`** adds one check, so a reviewer of a UI change
  sees hand-rolled routing or state beside an available framework.
- **Design-system READMEs** state why the kits and runtimes are built as
  they are and that the kits are not a model for product code.
- **Qualification:** native cases for a multi-view surface on the project's
  framework, a one-page tool that stays plain, and a reviewer finding on an
  untested tooling change.
