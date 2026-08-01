export const DOCUMENT_ROOT_ID = "cf-present-document";
export const CHROME_ROOT_ID = "cf-present-chrome";
export const CONFIG_ID = "cf-present-config";
export const REQUEST_HEADER = "X-CF-Present";

export type UtilityTheme = "editorial" | "technical";
export type AppearanceMode = "system" | "light" | "dark";
export type ResolvedMode = Exclude<AppearanceMode, "system">;
export type FeedbackKind = "comment" | "question" | "decision" | "suggestion";
export type ReviewVerdict =
  | "approve"
  | "approve_with_notes"
  | "request_changes";

export interface ChromeConfig {
  readonly schema_version: 1;
  readonly session_id: string;
  readonly revision: number;
  readonly title: string;
  readonly shortcuts_enabled: boolean;
  readonly keymap?: Readonly<{
    next: string;
    previous: string;
    review: string;
    edit: string;
  }>;
}

export interface TextSelector {
  readonly start_utf16: number;
  readonly end_utf16: number;
  readonly exact: string;
  readonly prefix: string;
  readonly suffix: string;
}

export interface PendingFeedback {
  readonly client_id: string;
  readonly block_id: string;
  readonly block_label: string;
  readonly kind: FeedbackKind;
  readonly body: string;
  readonly selector?: TextSelector;
}

export interface ReviewRequest {
  readonly event_id: string;
  readonly session_id: string;
  readonly revision: number;
  readonly verdict: ReviewVerdict;
  readonly instruction?: string;
  readonly notes: readonly PendingFeedback[];
}

export interface ReviewResponse {
  readonly event_id: string;
  readonly state: "received" | "duplicate";
}

export interface SessionEvent {
  readonly cursor: string;
  readonly kind: "feedback_state" | "revision" | "session_closed";
  readonly message?: string;
}

export function readChromeConfig(root: HTMLElement): ChromeConfig {
  const node = document.getElementById(CONFIG_ID);
  if (!(node instanceof HTMLScriptElement) || node.type !== "application/json") {
    throw new Error(`Missing ${CONFIG_ID} application/json payload`);
  }

  const value: unknown = JSON.parse(node.textContent ?? "null");
  if (!isRecord(value) || value.schema_version !== 1) {
    throw new Error("Unsupported cf-present chrome configuration");
  }
  if (
    typeof value.session_id !== "string" ||
    typeof value.revision !== "number" ||
    !Number.isSafeInteger(value.revision) ||
    value.revision < 1 ||
    typeof value.title !== "string" ||
    typeof value.shortcuts_enabled !== "boolean"
  ) {
    throw new Error("Malformed cf-present chrome configuration");
  }
  const keymap = value.keymap;
  if (keymap !== undefined && !isKeymap(keymap)) {
    throw new Error("Malformed cf-present shortcut configuration");
  }
  if (root.dataset.sessionId !== value.session_id) {
    throw new Error("Chrome root and configuration session IDs differ");
  }

  return {
    schema_version: 1,
    session_id: value.session_id,
    revision: value.revision,
    title: value.title,
    shortcuts_enabled: value.shortcuts_enabled,
    ...(keymap ? { keymap } : {}),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isKeymap(value: unknown): value is NonNullable<ChromeConfig["keymap"]> {
  if (!isRecord(value)) return false;
  const keys = ["next", "previous", "review", "edit"] as const;
  return keys.every((key) => typeof value[key] === "string" && [...value[key]].length === 1);
}
