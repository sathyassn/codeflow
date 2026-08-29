const SIGNALS = ["SIGINT", "SIGTERM"];
const GRACE_MS = 5_000;
const stopAttempts = new WeakMap();

export async function withSignalAwareChildLifecycle(action) {
  if (typeof action !== "function") throw new Error("child lifecycle action must be a function");
  let interruptedBy = null;
  let rejectInterruption;
  const interruption = new Promise((_, reject) => { rejectInterruption = reject; });
  interruption.catch(() => {});
  const children = new Set();
  const cleanups = new Set();
  const acquisitions = new Set();
  const childStops = new Map();
  let state = "open";
  let shutdownPromise = null;

  const cleanupRecord = (cleanup) => ({ cleanup, promise: null });
  const runCleanup = (record) => {
    record.promise ??= Promise.resolve().then(record.cleanup);
    return record.promise;
  };
  const startChildStop = (child, signal) => {
    let attempt = childStops.get(child);
    if (!attempt) {
      attempt = stopChild(child, signal);
      // A signal handler starts shutdown without awaiting it. Attach a handler
      // immediately, then surface the same failure from the awaited shutdown.
      attempt.catch(() => {});
      childStops.set(child, attempt);
    }
    return attempt;
  };
  const beginShutdown = (signal) => {
    if (state === "open") state = "closing";
    shutdownPromise ??= (async () => {
      for (const child of children) startChildStop(child, signal);
      while (acquisitions.size > 0) {
        await Promise.allSettled([...acquisitions].map((record) => record.promise));
      }
      while (true) {
        const pending = [...cleanups];
        await Promise.allSettled(pending.map(runCleanup));
        if (pending.length === cleanups.size && [...cleanups].every((record) => record.promise !== null)) break;
      }
      // A resource that settles during shutdown may register a late child. Keep
      // taking stable snapshots until every tracked attempt has proved exit.
      let observedAttempts = -1;
      let outcomes = [];
      while (observedAttempts !== childStops.size) {
        for (const child of children) startChildStop(child, signal);
        observedAttempts = childStops.size;
        outcomes = await Promise.allSettled(childStops.values());
      }
      const failures = outcomes.filter((outcome) => outcome.status === "rejected").map((outcome) => outcome.reason);
      if (failures.length === 1) throw failures[0];
      if (failures.length > 1) throw new AggregateError(failures, "portal child cleanup could not prove every child exit");
      state = "closed";
    })();
    return shutdownPromise;
  };

  const lifecycle = {
    get interruptedBy() { return interruptedBy; },
    trackChild(child) {
      if (!child || typeof child.kill !== "function" || typeof child.on !== "function" || typeof child.off !== "function") {
        throw new Error("tracked child is invalid");
      }
      children.add(child);
      if (state !== "open") startChildStop(child, interruptedBy ?? "SIGTERM");
      return () => {
        if (!exitProven(child) && child.pid !== undefined) return false;
        return children.delete(child);
      };
    },
    addCleanup(cleanup) {
      if (typeof cleanup !== "function") throw new Error("child lifecycle cleanup must be a function");
      const record = cleanupRecord(cleanup);
      cleanups.add(record);
      if (shutdownPromise !== null) runCleanup(record).catch(() => {});
      return () => cleanups.delete(record);
    },
    async wait(value) {
      return Promise.race([Promise.resolve(value), interruption]);
    },
    async acquire(value, cleanup) {
      if (typeof cleanup !== "function") throw new Error("acquired resource cleanup must be a function");
      if (state !== "open") throw interruptedError(interruptedBy ?? "SIGTERM");
      const record = { promise: null, cleanupPromise: null };
      acquisitions.add(record);
      record.promise = Promise.resolve(value).then(async (resource) => {
        if (state !== "open") {
          record.cleanupPromise ??= Promise.resolve().then(() => cleanup(resource));
          await record.cleanupPromise;
        }
        return resource;
      }).finally(() => { acquisitions.delete(record); });
      return Promise.race([record.promise, interruption]);
    },
    throwIfInterrupted() {
      if (interruptedBy !== null) throw interruptedError(interruptedBy);
    },
  };

  const handlers = new Map(SIGNALS.map((signal) => [signal, () => {
    if (interruptedBy === null) {
      interruptedBy = signal;
      rejectInterruption(interruptedError(signal));
    }
    beginShutdown(interruptedBy).catch(() => {});
  }]));
  for (const [signal, handler] of handlers) process.on(signal, handler);

  let result;
  let actionError = null;
  let shutdownError = null;
  try { result = await action(lifecycle); }
  catch (error) { actionError = error; }
  finally {
    try { await beginShutdown(interruptedBy ?? "SIGTERM"); }
    catch (error) { shutdownError = error; }
    for (const [signal, handler] of handlers) process.off(signal, handler);
  }
  if (shutdownError && actionError) {
    throw new AggregateError([actionError, shutdownError], "portal workflow and child cleanup both failed");
  }
  if (shutdownError) throw shutdownError;
  if (interruptedBy !== null) {
    process.kill(process.pid, interruptedBy);
    await new Promise(() => {});
  }
  if (actionError) throw actionError;
  return result;
}

export function stopChild(child, signal = "SIGTERM", { graceMs = GRACE_MS } = {}) {
  if (!child || typeof child.kill !== "function" || typeof child.on !== "function" || typeof child.off !== "function") {
    return Promise.reject(new Error("child termination target is invalid"));
  }
  if (!Number.isSafeInteger(graceMs) || graceMs < 1 || graceMs > GRACE_MS) {
    return Promise.reject(new Error(`child termination grace must be an integer from 1 to ${GRACE_MS} milliseconds`));
  }
  let attempt = stopAttempts.get(child);
  if (!attempt) {
    attempt = stopChildOnce(child, signal, graceMs);
    // Keep a rejected proof observable by every caller without creating an
    // unhandled rejection before the lifecycle reaches its awaited teardown.
    attempt.catch(() => {});
    stopAttempts.set(child, attempt);
  }
  return attempt;
}

async function stopChildOnce(child, signal, graceMs) {
  if (exitProven(child)) return;
  let proven = false;
  let ambiguousExit = false;
  const childErrors = [];
  let resolveExit;
  const exited = new Promise((resolve) => { resolveExit = resolve; });
  const onExit = (code, exitSignal) => {
    if (code === null && exitSignal === null && !exitProven(child)) {
      ambiguousExit = true;
      return;
    }
    proven = true;
    resolveExit();
  };
  const onError = (error) => { childErrors.push(boundedMessage(error)); };
  child.on("exit", onExit);
  child.on("error", onError);
  if (exitProven(child)) {
    proven = true;
    resolveExit();
  }

  const attempts = [];
  try {
    attempts.push(signalChild(child, signal));
    if (await boundedExitProof(exited, () => proven || exitProven(child), graceMs)) return;
    if (signal !== "SIGKILL") attempts.push(signalChild(child, "SIGKILL"));
    if (await boundedExitProof(exited, () => proven || exitProven(child), graceMs)) return;
  } finally {
    child.off("exit", onExit);
    child.off("error", onError);
  }

  const detail = attempts.map(({ attemptedSignal, accepted, error }) => {
    if (error) return `${attemptedSignal} threw ${boundedMessage(error)}`;
    return `${attemptedSignal} returned ${accepted}`;
  }).join("; ");
  const ambiguity = ambiguousExit ? "; an exit event without a code or signal was not accepted as proof" : "";
  const errors = childErrors.length > 0 ? `; child emitted error: ${childErrors.join("; ")}` : "";
  throw new Error(`child exit was not proven after bounded termination (${detail}${ambiguity}${errors})`);
}

function signalChild(child, signal) {
  try {
    return { attemptedSignal: signal, accepted: child.kill(signal) === true, error: null };
  } catch (error) {
    return { attemptedSignal: signal, accepted: false, error };
  }
}

async function boundedExitProof(exited, isProven, graceMs) {
  if (isProven()) return true;
  let timer;
  await Promise.race([
    exited,
    new Promise((resolve) => { timer = setTimeout(resolve, graceMs); }),
  ]);
  if (timer) clearTimeout(timer);
  return isProven();
}

function exitProven(child) {
  return child.exitCode !== null && child.exitCode !== undefined
    || child.signalCode !== null && child.signalCode !== undefined;
}

function boundedMessage(error) {
  const message = error instanceof Error ? error.message : String(error);
  return message.length <= 256 ? message : `${message.slice(0, 253)}...`;
}

function interruptedError(signal) {
  const error = new Error(`portal workflow interrupted by ${signal}`);
  error.code = "CODEFLOW_INTERRUPTED";
  error.signal = signal;
  return error;
}
