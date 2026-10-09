### Changed

<!-- codeflow:release-impact minor -->
- **Builders and reviewers ask whether a change fits the repository.** The
  plan challenge in `cf-model-orchestrator` now also asks for the change's
  fit to the existing code, one failure case that composes two mechanisms
  the design names, and whether a criterion can pass while the outcome is
  missed. `cf-develop` asks the builder to find the neighbour that already
  does the job before adding a helper, type, flag, check, key or module,
  and says the written rule wins over a precedent. `cf-reviewer` requires a
  finding that asks for a change to policy, hooks, CI, templates, schema or
  managed instructions to cite the rule it applies; a precedent commit is
  evidence, not the rule. The review brief contract names fit. No step, record or round is
  added. To pay for the words, the lifecycle reference and the reviewer's
  UI step point at the text they restated, and issue intake notes go in the
  PR Summary or the design note instead of an issue comment.
  `codeflow update` replaces the unmodified files.
