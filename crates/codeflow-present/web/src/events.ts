import type { SessionEvent } from "./contracts";
import { postJson, PresentRequestError } from "./http";

const RETRY_DELAY_MS = 1_000;

export function followSessionEvents(
  onEvent: (event: SessionEvent) => void,
  onError: (message: string) => void,
): () => void {
  const controller = new AbortController();
  void poll(controller.signal, onEvent, onError);
  return () => controller.abort();
}

async function poll(
  signal: AbortSignal,
  onEvent: (event: SessionEvent) => void,
  onError: (message: string) => void,
): Promise<void> {
  let cursor: string | null = null;
  while (!signal.aborted) {
    try {
      const nextEvent: SessionEvent = await postJson<SessionEvent>(
        "/app/api/events/poll",
        { cursor },
        signal,
      );
      cursor = nextEvent.cursor;
      onEvent(nextEvent);
      if (nextEvent.kind === "session_closed") return;
    } catch (error) {
      if (signal.aborted) return;
      if (error instanceof PresentRequestError && error.status === 410) return;
      onError("Live review updates paused. Reconnecting…");
      await abortableDelay(RETRY_DELAY_MS, signal);
    }
  }
}

async function abortableDelay(delay: number, signal: AbortSignal): Promise<void> {
  await new Promise<void>((resolve) => {
    const finish = (): void => {
      clearTimeout(timer);
      signal.removeEventListener("abort", finish);
      resolve();
    };
    const timer = window.setTimeout(finish, delay);
    signal.addEventListener("abort", finish, { once: true });
  });
}
