# TSK-007 qualification state

Ordinary Markdown treatment of the same content. `baseline.html` is a faithful
hand-authored HTML rendering of this file.

## Coverage

| Claim | macOS arm64 | Linux arm64 | Windows | WSL2 |
|---|---|---|---|---|
| Nine strict test targets | pass at `82671551` | pass at `74feaf04` | not run | not run |
| Headless browser journey | pass | pass (`headless_shell`) | not run | not run |
| Desktop Chromium binary | pass | SIGTRAP — not claimed | not run | not run |
| Injected-failure cleanup | pass | pass | not run | not run |
| Profile confinement | n/a | n/a | start-time guard only | not run |
| Account/profile/VM teardown | n/a | n/a | not proved | not run |
| Native product launcher | not evidence (`--no-launch`) | not evidence | not run | not run |

## Review

- Two GPT-5.6 Sol/high reviews: approved candidate `82671551`, no material findings.
- Claude judgment-primary verdict: blocked, `oauth_org_not_allowed`.

## Provenance

- Exact candidate `826715510440df53fc999e68d77429d7b49a6d55`.
- Renderer digest `d9d37a03b53f0006b79fbd77fcf53b9fec1fc0df5a3aef17e5a4d86032e0a900`.
- Release binary 16,518,448 bytes, SHA-256 `6cc9d1679b0a46d1b6a16b8c3c46bafab70e826ff6ce5a415835de6068ea795e`.
- Linux source archive `33fcfd57a55a709deddddbcf66ae33bb129084932dd1d4501d8313e3745665b4` at revision `74feaf04ca0eea0062a4554200e7a7e774fc5b6f`.
- No native Windows or WSL2 run: Parallels licence expired.
