# Qualification harness — review convergence

Ordinary repository Markdown. Retained as the second plain baseline; it is not a design.

## Rounds

| Round | Commit | Subject | Date |
|---|---|---|---|
| R1 | `86582dc0` | test(present): qualify owned runtime boundaries | 2026-08-02 |
| R2 | `74feaf04` | test(present): harden qualification cleanup | 2026-08-02 |
| R3 | `82671551` | test(present): close qualification review gaps | 2026-08-02 |
| R4 | `14107e48` | test(present): tolerate cold browser startup | 2026-08-03 |
| R5 | `c5ede869` | test(present): follow bootstrap readiness semantics | 2026-08-03 |
| R6 | `30133fd7` | test(present): bound cold renderer startup | 2026-08-03 |
| R7 | `e194b1fd` | fix(present): harden browser close recovery | 2026-08-03 |
| R8 | `a47b62c3` | fix(present): align browser qualification driver | 2026-08-03 |

## Findings

- **F1** the qualification child inherited unrelated host values — R2 opened and fixed;
  R3 re-opened it because the allow-list carried only the shouted `SYSTEMROOT` spelling.
- **F2** cleanup swallowed the primary error and its own failures — R2, closed.
- **F3** nothing proved that an injected failure still cleans up — R2 closed it; R7 and
  R8 extended what the check proves. These are extensions, not re-openings.
- **F4** Windows profile confinement was unscoped — R3, closed with a residual.
- **F5** a cold runner outran the qualification's bounded waits — R4 opened, R5 re-opened,
  R6 re-opened and closed.
- **F6** the browser context close timed out with an earlier task-owned Chrome present —
  R7 opened, R8 re-opened and closed. The cause of the R8 work was outside the harness:
  the pinned browser toolchain, Chrome 150 × Playwright 1.55.
- **F7** the POSIX process inventory failed closed on a transient overrun — R8, closed.

> The qualification had been driving Chrome 150 through Playwright 1.55, outside that
> Playwright release's tested stable-browser window
