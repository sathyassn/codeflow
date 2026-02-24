---
name: cf-go-standards
description: Go development standards, patterns, and validation reference. Covers package structure, naming, error handling, concurrency, testing, and lint rules for .go files. Targets Go 1.24+.
---

# Go Standards Skill

## Type

**Procedural** - On-demand reference for Go development conventions and validation.

## Purpose

**Quick reference for Go package structure, naming, error handling, concurrency, testing, lint rules, and common patterns used in CodeFlow.**

## Responsibilities

- Define standard package structure, naming, and interface conventions
- Document common patterns (CLI entry, error handling, JSON, concurrency)
- Specify lint configuration and rules
- NOT: Linting execution (cf-development SOPs run `golangci-lint`)
- NOT: Test writing (cf-quality-assurance handles test implementation)

## Decision Tree

```text
Working with .go file:
├── New package?            → apply-structure
├── Editing existing?       → Check conventions
│   ├── Naming/types?       → apply-conventions
│   ├── Logic/patterns?     → apply-patterns
│   └── Goroutines/channels?→ apply-concurrency
└── Before commit?          → validate-package
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | apply-structure | ENF-L3 Advisory | Package skeleton, module layout, CLI entry points |
| 2 | apply-conventions | ENF-L3 Advisory | Naming, types, interfaces, error handling, imports |
| 3 | apply-patterns | ENF-L3 Advisory | Common patterns and anti-patterns |
| 4 | apply-concurrency | ENF-L3 Advisory | Goroutines, channels, sync, context, race safety |
| 5 | validate-package | ENF-L3 Advisory | Lint, format, vet, test coverage rules |

## Operation Details

### apply-structure

```text
When: Creating a new Go package or reviewing package skeleton
Purpose: Ensure correct module layout, CLI entry pattern, and import grouping
Enforcement: ENF-L3 Advisory

Go Module Layout:

  codeflow-cli/
  ├── cmd/codeflow/        # CLI entry points (main.go per command)
  │   └── main.go          # main() → run() → os.Exit pattern
  ├── internal/             # Private packages (not importable externally)
  │   ├── config/           # Configuration loading
  │   ├── db/               # Database operations
  │   └── session/          # Session management
  ├── pkg/                  # Public packages (importable by other modules)
  ├── testdata/             # Test fixtures (ignored by go build)
  ├── go.mod                # Module definition
  ├── go.sum                # Dependency checksums
  └── Makefile              # Build, test, lint targets

CLI Entry Point Template (main.go):

  package main

  import (
      "fmt"
      "os"
  )

  var version = "dev"

  func main() {
      if err := run(os.Args[1:]); err != nil {
          fmt.Fprintf(os.Stderr, "error: %v\n", err)
          os.Exit(1)
      }
  }

  func run(args []string) error {
      // CLI logic here — testable without os.Exit
      return nil
  }

Package doc.go Convention:

  // Package session provides session lifecycle management.
  package session

Import Grouping (separated by blank lines):

  1. Standard library (fmt, os, path/filepath)
  2. External packages (third-party modules)
  3. Internal packages (github.com/codeflow/codeflow-cli/internal/...)

Build Tags and Cross-Compilation:

  //go:build linux
  CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build ./cmd/codeflow/

Embedded Resources (go:embed):

  import "embed"

  //go:embed schema.sql
  var schemaSQL string

Required Elements:

  | Element           | Rule                                                       |
  |-------------------|------------------------------------------------------------|
  | Package comment   | doc.go with // Package <name> provides... for every package|
  | CLI entry         | main() → run() → os.Exit pattern for commands              |
  | Import grouping   | stdlib | external | internal, blank line separated          |
  | Error returns     | Functions that can fail return error as last value          |
  | Context parameter | First parameter for I/O and long-running functions         |

Procedure:
  1. Create package directory under internal/ or pkg/
  2. Add doc.go with package comment
  3. Create source files with correct import grouping
  4. For CLI commands, use main() → run() entry pattern
  5. Add _test.go files alongside source files
  6. Verify package builds: go build ./internal/<package>/

Output: Go package with correct layout, doc.go, and import grouping
```

### apply-conventions

```text
When: Writing or editing Go code — naming, types, interfaces, or error handling
Purpose: Ensure consistent naming, proper error handling, and godoc-compliant comments
Enforcement: ENF-L3 Advisory

Naming Conventions:

  | Element        | Convention                      | Example                        |
  |----------------|---------------------------------|--------------------------------|
  | Packages       | lowercase, no underscores       | session, testutil              |
  | Files          | lowercase, underscores OK       | connection_test.go             |
  | Exported funcs | PascalCase                      | OpenDatabase(), NewConfig()    |
  | Unexported     | camelCase                       | parseArgs(), dbConn            |
  | Constants (exp)| PascalCase                      | MaxRetries, DefaultTimeout     |
  | Constants (unx)| camelCase                       | defaultTimeout, maxBatchSize   |
  | Interfaces     | -er suffix for single method    | Reader, Closer, Validator      |
  | Errors         | Err prefix for sentinel errors  | ErrNotFound, ErrTimeout        |
  | Test files     | _test.go suffix                 | config_test.go                 |

Error Handling Patterns:

  Sentinel errors:
    var ErrNotFound = errors.New("not found")
    var ErrTimeout  = errors.New("operation timed out")

  Error wrapping (include context):
    fmt.Errorf("opening config %s: %w", path, err)

  Error checking:
    errors.Is(err, ErrNotFound)              // sentinel check
    errors.As(err, &target)                  // type check (pre-1.26)
    errors.AsType[*PathError](err)           // type check (Go 1.26+)

  NEVER ignore errors:
    _ = someFunc()          // BAD — error silently discarded
    if err := someFunc(); err != nil { ... } // GOOD

  Custom error types:
    type ValidationError struct {
        Field   string
        Message string
    }
    func (e *ValidationError) Error() string {
        return fmt.Sprintf("validation: %s: %s", e.Field, e.Message)
    }

  Error message style:
    - Lowercase, no trailing punctuation
    - Include context: "opening file %s: %w"
    - Chain from inner to outer: "db.Open: opening file config.db: permission denied"

Type and Interface Guidelines:

  Accept interfaces, return concrete types.
  Keep interfaces small (1-3 methods).
  Define interfaces at the consumer, not the producer.

  // Good: interface defined where it's consumed
  type Store interface {
      Get(ctx context.Context, id string) (*Task, error)
  }

  Functional options for complex constructors:
    type Option func(*Config)
    func WithTimeout(d time.Duration) Option { ... }
    func NewClient(opts ...Option) *Client { ... }

Comments and Godoc:

  Exported symbols MUST have comments starting with the symbol name:
    // OpenDatabase opens a SQLite database at the given path.
    func OpenDatabase(path string) (*DB, error) { ... }

  Package comments go in doc.go:
    // Package db provides database operations for the WorkGraph.
    package db

Procedure:
  1. Apply naming convention from table for the element type
  2. Return error as last value from functions that can fail
  3. Wrap errors with context using fmt.Errorf("context: %w", err)
  4. Add godoc comments to all exported symbols
  5. Define interfaces at the consumer, keep them small

Output: Code following consistent naming, error handling, and documentation conventions
```

### apply-patterns

```text
When: Implementing logic — CLI, file I/O, error handling, JSON, database, testing
Purpose: Use correct patterns and avoid known anti-patterns
Enforcement: ENF-L3 Advisory

Common Patterns:

  | Pattern          | Rule                                                                    |
  |------------------|-------------------------------------------------------------------------|
  | CLI entry        | main() → run(args) error → os.Exit. Separates testable logic from exit. |
  | Error handling   | Always wrap with context: fmt.Errorf("doing X: %w", err)                |
  | Resource cleanup | defer closer.Close() immediately after successful open                  |
  | Config loading   | os.OpenRoot() (Go 1.24+) for directory-scoped file access               |
  | JSON I/O         | json.NewDecoder/Encoder for streams, json.Marshal/Unmarshal for buffers |
  | File I/O         | os.ReadFile/WriteFile for simple ops, os.Open + defer Close for streams |
  | Logging          | slog structured logging (Go 1.21+). slog.With() for context.           |
  | Paths            | filepath.Join for OS paths, path.Join for URLs. filepath.WalkDir.       |
  | Database         | sql.DB is a pool (don't open/close per query). context.Context for all. |
  | HTTP             | http.Client with timeouts. Never use http.DefaultClient in production.  |
  | Testing          | Table-driven with t.Run(). t.Helper() on helpers. t.TempDir(). t.Context() (Go 1.24+). |
  | Benchmarks       | b.Loop() (Go 1.24+) instead of for i := 0; i < b.N; i++               |
  | String building  | strings.Builder for concatenation in loops.                             |

Anti-Patterns:

  | Anti-Pattern                              | Correct Alternative                                  |
  |-------------------------------------------|------------------------------------------------------|
  | _ = f.Close()                             | defer f.Close() or check error                       |
  | Goroutine leak (no cancellation)          | Always use context.WithCancel/Timeout                |
  | sync.Mutex without defer Unlock           | defer mu.Unlock() immediately after mu.Lock()        |
  | filepath.Walk                             | filepath.WalkDir (avoids unnecessary stat calls)     |
  | log.Fatal in library code                 | Return error to caller, let main decide              |
  | init() functions with side effects        | Explicit initialization in main/run                  |
  | Naked goroutine go func(){}()             | errgroup.Group or sync.WaitGroup.Go() (Go 1.25+)    |
  | Global sql.DB                             | Dependency injection via struct field or parameter    |
  | interface{} / any overuse                 | Use generics (Go 1.18+) or specific types            |
  | Ignoring context.Context                  | Pass context as first parameter to all I/O functions  |
  | time.Sleep in tests                       | testing/synctest (Go 1.25+) or channels              |
  | os.MkdirAll without error check           | Always check error, include path in error message    |
  | Mutable default args (slice/map literals) | Accept nil + allocate inside function                |

Procedure:
  1. Check implementation against common patterns table
  2. Replace any anti-patterns with correct alternatives
  3. Verify error handling wraps with context
  4. Verify logging uses slog, not fmt.Println
  5. Verify paths use filepath.Join, not string concatenation

Output: Code using correct patterns with no anti-patterns present
```

### apply-concurrency

```text
When: Working with goroutines, channels, shared state, or concurrent operations
Purpose: Ensure safe concurrency patterns, proper cancellation, and race-free code
Enforcement: ENF-L3 Advisory

Goroutine Patterns:

  Always use context.Context for cancellation.
  Use errgroup.Group for managed goroutine lifecycle.
  Use sync.WaitGroup.Go() (Go 1.25+) for simple fire-and-forget.
  Never start goroutines without a clear shutdown path.
  Document goroutine ownership ("who stops this?").

  // Good: errgroup with context cancellation
  g, ctx := errgroup.WithContext(ctx)
  for _, item := range items {
      g.Go(func() error {
          return process(ctx, item)
      })
  }
  if err := g.Wait(); err != nil {
      return fmt.Errorf("processing items: %w", err)
  }

Channel Patterns:

  Unbuffered for synchronization, buffered for decoupling.
  Always close channels from the sender side.
  Use select with ctx.Done() for cancellable operations.

  // Cancellable channel receive
  select {
  case result := <-ch:
      return result, nil
  case <-ctx.Done():
      return nil, ctx.Err()
  }

  Fan-out/fan-in with bounded concurrency:
    sem := make(chan struct{}, maxConcurrent)
    for _, item := range items {
        sem <- struct{}{}
        g.Go(func() error {
            defer func() { <-sem }()
            return process(ctx, item)
        })
    }

Sync Primitives:

  | Primitive     | When to Use                                          |
  |---------------|------------------------------------------------------|
  | sync.Mutex    | Lock/Unlock with defer, protect minimal critical section |
  | sync.RWMutex  | Read-heavy workloads (many readers, few writers)     |
  | sync.Once     | Lazy initialization (exactly-once semantics)         |
  | sync.Map      | Read-mostly maps with stable keys only               |

Memory Safety:

  No shared mutable state without synchronization.
  Slice/map pre-allocation: make([]T, 0, capacity).
  Avoid large allocations in hot loops.
  Use sync.Pool for frequently allocated/freed objects.
  Resource cleanup order: close in reverse order of opening.

Race Detection:

  ALWAYS run go test -race ./... before commit.
  Tests must pass with -race flag.
  Use t.Setenv() instead of os.Setenv() in tests (avoids race).
  Use testing/synctest (Go 1.25+) for deterministic concurrent tests.

Signal Handling (CLI):

  ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
  defer stop()

Procedure:
  1. Verify all goroutines have cancellation via context.Context
  2. Use errgroup.Group or sync.WaitGroup.Go() for goroutine lifecycle
  3. Verify channels are closed by sender, received with ctx.Done() select
  4. Verify shared state is protected by sync primitives with defer Unlock
  5. Run go test -race ./... to detect races

Output: Concurrent code with proper cancellation, synchronization, and race safety
```

### validate-package

```text
When: Before committing Go packages or during pre-commit checks
Purpose: Run lint, format, vet, and coverage checks
Enforcement: ENF-L3 Advisory

Lint / Format / Vet Rules:

  | Command                           | Usage                          |
  |-----------------------------------|--------------------------------|
  | go fmt ./...                      | Format all files (mandatory)   |
  | goimports -w .                    | Fix import grouping            |
  | go vet ./...                      | Catch common mistakes          |
  | golangci-lint run                 | Primary linter (comprehensive) |
  | go test -race ./...               | Race detection (mandatory)     |
  | go test -coverprofile=c.out ./... | Coverage report                |
  | go mod tidy                       | Remove unused dependencies     |

Coverage Thresholds:

  85% LINE coverage for application packages (internal/, pkg/).
  Lower threshold acceptable for cmd/ stubs (main.go entry points).
  go tool cover -func=coverage.out to verify per-function coverage.

Build Verification:

  CGO_ENABLED=0 go build ./cmd/codeflow/     # Must succeed
  GOOS=linux GOARCH=amd64 go build ./...      # Cross-compile check

Go 1.24-1.26 Features to Prefer:

  | Feature                | Version | Replaces                                      |
  |------------------------|---------|-----------------------------------------------|
  | errors.AsType[T]()     | 1.26    | errors.As() with var declaration              |
  | filepath.WalkDir       | 1.16+   | filepath.Walk                                 |
  | t.Context()            | 1.24    | Manual context in tests                       |
  | b.Loop()               | 1.24    | for i := 0; i < b.N; i++                     |
  | t.Chdir()              | 1.24    | Manual chdir + cleanup                        |
  | os.OpenRoot()          | 1.24    | Unrestricted path traversal                   |
  | sync.WaitGroup.Go()    | 1.25    | Manual wg.Add(1); go func(){ defer wg.Done() }|
  | testing/synctest        | 1.25    | time.Sleep in concurrent tests                |
  | slog.NewMultiHandler   | 1.26    | Manual handler chaining                       |
  | json ",omitzero"       | 1.24    | Ambiguous omitempty for zero values            |

Procedure:
  1. Run go fmt ./... to format all files
  2. Run go vet ./... to catch common mistakes
  3. Run golangci-lint run for comprehensive linting
  4. Run go test -race ./... for race detection
  5. Run go test -coverprofile to verify 85% coverage threshold
  6. Run go mod tidy to remove unused dependencies
  7. Verify CGO_ENABLED=0 go build succeeds
  8. Fix all issues before commit

Output: Clean lint, vet, race, and coverage output with no errors
```
