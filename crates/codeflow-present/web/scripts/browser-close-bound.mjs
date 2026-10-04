/**
 * How long the qualification waits for a browser close before it falls back to
 * exact-owned termination: the ceiling, less what the run needs to finish its
 * teardown inside the qualification deadline. The parent check kills the run
 * 30 s after that deadline, so a close that starts late waits only a short
 * floor and its fallback starts before the kill. The fallback's own waits can
 * still overrun the 30 s when a process will not die; that run has failed
 * already. A healthy close takes well under a second, so the floor is no
 * limit on a close that works on a quiet host, and a late run on a loaded
 * host is near its deadline failure anyway.
 */
export const CLOSE_TEARDOWN_RESERVE_MS = 30_000;
export const MINIMUM_CLOSE_WAIT_MS = 5_000;

export function closeWaitMs(ceilingMs, remainingMs) {
  return Math.max(
    MINIMUM_CLOSE_WAIT_MS,
    Math.min(ceilingMs, remainingMs - CLOSE_TEARDOWN_RESERVE_MS),
  );
}
