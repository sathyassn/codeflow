# Post-merge hardening work and evidence

Status: implementation and verification in progress. This record does not certify
native model behavior or visual quality before the corresponding trials finish.

Base: CodeFlow `cf0f124c544eb13a08b71483aa33a2b9f03e3b8b`; companion Agent OS
alignment starts at `919bdbae410b7225d7470ed008236a4eff9743f2`.

## Required outcomes

- Isolate authored presentation CSS from review controls without breaking semantic
  text, element, or area annotations. Exercise hostile selectors in a real browser.
- Anchor selected text to the actual occurrence, including repeated text, nested
  markup, whitespace normalization, and UTF-16 offsets; fail closed on mismatch.
- Make the advertised utility font choices real, locally available fonts in both
  the presentation runtime and documentation portal, including offline export.
- Preserve purposeful explanatory motion and reduced-motion accessibility; do not
  constrain motion to decoration or impose product-design choices on utilities.
- Exercise presentation creation, annotations, submission, retrieval, and native
  harness consumption. Distinguish portable feedback retrieval from automatic
  insertion into a chat UI; do not claim unsupported integrations.
- Build and browser-test the documentation portal and its reusable starter.
- Reconcile Agent OS permissions, transport selection, and effort guidance without
  relaxing human authority, destructive-action, or secrets boundaries.
- Align PR guidance with complete branch changes, attributable verification,
  independent review, honest missing evidence, and safe git closeout. Executable
  files under documentation paths must not bypass testing requirements.
- Verify medium-default orchestration with deliberate strongest-qualified
  same-family high/xhigh delegation for demanding architecture, technical planning,
  design, and other complex work. A direct xhigh trigger need not first fail at
  high. Preserve cross-family review and avoid wasteful routine escalation.
- Add adversarial behavioral cases for these decisions, run applicable native
  trials, and report unobserved behavior separately from deterministic contracts.
- Synchronize canonical assets, installed mirrors, baselines, and manifests; run
  proportionate full checks and independent Claude review before final PRs.

## Delivery boundary

Use isolated fix branches. No main merges, visibility changes, or public release
are part of this work. Historical evidence remains historical; new results must
identify their tested revision and limitations rather than rewriting old claims.
