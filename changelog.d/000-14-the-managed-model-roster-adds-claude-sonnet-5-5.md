### Changed

<!-- codeflow:release-impact minor -->
- **The managed model roster adds Claude Sonnet 5.5 and adopts GPT-6.1 Sol.**
  The primary seats do not change: the Claude primary seat is Opus 5.5, then
  Fable 5.1, and the Codex primary seat is GPT-6 Astra, then the Sol line.
  After `codeflow update`, the catalog has a `sonnet` worker line
  (`claude-sonnet-5-5`, medium and high effort) that holds no seat. Among
  workers it is tried before Opus for bounded execution, evidence
  collection and the Claude fallback in engineering implementation, and it
  never serves orchestration, planning, design or review. The Sol line
  adopts GPT-6.1 Sol (`gpt-6.1-sol`), designated for `codex-primary` on
  2026-10-02; GPT-6 Sol and GPT-5.6 Sol stay as its fallback versions.
  Orchestration, technical planning and review stay with the Claude
  primary seat, Opus first and Fable second. Design stays with Opus alone:
  Fable takes design only under a task's operator override, so without
  Opus the design duty stays open. Fable comes first for consultation and
  reasoning support.
  No new version has a qualification record yet, so `codeflow doctor
  --check model-bindings` also warns for GPT-6.1 Sol.
