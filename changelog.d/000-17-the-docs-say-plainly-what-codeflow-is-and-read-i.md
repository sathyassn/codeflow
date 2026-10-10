### Changed

<!-- codeflow:release-impact patch -->
- **The docs say plainly what CodeFlow is, and read in one order.**
  `docs/product.md` now states what CodeFlow is, the problem it solves, who
  it is for, what it does and what it is not, with a figure of the four
  enforcement planes. The README opens from it and adds a table of the three
  init tiers: what each installs and when to pick it. The docs are listed
  in the order understand, start, use, configure, reference and maintain,
  in the README and in the portal navigation, whose routes follow the new
  groups (`understand/`, `start/`, `use/`, `configure/`, `reference/`,
  `maintain/`). The other guides were swept for mannered prose and long
  blocks, and gained text flows where a flow carries the point. No command,
  flag, policy key or behavior changes. Links from the decision and task
  records to `docs/verification/` now point at the copy on `main`, because
  the source archive leaves that folder out. A workflow, `portal-pages.yml`,
  builds the portal and deploys it to GitHub Pages once the repository owner
  enables Pages; the portal `base` is now `/codeflow/`.
