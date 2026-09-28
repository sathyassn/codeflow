export const DOCUMENT_ROOT_ID = "cf-present-document";
export const CHROME_ROOT_ID = "cf-present-chrome";
export const CONFIG_ID = "cf-present-config";
export const REQUEST_HEADER = "X-CF-Present";

export type UtilityTheme = "graphite" | "slate" | "sage";
export type Typeface = "archivo" | "inter" | "plex";
export type TypeScale = "compact" | "default" | "large";
export type AppearanceMode = "system" | "light" | "dark";
export type ResolvedMode = Exclude<AppearanceMode, "system">;
export type FeedbackKind = "comment" | "question" | "decision" | "suggestion" | "adjustment";
export type ReviewVerdict =
  | "approve"
  | "approve_with_notes"
  | "request_changes";
export type FeedbackLifecycle = "received" | "delivered" | "addressed" | "dismissed";
export type FeedbackAnchor =
  | Readonly<{ state: "block"; block_id: string }>
  | Readonly<{ state: "anchored" | "reanchored"; start_utf16: number; end_utf16: number; changed?: boolean }>
  | Readonly<{ state: "entity_anchored"; entity_id: string }>
  | Readonly<{ state: "entity_reanchored"; entity_id: string; label_changed: boolean }>
  | Readonly<{ state: "block_fallback"; block_id: string; reason: string }>
  | Readonly<{ state: "element_anchored" | "element_reanchored"; element_path: string }>
  | Readonly<{
      state: "region_anchored" | "region_reanchored";
      scope: RegionScope;
      anchor_id: string;
      x_ppm: number;
      y_ppm: number;
      width_ppm: number;
      height_ppm: number;
    }>
  | Readonly<{ state: "orphaned"; reason: string }>;

export interface FeedbackHistoryNote {
  readonly id: string;
  readonly block_label: string;
  readonly kind: FeedbackKind;
  readonly body: string;
  readonly quote?: string;
  readonly anchor: FeedbackAnchor;
}

export interface FeedbackHistoryItem {
  readonly event_id: string;
  readonly source_revision: number;
  readonly event_version: number;
  readonly lifecycle: FeedbackLifecycle;
  /** The agent acknowledged the review (SPC-014 B8), apart from delivery. */
  readonly acknowledged?: boolean;
  readonly verdict: ReviewVerdict;
  readonly instruction?: string;
  readonly notes: readonly FeedbackHistoryNote[];
}

export interface FeedbackSnapshot {
  readonly items: readonly FeedbackHistoryItem[];
  readonly omitted_older: number;
}

export interface ChromeConfig {
  readonly schema_version: 1;
  readonly session_id: string;
  readonly revision: number;
  readonly event_sequence: number;
  readonly response_sequence: number;
  readonly title: string;
  readonly shortcuts_enabled: boolean;
  readonly review_limits: Readonly<{
    readonly max_notes: number;
    readonly max_visible_feedback: number;
    readonly max_text_utf16: number;
    readonly max_selector_utf16: number;
    readonly max_payload_bytes: number;
  }>;
  readonly identity?: Readonly<{
    readonly src: string;
    readonly alt: string;
  }>;
  readonly feedback?: FeedbackSnapshot;
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

export interface ElementSelector {
  readonly element_path: string;
  readonly tag_name: string;
  readonly label: string;
  readonly block_digest: string;
}

/** SPC-014 I2: a note on a named entity, checked by the service at submit. */
export interface EntitySelector {
  readonly entity_id: string;
  readonly label: string;
  readonly block_digest: string;
  readonly variant?: "wide" | "narrow";
  readonly crop_box?: Readonly<{ x: number; y: number; width: number; height: number }>;
}

/** SPC-014 I3: the body of a refused review. */
export interface ServiceErrorBody {
  readonly error: string;
  readonly message: string;
  readonly details?: Readonly<Record<string, unknown>>;
}

export function parseServiceError(text: string): ServiceErrorBody | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  if (!isRecord(value) || typeof value.error !== "string" || typeof value.message !== "string") return null;
  if (value.details !== undefined && !isRecord(value.details)) return null;
  return {
    error: value.error,
    message: value.message,
    ...(value.details !== undefined ? { details: value.details } : {}),
  };
}

export type RegionScope = "block" | "document";

export interface RegionSelector {
  readonly scope: RegionScope;
  readonly anchor_id: string;
  readonly block_digest: string;
  readonly x_ppm: number;
  readonly y_ppm: number;
  readonly width_ppm: number;
  readonly height_ppm: number;
  readonly capture_width_px: number;
  readonly capture_height_px: number;
}

export interface PendingFeedback {
  readonly client_id: string;
  readonly block_id: string;
  readonly block_label: string;
  readonly kind: FeedbackKind;
  readonly body: string;
  readonly selector?: TextSelector;
  readonly element_selector?: ElementSelector;
  readonly entity_selector?: EntitySelector;
  readonly region_selector?: RegionSelector;
  readonly target_summary?: string;
  readonly excerpt?: {
    readonly text?: string;
    readonly image?: { readonly media_type: "image/jpeg" | "image/png"; readonly data_base64: string };
  };
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

/** Where an answer is on its way to the agent (SPC-014 B8). */
export type AnswerDelivery = "pending" | "delivered" | "acknowledged";

export interface AnswerStateEntry {
  readonly answer_id: string;
  readonly status: AnswerDelivery;
}

export interface SessionEvent {
  readonly cursor: string;
  readonly kind: "feedback_state" | "revision" | "session_closed" | "answer_state";
  readonly message?: string;
  readonly answers?: readonly AnswerStateEntry[];
}

export function readChromeConfig(root: HTMLElement): ChromeConfig {
  const node = document.getElementById(CONFIG_ID);
  if (!(node instanceof HTMLTemplateElement)) {
    throw new Error(`Missing ${CONFIG_ID} inert configuration payload`);
  }

  const value: unknown = JSON.parse(node.content.textContent ?? "null");
  if (!isRecord(value) || value.schema_version !== 1) {
    throw new Error("Unsupported cf-present chrome configuration");
  }
  if (
    typeof value.session_id !== "string" ||
    typeof value.revision !== "number" ||
    !Number.isSafeInteger(value.revision) ||
    value.revision < 1 ||
    typeof value.event_sequence !== "number" ||
    !Number.isSafeInteger(value.event_sequence) ||
    value.event_sequence < 0 ||
    typeof value.response_sequence !== "number" ||
    !Number.isSafeInteger(value.response_sequence) ||
    value.response_sequence < 0 ||
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
  const identity = value.identity;
  if (identity !== undefined && identity !== null && !isIdentity(identity)) {
    throw new Error("Malformed cf-present identity configuration");
  }
  if (!isReviewLimits(value.review_limits)) {
    throw new Error("Malformed cf-present review limits");
  }
  const feedback = value.feedback;
  if (
    feedback !== undefined &&
    feedback !== null &&
    !isFeedbackSnapshot(feedback, value.review_limits)
  ) {
    throw new Error("Malformed cf-present feedback snapshot");
  }

  return {
    schema_version: 1,
    session_id: value.session_id,
    revision: value.revision,
    event_sequence: value.event_sequence,
    response_sequence: value.response_sequence,
    title: value.title,
    shortcuts_enabled: value.shortcuts_enabled,
    review_limits: value.review_limits,
    ...(identity ? { identity } : {}),
    ...(feedback ? { feedback } : {}),
    ...(keymap ? { keymap } : {}),
  };
}

function isReviewLimits(value: unknown): value is ChromeConfig["review_limits"] {
  if (!isRecord(value)) return false;
  return ["max_notes", "max_visible_feedback", "max_text_utf16", "max_selector_utf16", "max_payload_bytes"].every(
    (key) => typeof value[key] === "number" && Number.isSafeInteger(value[key]) && value[key] > 0,
  );
}

function isFeedbackSnapshot(
  value: unknown,
  limits: ChromeConfig["review_limits"],
): value is FeedbackSnapshot {
  if (!isRecord(value) || !Array.isArray(value.items) || value.items.length > limits.max_visible_feedback) return false;
  if (
    typeof value.omitted_older !== "number" ||
    !Number.isSafeInteger(value.omitted_older) ||
    value.omitted_older < 0
  ) return false;
  return value.items.every((item) => {
    if (!isRecord(item) || !Array.isArray(item.notes) || item.notes.length > limits.max_notes) return false;
    if (
      typeof item.event_id !== "string" ||
      typeof item.source_revision !== "number" ||
      !Number.isSafeInteger(item.source_revision) ||
      item.source_revision < 1 ||
      typeof item.event_version !== "number" ||
      !Number.isSafeInteger(item.event_version) ||
      item.event_version < 1 ||
      (item.instruction !== undefined && typeof item.instruction !== "string") ||
      !["received", "delivered", "addressed", "dismissed"].includes(String(item.lifecycle)) ||
      (item.acknowledged !== undefined && typeof item.acknowledged !== "boolean") ||
      !["approve", "approve_with_notes", "request_changes"].includes(String(item.verdict))
    ) return false;
    return item.notes.every((note) =>
      isRecord(note) &&
      typeof note.id === "string" &&
      typeof note.block_label === "string" &&
      typeof note.body === "string" &&
      (note.quote === undefined || typeof note.quote === "string") &&
      ["comment", "question", "decision", "suggestion", "adjustment"].includes(String(note.kind)) &&
      isFeedbackAnchor(note.anchor)
    );
  });
}

function isFeedbackAnchor(value: unknown): value is FeedbackAnchor {
  if (!isRecord(value) || typeof value.state !== "string") return false;
  if (value.state === "block") return typeof value.block_id === "string";
  if (value.state === "orphaned") return typeof value.reason === "string";
  if (value.state === "entity_anchored") return typeof value.entity_id === "string";
  if (value.state === "entity_reanchored") return typeof value.entity_id === "string" && typeof value.label_changed === "boolean";
  if (value.state === "block_fallback") return typeof value.block_id === "string" && typeof value.reason === "string";
  if (value.state === "element_anchored" || value.state === "element_reanchored") {
    return typeof value.element_path === "string";
  }
  if (value.state === "region_anchored" || value.state === "region_reanchored") {
    return (
      (value.scope === "block" || value.scope === "document") &&
      typeof value.anchor_id === "string" &&
      ["x_ppm", "y_ppm", "width_ppm", "height_ppm"].every(
        (key) => typeof value[key] === "number" && Number.isSafeInteger(value[key]),
      )
    );
  }
  return (
    (value.state === "anchored" || value.state === "reanchored") &&
    typeof value.start_utf16 === "number" &&
    Number.isSafeInteger(value.start_utf16) &&
    value.start_utf16 >= 0 &&
    typeof value.end_utf16 === "number" &&
    Number.isSafeInteger(value.end_utf16) &&
    value.end_utf16 > value.start_utf16 &&
    (value.changed === undefined || typeof value.changed === "boolean")
  );
}

function isIdentity(value: unknown): value is NonNullable<ChromeConfig["identity"]> {
  if (!isRecord(value) || typeof value.src !== "string" || typeof value.alt !== "string") {
    return false;
  }
  return (
    value.alt.length > 0 &&
    value.alt.length <= 512 &&
    (value.src.startsWith("data:image/png;base64,") ||
      value.src.startsWith("data:image/webp;base64,"))
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isKeymap(value: unknown): value is NonNullable<ChromeConfig["keymap"]> {
  if (!isRecord(value)) return false;
  const keys = ["next", "previous", "review", "edit"] as const;
  return keys.every((key) => typeof value[key] === "string" && [...value[key]].length === 1);
}
