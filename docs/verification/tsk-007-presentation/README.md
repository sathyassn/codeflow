# TSK-007 presentation qualification evidence

This record binds the current macOS qualification and independent review to
implementation candidate `826715510440df53fc999e68d77429d7b49a6d55`. It also
retains earlier native Linux evidence at its actual source revision rather than
silently transferring it to the newer candidate. TSK-007 remains in progress:
native Windows, WSL2, native-launcher coverage, and an exact-candidate Claude
verdict are not complete.

## Exact-candidate deterministic qualification

On macOS arm64, `codeflow test --mode full --strict` passed all nine targets at
the exact clean candidate:

| Target | Result | Duration |
|---|---:|---:|
| Rust format | pass | 0.863 s |
| Rust workspace | pass | 81.481 s |
| Clippy with warnings denied | pass | 0.408 s |
| Rustdoc with warnings denied | pass | 1.730 s |
| Rust coverage gate | pass | 100.602 s |
| Gate parity | pass | 0.056 s |
| Model evaluation kit | pass | 1.902 s |
| Presentation qualification | pass | 158.336 s |
| Documentation portal | pass | 58.514 s |

The presentation target covered the locked renderer build and reproducibility
digest, supply-chain checks, browser structural checks, the Windows
profile-confinement guard, 93 presentation tests, both presentation CLI tests,
both greenfield/brownfield init and initialized-baseline parity tests, an
isolated exact-repeat release-binary measurement, a real headless Chromium
journey, and an injected-failure cleanup journey. The renderer digest was
`d9d37a03b53f0006b79fbd77fcf53b9fec1fc0df5a3aef17e5a4d86032e0a900`.

The isolated macOS arm64 release measurement reported a 16,518,448-byte
production binary with SHA-256
`6cc9d1679b0a46d1b6a16b8c3c46bafab70e826ff6ce5a415835de6068ea795e`.
The service delta was 1,025,264 bytes against a 1,250,000-byte limit; the
combined service/export delta was 2,098,816 bytes against a 2,600,000-byte
limit. Two exact production builds matched. The per-run target and marked
temporary root were removed.

The real-browser journey exercised authenticated loopback bootstrap,
declarative content, syntax and diagram rendering, light/dark WCAG checks,
feedback delivery and resolution, immutable revision update and reopen,
offline export, console/network observation, session clearing, and exact-owned
service, process, browser-profile, output, trace, and temporary-root teardown.
It used `--no-launch` and a directly owned Playwright browser, so it is not
native product-launcher evidence. No headed browser or operator profile was
used.

## Native Linux evidence and boundary

A plain headless Ubuntu 24.04 arm64 Lima/VZ guest ran source revision
`74feaf04ca0eea0062a4554200e7a7e774fc5b6f`. The secret-scanned source archive
had SHA-256
`33fcfd57a55a709deddddbcf66ae33bb129084932dd1d4501d8313e3745665b4`.
The guest used Linux 6.8, two CPUs, 4 GiB memory, Rust 1.94, Node 26.4.0, and
npm 11.17.0. It passed 91 platform-applicable presentation tests, both CLI
tests, the locked web install with zero npm vulnerabilities, the complete
headless-browser journey, and the injected-failure cleanup journey. Playwright
arm64 `headless_shell` ran successfully; the full desktop Chromium binary
exited with `SIGTRAP` under that VZ guest and is not claimed as passed. The
guest contained no remaining task profile or runtime roots and was stopped and
deleted.

This is native Linux arm64 evidence only. It is not Linux x86_64, WSL2, native
Windows, or native desktop-browser evidence. Because later qualification-harness
corrections produced candidate `82671551`, the Linux run is retained as bounded
diagnostic evidence and is not promoted to exact-candidate platform approval.

## Windows confinement boundary

The deterministic guard now fails closed on Windows unless the outer lane sets
an explicit externally provisioned disposable-profile marker and the current
`USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`, and the actual Windows
Local AppData Known Folder are all confined to that profile. It rejects an
ordinary unmarked operator profile, mismatched Known Folder/environment roots,
and escaping temporary roots.

That guard proves start-time confinement, not account, profile, or VM teardown.
The outer native-Windows lane must separately create the disposable boundary,
run the native journey, and prove its deletion. No native Windows or WSL2 run
was available because the installed Parallels license had expired. Cross-target
builds and this platform-independent guard are not substitutes.

## Independent review state

Two independent GPT-5.6 Sol/high reviews inspected exact candidate `82671551`
with a clean worktree and returned approval with no material findings. Focused
rechecks covered the Windows confinement positives and negatives, restricted
child boundaries, greenfield/brownfield initialized parity, real-browser and
injected-cleanup journeys, formatting, strict linting, isolated release-binary
measurement, and temporary-root cleanup.

A fresh native interactive Claude Code 2.1.220 session observed `Fable 5 with
high effort`, Claude Max, and auto mode. The schema-v2 delegate lifecycle bound
session `523349c9-070a-4f3d-a56f-68b8a3769fb5` to the exact review prompt, but
the accepted turn ended immediately with `oauth_org_not_allowed`: the
organization had disabled Claude subscription access for Claude Code. The
session, prompt, and state directory were removed. This is a blocked review
attempt, not Claude approval; no API-key billing or fallback model was used.

## Remaining release evidence

TSK-007 cannot close until the remaining acceptance evidence is recorded on an
integration-ready exact candidate:

- native product-launcher, state, permission, browser, export, and cleanup
  journeys for macOS, Linux, native Windows, and WSL2, with each platform kept
  distinct;
- the outer native-Windows disposable profile/account or VM teardown proof;
- exact-candidate native Claude judgment and the required fresh cross-lineage
  semantic/discovery evidence; and
- the task's final uninstall-style scaffold/runtime cleanup canary and
  integrated closeout review.

No cross-build, static guard, earlier-SHA result, test pass, or same-lineage
agreement is relabelled as that missing evidence.
