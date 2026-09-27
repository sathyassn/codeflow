# Project model overrides

If `.codeflow/model-selection.json` contains project overrides, run
`codeflow doctor --check model-bindings` and use only the effective qualified
role bindings it reports. An absent or empty file keeps the managed defaults;
an invalid or drifted active selection blocks preflight without partial
application or silent fallback. The project file may reference binding IDs
only; it never owns raw selectors, worker routes, or commands.
Harness adapters are not alternate sources of truth.
