### Added

<!-- codeflow:release-impact minor -->
- **A project setup hook runs before the CI test gate.** The managed gates
  job ran `codeflow test --strict` on the runner's default toolchain, so a
  project that needs its own either edited the managed file or ran the gate
  twice (sathyassn/codeflow#46). The gates job and the shared script of the
  other templates now source a project-owned `.codeflow/ci-setup.sh`, when
  the change holds one, under `set -eu` after installing codeflow and just
  before `codeflow test`, so what it exports reaches the gate and a failing
  command fails the job. `codeflow update` never writes it, and `codeflow
  doctor --check ci-perimeter` says whether it exists, whether the CI file
  sources it and its first command.
