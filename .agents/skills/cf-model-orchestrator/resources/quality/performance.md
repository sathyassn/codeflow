## Performance, scale and concurrency

For performance-, scale-, or concurrency-sensitive paths, review the actual
operating shape rather than only functional output: algorithmic complexity and
N+1 access, bounded work and memory, backpressure and cancellation, blocking in
async paths, state ownership and synchronization, lost updates, races,
deadlocks, idempotency, retry amplification, and resource cleanup as
applicable. Require a benchmark, profiler, load/stress test, race/concurrency
test, or direct operational measurement when a material claim or evidenced risk
needs it; do not add ceremonial performance tests to an unaffected path.
