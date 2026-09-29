// The page's answer checks (SPC-014 B6): the same rules as the server's
// `form.rs` `validate_answer`, so the page refuses before it sends what the
// server would refuse. Both sides are pinned to one vector file,
// `tests/fixtures/form-rules/parity.json`, which `scripts/check.mjs` and a
// Rust test in codeflow-present evaluate. No DOM here: `forms.ts` reads the
// form from the page and calls these.

export type FieldKind = "text" | "number" | "integer" | "boolean" | "choice" | "choices";
export type TextFormat = "email" | "uri" | "date" | "date-time" | "multiline";
export type RationaleMode = "none" | "optional" | "required";
export type Outcome = "submit" | "decline" | "cancel";
export type FieldErrorCode =
  | "required"
  | "unknown_field"
  | "wrong_kind"
  | "too_short"
  | "too_long"
  | "format"
  | "below_minimum"
  | "above_maximum"
  | "not_integer"
  | "unknown_option"
  | "too_few"
  | "too_many"
  | "rationale_required"
  | "rationale_not_allowed";

export interface FieldRules {
  readonly id: string;
  readonly kind: FieldKind;
  readonly rationale: RationaleMode;
  readonly minLength?: number;
  readonly maxLength?: number;
  readonly format?: TextFormat;
  readonly minimum?: number;
  readonly maximum?: number;
  readonly options?: readonly string[];
  readonly minItems?: number;
  readonly maxItems?: number;
}

export interface FormRules {
  readonly fields: readonly FieldRules[];
  readonly required: readonly string[];
}

export interface AnswerDraft {
  readonly outcome: Outcome;
  readonly values: Readonly<Record<string, unknown>>;
  readonly rationales: Readonly<Record<string, string>>;
  readonly reason?: string;
}

export interface FieldError {
  readonly field: string;
  readonly code: FieldErrorCode;
}

export const MAX_FORM_TEXT_UTF16 = 16_384;
export const MAX_DECLINE_REASON_BYTES = 4 * 1024;
export const MAX_ANSWER_REQUEST_BYTES = 64 * 1024;
export const DECISION_FIELD_ID = "choice";

/** The field errors of an answer, in the server's order. */
export function validateAnswer(form: FormRules, draft: AnswerDraft): FieldError[] {
  const errors: FieldError[] = [];
  const known = new Set(form.fields.map((field) => field.id));
  if (draft.outcome === "submit") {
    if (draft.reason !== undefined) errors.push({ field: "reason", code: "unknown_field" });
    for (const field of form.fields) {
      // A present null is a value of no field's kind, never an absent answer.
      const value = Object.hasOwn(draft.values, field.id) ? draft.values[field.id] : undefined;
      if ((value === undefined || isEmptyAnswer(value)) && form.required.includes(field.id)) {
        errors.push({ field: field.id, code: "required" });
      } else if (value !== undefined) {
        const code = checkValue(field, value);
        if (code) errors.push({ field: field.id, code });
      }
      const answered = value !== undefined && !isEmptyAnswer(value);
      const rationale = Object.hasOwn(draft.rationales, field.id) ? draft.rationales[field.id] : undefined;
      const code = checkRationale(field, rationale, answered);
      if (code) errors.push({ field: field.id, code });
    }
    const unknown = [...new Set([...Object.keys(draft.values), ...Object.keys(draft.rationales)])]
      .filter((key) => !known.has(key))
      .sort(byCodeUnits);
    errors.push(...unknown.map((field) => ({ field, code: "unknown_field" as const })));
    return errors;
  }
  errors.push(...Object.keys(draft.values).sort(byCodeUnits).map((field) => ({ field, code: "unknown_field" as const })));
  errors.push(...Object.keys(draft.rationales).sort(byCodeUnits).map((field) => ({ field, code: "rationale_not_allowed" as const })));
  if (draft.reason !== undefined) {
    if (draft.outcome === "cancel") errors.push({ field: "reason", code: "unknown_field" });
    else if (new TextEncoder().encode(draft.reason).byteLength > MAX_DECLINE_REASON_BYTES) {
      errors.push({ field: "reason", code: "too_long" });
    }
  }
  return errors;
}

// Rust sorts `String` keys by bytes; for the ASCII field ids of a form the
// UTF-16 order is the same.
function byCodeUnits(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function isEmptyAnswer(value: unknown): boolean {
  return value === "" || (Array.isArray(value) && value.length === 0);
}

function checkRationale(field: FieldRules, rationale: string | undefined, answered: boolean): FieldErrorCode | null {
  if (field.rationale === "none" && rationale !== undefined) return "rationale_not_allowed";
  if (field.rationale === "required" && answered && (rationale === undefined || rationale.trim() === "")) {
    return "rationale_required";
  }
  return null;
}

function checkValue(field: FieldRules, value: unknown): FieldErrorCode | null {
  switch (field.kind) {
    case "text": {
      if (typeof value !== "string") return "wrong_kind";
      if (value.length < (field.minLength ?? 0)) return "too_short";
      if (value.length > (field.maxLength ?? MAX_FORM_TEXT_UTF16)) return "too_long";
      return textFormatValid(field.format, value) ? null : "format";
    }
    case "number":
      if (typeof value !== "number" || !Number.isFinite(value)) return "wrong_kind";
      return checkBounds(field, value);
    case "integer":
      if (typeof value !== "number" || !Number.isFinite(value)) return "wrong_kind";
      if (!Number.isInteger(value)) return "not_integer";
      if (value < -Number.MAX_SAFE_INTEGER) return "below_minimum";
      if (value > Number.MAX_SAFE_INTEGER) return "above_maximum";
      return checkBounds(field, value);
    case "boolean":
      return typeof value === "boolean" ? null : "wrong_kind";
    case "choice":
      if (typeof value !== "string") return "wrong_kind";
      return field.options?.includes(value) ? null : "unknown_option";
    case "choices": {
      if (!Array.isArray(value) || value.some((item) => typeof item !== "string")) return "wrong_kind";
      const chosen = value as string[];
      if (chosen.some((item) => !field.options?.includes(item))) return "unknown_option";
      if (chosen.length < (field.minItems ?? 0)) return "too_few";
      if (field.maxItems !== undefined && chosen.length > field.maxItems) return "too_many";
      // A choices value is a set: a repeated option is the wrong shape.
      return new Set(chosen).size === chosen.length ? null : "wrong_kind";
    }
  }
}

function checkBounds(field: FieldRules, value: number): FieldErrorCode | null {
  if (field.minimum !== undefined && value < field.minimum) return "below_minimum";
  if (field.maximum !== undefined && value > field.maximum) return "above_maximum";
  return null;
}

export function textFormatValid(format: TextFormat | undefined, text: string): boolean {
  switch (format) {
    case undefined:
    case "multiline":
      return true;
    case "email":
      return isEmail(text);
    case "uri":
      return isUri(text);
    case "date":
      return isFullDate(text);
    case "date-time":
      return isDateTime(text);
  }
}

// Unicode White_Space, as Rust's `char::is_whitespace`: JavaScript's `\s`
// differs (it holds U+FEFF and lacks U+0085).
const WHITE_SPACE = /[\u0009-\u000D\u0020\u0085\u00A0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000]/u;

/** B6 `email`: one `@` with a non-empty local part and a domain holding a dot, no whitespace. */
export function isEmail(text: string): boolean {
  const at = text.indexOf("@");
  if (at < 0) return false;
  const local = text.slice(0, at);
  const domain = text.slice(at + 1);
  return local.length > 0 && !domain.includes("@") && domain.includes(".") && !WHITE_SPACE.test(text);
}

/** B6 `uri`: a scheme (a letter, then letters, digits, `+`, `-`, `.`), a colon and a non-empty rest of printable ASCII. */
export function isUri(text: string): boolean {
  const colon = text.indexOf(":");
  if (colon < 0) return false;
  return /^[A-Za-z][A-Za-z0-9+.-]*$/u.test(text.slice(0, colon)) && /^[\x21-\x7E]+$/u.test(text.slice(colon + 1));
}

/** RFC 3339 `full-date`: `YYYY-MM-DD` naming a real day. */
export function isFullDate(text: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/u.exec(text);
  if (!match) return false;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1];
  return days !== undefined && day >= 1 && day <= days;
}

/** RFC 3339 `date-time`: a full-date, `T`, `HH:MM:SS`, an optional fraction, and `Z` or a `+HH:MM`/`-HH:MM` offset. */
export function isDateTime(text: string): boolean {
  if (!/^[\x00-\x7F]*$/u.test(text) || text.length < 20) return false;
  if (!isFullDate(text.slice(0, 10))) return false;
  const match = /^[Tt](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:([Zz])|([+-])(\d{2}):(\d{2}))$/u.exec(text.slice(10));
  if (!match) return false;
  if (Number(match[1]) > 23 || Number(match[2]) > 59 || Number(match[3]) > 60) return false;
  return match[4] !== undefined || (Number(match[6]) <= 23 && Number(match[7]) <= 59);
}

/**
 * The rules of a `form` block or a v2 `decision` as the document declares
 * it (the JSON the author wrote), for the parity check. The page reads the
 * same rules from the rendered form instead.
 */
export function rulesFromBlock(block: Readonly<Record<string, unknown>>): FormRules {
  if (block.type === "decision") {
    const options = (block.options as readonly Readonly<Record<string, unknown>>[]).map((option) => String(option.value));
    return {
      fields: [{ id: DECISION_FIELD_ID, kind: "choice", rationale: (block.rationale as RationaleMode | undefined) ?? "optional", options }],
      required: [DECISION_FIELD_ID],
    };
  }
  const fields = (block.fields as readonly Readonly<Record<string, unknown>>[]).map((field): FieldRules => ({
    id: String(field.id),
    kind: field.kind as FieldKind,
    rationale: (field.rationale as RationaleMode | undefined) ?? "none",
    ...(typeof field.min_length === "number" ? { minLength: field.min_length } : {}),
    ...(typeof field.max_length === "number" ? { maxLength: field.max_length } : {}),
    ...(typeof field.format === "string" ? { format: field.format as TextFormat } : {}),
    ...(typeof field.minimum === "number" ? { minimum: field.minimum } : {}),
    ...(typeof field.maximum === "number" ? { maximum: field.maximum } : {}),
    ...(Array.isArray(field.options) ? { options: field.options.map((option: Readonly<Record<string, unknown>>) => String(option.value)) } : {}),
    ...(typeof field.min_items === "number" ? { minItems: field.min_items } : {}),
    ...(typeof field.max_items === "number" ? { maxItems: field.max_items } : {}),
  }));
  return { fields, required: (block.required as readonly string[] | undefined) ?? [] };
}
