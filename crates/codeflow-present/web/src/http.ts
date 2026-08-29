import { REQUEST_HEADER } from "./contracts";

const MAX_RESPONSE_BYTES = 64 * 1024;

export class PresentRequestError extends Error {
  public readonly status: number;

  public constructor(status: number, message: string) {
    super(message);
    this.name = "PresentRequestError";
    this.status = status;
  }
}

export async function postJson<T>(
  path: `/app/${string}`,
  body: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const response = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    cache: "no-store",
    redirect: "error",
    headers: {
      "Content-Type": "application/json",
      [REQUEST_HEADER]: "1",
    },
    body: JSON.stringify(body),
    ...(signal ? { signal } : {}),
  });
  const declaredLength = Number(response.headers.get("Content-Length") ?? "0");
  if (Number.isFinite(declaredLength) && declaredLength > MAX_RESPONSE_BYTES) {
    throw new PresentRequestError(response.status, "Response exceeds the client limit");
  }
  const text = await response.text();
  if (new TextEncoder().encode(text).byteLength > MAX_RESPONSE_BYTES) {
    throw new PresentRequestError(response.status, "Response exceeds the client limit");
  }
  if (!response.ok) {
    throw new PresentRequestError(response.status, text || `Request failed (${response.status})`);
  }
  const contentType = response.headers.get("Content-Type")?.toLowerCase() ?? "";
  if (!contentType.startsWith("application/json")) {
    throw new PresentRequestError(response.status, "Response is not JSON");
  }
  return JSON.parse(text) as T;
}
