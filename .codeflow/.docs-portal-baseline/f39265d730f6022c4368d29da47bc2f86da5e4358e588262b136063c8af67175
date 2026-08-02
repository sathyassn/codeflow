const SIGNALS = ["SIGINT", "SIGTERM"];
const GRACE_MS = 5_000;

export async function withSignalAwareChildLifecycle(action) {
  if (typeof action !== "function") throw new Error("child lifecycle action must be a function");
  let interruptedBy = null;
  let rejectInterruption;
  const interruption = new Promise((_, reject) => { rejectInterruption = reject; });
  interruption.catch(() => {});
  const children = new Set();
  const cleanups = new Set();

  const lifecycle = {
    get interruptedBy() { return interruptedBy; },
    trackChild(child) {
      if (!child || typeof child.kill !== "function") throw new Error("tracked child is invalid");
      children.add(child);
      if (interruptedBy !== null) child.kill(interruptedBy);
      return () => children.delete(child);
    },
    addCleanup(cleanup) {
      if (typeof cleanup !== "function") throw new Error("child lifecycle cleanup must be a function");
      cleanups.add(cleanup);
      if (interruptedBy !== null) Promise.resolve().then(cleanup).catch(() => {});
      return () => cleanups.delete(cleanup);
    },
    async wait(value) {
      return Promise.race([Promise.resolve(value), interruption]);
    },
    async acquire(value, cleanup) {
      if (typeof cleanup !== "function") throw new Error("acquired resource cleanup must be a function");
      const pending = Promise.resolve(value);
      try { return await Promise.race([pending, interruption]); }
      catch (error) {
        if (interruptedBy !== null) pending.then(cleanup, () => {}).catch(() => {});
        throw error;
      }
    },
    throwIfInterrupted() {
      if (interruptedBy !== null) throw interruptedError(interruptedBy);
    },
  };

  const handlers = new Map(SIGNALS.map((signal) => [signal, () => {
    if (interruptedBy !== null) return;
    interruptedBy = signal;
    for (const child of children) child.kill(signal);
    for (const cleanup of cleanups) Promise.resolve().then(cleanup).catch(() => {});
    rejectInterruption(interruptedError(signal));
  }]));
  for (const [signal, handler] of handlers) process.once(signal, handler);

  let result;
  let actionError = null;
  try { result = await action(lifecycle); }
  catch (error) { actionError = error; }
  finally {
    for (const [signal, handler] of handlers) process.off(signal, handler);
    await Promise.allSettled([...cleanups].map((cleanup) => Promise.resolve().then(cleanup)));
    await Promise.all([...children].map((child) => stopChild(child, interruptedBy ?? "SIGTERM")));
  }
  if (interruptedBy !== null) {
    process.kill(process.pid, interruptedBy);
    await new Promise(() => {});
  }
  if (actionError) throw actionError;
  return result;
}

async function stopChild(child, signal) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((resolve) => child.once("exit", resolve));
  child.kill(signal);
  const graceful = await Promise.race([
    exited.then(() => true),
    new Promise((resolve) => {
      const timer = setTimeout(() => resolve(false), GRACE_MS);
      timer.unref();
    }),
  ]);
  if (graceful || child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGKILL");
  await Promise.race([
    exited,
    new Promise((resolve) => {
      const timer = setTimeout(resolve, GRACE_MS);
      timer.unref();
    }),
  ]);
}

function interruptedError(signal) {
  const error = new Error(`portal workflow interrupted by ${signal}`);
  error.code = "CODEFLOW_INTERRUPTED";
  error.signal = signal;
  return error;
}
