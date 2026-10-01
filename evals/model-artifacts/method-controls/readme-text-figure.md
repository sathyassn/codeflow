# Ledger jobs

Ledger jobs runs scheduled accounting jobs against the ledger database.

## Usage

`lj run <job>` runs one job now.

## Architecture

The CLI reaches only the API; the runner is the one part that touches the ledger and the object store.

```text
[lj CLI] --> [API] --job requests--> [queue] --jobs--> [runner] --artifacts--> [object store]
               ^                                           |
               |                                           | results
               +------ reads results ---- [Postgres] <-----+
                                          (ledger)

--> data moves this way
```
Only the runner writes to Postgres and the object store; the CLI never reaches either.
