---
title: "INF-EPC-021 Performance Benchmarks: Shell vs Go Hook Latency"
task: "INF-TSK-021-024"
epic: "INF-EPC-021"
created_at: "2026-03-04"
platform: "darwin/arm64 (Apple M1 Max)"
go_version: "1.26.0"
---

# INF-EPC-021 Performance Benchmarks

## Executive Summary

The Go CLI hook implementation achieves a **1,300x improvement** in per-tool-call
overhead compared to the shell hook stack. Combined PreToolUse + PostToolUse
overhead dropped from ~300ms (shell) to ~0.23ms (Go). Binary cold start is
~7.3ms, well under the 10ms target.

## Shell Baseline (Historical Reference)

Source: go-cli-migration-comprehensive.md Section 7.

Shell overhead was estimated based on subprocess fork costs (~5-15ms per fork)
and measured library sourcing behavior. These measurements were taken before the
Go cutover (INF-TSK-021-022) retired the shell scripts.

| Hook Area | Shell Latency (estimated) | Notes |
|-----------|--------------------------|-------|
| PreToolUse hooks total | ~300ms per tool call | 7 hooks registered |
| Security enforcement stack | ~120ms | 12 shell modules sourced per call |
| Checkpoint operations (jq) | 30-80ms per write | jq JSON pipeline |
| Sentinel checks (stat+date) | ~10ms | Platform-specific stat/date |
| YAML parsing (sed) | ~20ms | Fragile sed-based parsing |
| Session start hook | ~200ms | Full initialization path |
| ULID generation | ~50ms | python3 subprocess |
| **Total per-tool-call** | **~300ms** | Sum of all firing hooks |

## Go Benchmark Results

Platform: darwin/arm64 (Apple M1 Max), Go 1.26.0.
Run: go test -bench=. -benchmem -v ./internal/benchmark/...

### Individual Hook Benchmarks

| Benchmark | Latency | Allocs/op | Bytes/op | Shell Equivalent |
|-----------|---------|-----------|----------|------------------|
| SecurityCheck | 229us | 2,876 | 425KB | ~120ms (security stack) |
| SecurityCheck (dangerous cmd) | 407us | 4,476 | 669KB | ~120ms |
| GateCheck | 2.0us | 9 | 704B | ~10ms (sentinel stat) |
| GateCheck (with sentinel) | 2.2us | 5 | 496B | ~10ms |
| EditWriteGuard | 1.4us | 16 | 1.1KB | ~10ms (path validation) |
| SentinelWrite | 46us | 26 | 1.4KB | ~10ms |
| SentinelWrite (from Reader) | 49us | 28 | 1.9KB | ~10ms |
| CheckpointRegister | 3.9us | 25 | 1.6KB | 30-80ms (jq pipeline) |
| CheckpointComplete | 0.7us | 6 | 736B | 30-80ms (jq pipeline) |
| TeamCreate | 206us | 31 | 2.3KB | N/A (Go-only) |
| TeammateSpawn | 219us | 43 | 3.2KB | N/A (Go-only) |

### Combined Per-Tool-Call Overhead

| Benchmark | Latency | Allocs/op | Bytes/op |
|-----------|---------|-----------|----------|
| PreToolUse total (security + gate + scope) | 229us | 2,897 | 427KB |
| PostToolUse total (sentinel + checkpoint) | 1.5us | 20 | 1.2KB |
| **Combined (all hooks per tool call)** | **233us** | **2,917** | **428KB** |

### Binary Startup

| Benchmark | Latency | Target | Status |
|-----------|---------|--------|--------|
| BinaryStartup (cold) | 7.3ms | < 10ms | PASS |

## Improvement Ratios

| Area | Shell | Go | Improvement |
|------|-------|----|-------------|
| Total per-tool-call overhead | ~300ms | 0.23ms | **1,300x** |
| Security enforcement | ~120ms | 0.23ms | **520x** |
| Gate check | ~10ms | 2.0us | **5,000x** |
| Checkpoint operations | 30-80ms | 3.9us | **7,700-20,500x** |
| Sentinel write | ~10ms | 46us | **217x** |
| Edit/write scope check | ~10ms | 1.4us | **7,100x** |

## Per-Session Impact

| Metric | Shell | Go | Savings |
|--------|-------|----|---------|
| Overhead per tool call | ~300ms | 0.23ms | 299.77ms |
| 10-tool-call session | ~3,000ms | 2.3ms | **2,998ms (~3s)** |
| 100-tool-call session | ~30,000ms | 23ms | **29,977ms (~30s)** |
| 1000-tool-call session | ~300,000ms | 230ms | **299,770ms (~5min)** |

## Memory Usage

The security checker dominates memory allocation at ~425KB per invocation due to
regex compilation and pattern matching across multiple security dimensions.
All other hooks are sub-2KB per invocation.

| Hook | Bytes/op | Allocs/op | Assessment |
|------|----------|-----------|------------|
| SecurityCheck | 425KB | 2,876 | Dominant; regex-heavy |
| GateCheck | 496-704B | 5-9 | Minimal |
| EditWriteGuard | 1.1KB | 16 | Minimal |
| SentinelWrite | 1.4KB | 26 | Minimal (includes file I/O) |
| CheckpointRegister | 1.6KB | 25 | Minimal |
| CheckpointComplete | 736B | 6 | Minimal |

## Methodology

1. **Shell baseline**: Historical estimates from pre-cutover analysis
   (go-cli-migration-comprehensive.md Section 7). Based on subprocess fork
   costs and library sourcing measurements.

2. **Go benchmarks**: Standard testing.B benchmarks with b.ResetTimer()
   after setup and b.ReportAllocs() for memory profiling. Run on Apple M1 Max
   with Go 1.26.0.

3. **Combined overhead**: Simulates the actual hook firing sequence for a
   typical Bash tool call (security + gate + scope pre-tool, sentinel +
   checkpoint post-tool).

4. **Binary startup**: Measures exec.Command("codeflow", "version") including
   process fork, Go runtime initialization, command dispatch, and exit.

## Benchmark Location

- Package: codeflow-cli/internal/benchmark/
- Test file: codeflow-cli/internal/benchmark/benchmark_test.go
- Test data: codeflow-cli/internal/benchmark/testdata/
- Run: cd codeflow-cli && go test -bench=. -benchmem ./internal/benchmark/...
