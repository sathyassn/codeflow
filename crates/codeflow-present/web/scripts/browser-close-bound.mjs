/**
 * How long the qualification waits for a browser close before it falls back to
 * exact-owned termination: the ceiling, less what the run needs to finish its
 * teardown inside the qualification deadline. The parent check kills the run
 * 30 s after that deadline, so a close that starts late waits only a short
 * floor and the fallback can finish before the kill. A healthy close takes
 * well under a second, so the floor never cuts a working close short.
 */
export const CLOSE_TEARDOWN_RESERVE_MS = 30_000;
export const MINIMUM_CLOSE_WAIT_MS = 5_000;

export function closeWaitMs(ceilingMs, remainingMs) {
  return Math.max(
    MINIMUM_CLOSE_WAIT_MS,
    Math.min(ceilingMs, remainingMs - CLOSE_TEARDOWN_RESERVE_MS),
  );
}
