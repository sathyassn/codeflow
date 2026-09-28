// The form runtime (SPC-014 B6). The service renders each `form` block and
// v2 `decision` with its controls, rules and digest; this module reads them,
// checks the answer with the rules the server shares, and sends it to
// `POST /app/api/answers` under the v1 request checks. The draft is the
// controls themselves: it lives in page memory for the life of the page and
// is never written to any browser storage. Annotating a form never changes a
// value and never sends.

import { parseServiceError, REQUEST_HEADER, type ChromeConfig } from "./contracts";
import {
  MAX_ANSWER_REQUEST_BYTES,
  MAX_DECLINE_REASON_BYTES,
  validateAnswer,
  type AnswerDraft,
  type FieldError,
  type FieldKind,
  type FieldRules,
  type FormRules,
  type Outcome,
  type RationaleMode,
  type TextFormat,
} from "./form-rules";

/** The chrome forwards session events to the document root under this name. */
export const SESSION_EVENT = "cf-present:session-event";
export type SessionEventDetail = "revision" | "session_closed";

type FormState = "editing" | "submitting" | "stored" | "failed" | "stale" | "closed";
type Action = "submit" | "decline" | "cancel" | "resend" | "confirm" | "amend";

const ANSWERS_PATH = "/app/api/answers";
const MAX_RESPONSE_BYTES = 64 * 1024;
const JSON_NUMBER = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/u;
const CONTROLS = "input, textarea, select, button";

interface Receipt {
  readonly answer_id: string;
  readonly revision: number;
  readonly state: string;
}

interface Sent {
  readonly body: string;
  readonly outcome: Outcome;
}

interface StaleTarget {
  readonly revision: number;
  readonly digest: string | null;
}

export function enhanceForms(root: HTMLElement, config: ChromeConfig): void {
  const forms = [...root.querySelectorAll<HTMLElement>("article[data-cf-form]")]
    .filter((article) => article.querySelector("[data-cf-form-action]"))
    .map((article) => new FormController(article, config));
  if (forms.length === 0) return;
  guardAnnotation(root);
  root.addEventListener(SESSION_EVENT, (event) => {
    const detail = (event as CustomEvent<SessionEventDetail>).detail;
    for (const form of forms) {
      if (detail === "revision") form.newerRevision();
      else if (detail === "session_closed") form.close();
    }
  });
}

// While the reviewer is commenting, a click on a form pins a note and does
// nothing else: it never toggles an option, focuses a control or presses a
// button (B6). The chrome's own capture listeners on the same root still run.
function guardAnnotation(root: HTMLElement): void {
  const commenting = (): boolean => root.dataset.cfCommenting === "true";
  const inForm = (target: EventTarget | null): target is Element =>
    target instanceof Element && root.contains(target) && target.closest("[data-cf-form]") !== null;
  root.addEventListener("click", (event) => {
    if (!commenting() || !inForm(event.target)) return;
    event.preventDefault();
    event.stopPropagation();
  }, { capture: true });
  root.addEventListener("mousedown", (event) => {
    if (commenting() && inForm(event.target) && event.target.closest(CONTROLS)) event.preventDefault();
  }, { capture: true });
  root.addEventListener("keydown", (event) => {
    if (!commenting() || !inForm(event.target) || !event.target.closest(CONTROLS)) return;
    if (event.key === " " || event.key === "Enter" || event.key.startsWith("Arrow")) event.preventDefault();
  }, { capture: true });
}

class FormController {
  private readonly id: string;
  private readonly rules: FormRules;
  private readonly stateLine: HTMLElement;
  private readonly declineArea: HTMLElement | null;
  private readonly buttons: Map<Action, HTMLButtonElement>;
  private state: FormState = "editing";
  private revision: number;
  private digest: string;
  private original: string | null = null;
  private amending = false;
  private declining = false;
  private sent: Sent | null = null;
  private stale: StaleTarget | null = null;
  private gone = false;
  private newer = false;

  public constructor(private readonly article: HTMLElement, private readonly config: ChromeConfig) {
    this.id = article.dataset.cfForm ?? "";
    this.digest = article.dataset.cfFormDigest ?? "";
    this.revision = config.revision;
    this.rules = readRules(article);
    this.stateLine = article.querySelector<HTMLElement>("[data-cf-form-state]") ?? document.createElement("p");
    this.declineArea = article.querySelector<HTMLElement>(".cf-form__decline");
    this.buttons = new Map(
      [...article.querySelectorAll<HTMLButtonElement>("button[data-cf-form-action]")].map((button) => [button.dataset.cfFormAction as Action, button]),
    );
    this.buttons.forEach((button, action) => button.addEventListener("click", () => this.act(action)));
    this.article.addEventListener("input", () => this.clearFieldError());
    this.render("");
  }

  public newerRevision(): void {
    this.newer = true;
    if (this.state === "editing" || this.state === "failed") {
      this.render(this.state === "failed"
        ? "A newer revision exists. Your answer is kept; send it again, and the service checks it against the current revision."
        : "A newer revision exists. Your draft is kept; sending it checks it against the current revision.", this.state === "failed" ? "failed" : "stale");
    }
  }

  public close(): void {
    this.render("This session is closed. Your draft is kept here; nothing can be sent.", "closed");
  }

  private act(action: Action): void {
    if (this.article.closest("[data-cf-commenting='true']")) return;
    switch (action) {
      case "submit":
        void this.send("submit");
        return;
      case "decline":
        if (!this.declining) {
          this.declining = true;
          this.render("Add a reason if you like, then send the decline.");
          this.declineArea?.querySelector("textarea")?.focus();
          return;
        }
        void this.send("decline");
        return;
      case "cancel":
        void this.send("cancel");
        return;
      case "resend":
        if (this.sent) void this.post(this.sent, null);
        return;
      case "confirm":
        if (this.stale && this.sent) void this.send(this.sent.outcome, this.stale);
        return;
      case "amend":
        this.amending = true;
        this.declining = false;
        this.render("Correcting the stored answer. Sending adds a correction; the original stays readable.", "editing");
        return;
    }
  }

  private async send(outcome: Outcome, target: StaleTarget | null = null): Promise<void> {
    const draft = this.draft(outcome);
    const errors = validateAnswer(this.rules, draft);
    this.showErrors(errors);
    if (errors.length > 0) {
      const reason = errors.find((error) => error.field === "reason");
      this.render(reason
        ? `The decline reason is over ${MAX_DECLINE_REASON_BYTES / 1024} KiB; shorten it.`
        : "Check the marked fields, then send again.");
      return;
    }
    const body = JSON.stringify({
      request_id: crypto.randomUUID(),
      session_id: this.config.session_id,
      revision: target?.revision ?? this.revision,
      form_id: this.id,
      form_digest: target?.digest ?? this.digest,
      outcome,
      values: draft.values,
      rationales: draft.rationales,
      ...(draft.reason !== undefined ? { reason: draft.reason } : {}),
      ...(this.amending && this.original ? { amends: this.original } : {}),
    });
    if (new TextEncoder().encode(body).byteLength > MAX_ANSWER_REQUEST_BYTES) {
      this.render(`This answer is over the ${MAX_ANSWER_REQUEST_BYTES / 1024} KiB limit; shorten it.`);
      return;
    }
    await this.post({ body, outcome }, target);
  }

  // One request: a new one, or the same bytes again for a resend, so the
  // service answers a resend with the original receipt (B6).
  private async post(sent: Sent, target: StaleTarget | null): Promise<void> {
    this.sent = sent;
    this.render("Sending...", "submitting");
    let status: number;
    let text: string;
    try {
      const response = await fetch(ANSWERS_PATH, {
        method: "POST",
        credentials: "same-origin",
        cache: "no-store",
        redirect: "error",
        headers: { "Content-Type": "application/json", [REQUEST_HEADER]: "1" },
        body: sent.body,
      });
      status = response.status;
      text = await response.text();
    } catch {
      this.render("Not confirmed as stored: the connection failed. Your answer is kept; send it again.", "failed");
      return;
    }
    if (text.length > MAX_RESPONSE_BYTES) {
      this.render("Not confirmed as stored: the reply was too large. Your answer is kept; send it again.", "failed");
      return;
    }
    if (status === 200) {
      this.stored(text, target);
      return;
    }
    this.refused(status, text);
  }

  private stored(text: string, target: StaleTarget | null): void {
    let receipt: Receipt;
    try {
      receipt = JSON.parse(text) as Receipt;
    } catch {
      this.render("Not confirmed as stored: the reply was not understood. Your answer is kept; send it again.", "failed");
      return;
    }
    if (receipt.state !== "stored" || typeof receipt.answer_id !== "string") {
      this.render("Not confirmed as stored: the reply was not understood. Your answer is kept; send it again.", "failed");
      return;
    }
    if (target) {
      this.revision = target.revision;
      if (target.digest) this.digest = target.digest;
    }
    if (!this.amending || !this.original) this.original = receipt.answer_id;
    this.article.dataset.cfAnswerId = this.original;
    this.sent = null;
    this.stale = null;
    this.amending = false;
    this.declining = false;
    this.render("Stored, waiting for agent", "stored");
  }

  private refused(status: number, text: string): void {
    const error = parseServiceError(text);
    const details = (error?.details ?? {}) as Record<string, unknown>;
    switch (error?.error) {
      case "session_closed":
        this.close();
        return;
      case "stale_revision": {
        const revision = Number(details.current_revision);
        const present = details.form_present === true;
        const digest = typeof details.current_form_digest === "string" ? details.current_form_digest : null;
        this.stale = present ? { revision, digest } : null;
        this.gone = !present;
        this.render(present
          ? `Revision ${revision} is current. ${digest === this.digest ? "This question is unchanged there." : "This question changed there; check your answer against it."} Confirm to send it against revision ${revision}.`
          : `This question is not in revision ${revision}. Your draft is kept here; it cannot be sent.`, "stale");
        return;
      }
      case "store_unavailable":
        // No receipt: the same request is safe to send again, and the
        // service stores it once or returns its receipt.
        this.render("Not confirmed as stored: the answer store was unavailable. Your answer is kept; send it again.", "failed");
        return;
      case "request_id_conflict":
        this.sent = null;
        this.render("That request was already used for a different answer. Send again to make a new request.", "editing");
        return;
      case "answer_too_large":
        this.sent = null;
        this.render(`This answer is over the ${MAX_ANSWER_REQUEST_BYTES / 1024} KiB limit; shorten it.`, "editing");
        return;
      case "invalid_answer": {
        this.sent = null;
        const fields = Array.isArray(details.fields) ? (details.fields as FieldError[]) : [];
        this.showErrors(fields);
        this.render("The service refused the answer; check the marked fields.", "editing");
        return;
      }
      case undefined:
        break;
      default:
        this.sent = null;
        this.render(`Not stored: ${error?.message ?? "refused"}`, "editing");
        return;
    }
    this.render(`Not confirmed as stored (${status}). Your answer is kept; send it again.`, "failed");
  }

  // The answer the controls hold now; a decline or cancel carries no values.
  private draft(outcome: Outcome): AnswerDraft {
    if (outcome !== "submit") {
      const reason = outcome === "decline" ? this.declineArea?.querySelector("textarea")?.value ?? "" : "";
      return { outcome, values: {}, rationales: {}, ...(reason.trim() !== "" ? { reason } : {}) };
    }
    const values: Record<string, unknown> = {};
    const rationales: Record<string, string> = {};
    for (const field of this.article.querySelectorAll<HTMLElement>("[data-cf-field]")) {
      const id = field.dataset.cfField ?? "";
      const value = readValue(field, field.dataset.cfFieldKind as FieldKind);
      if (value !== undefined) values[id] = value;
      const rationale = field.querySelector<HTMLTextAreaElement>("[data-cf-rationale-input]")?.value ?? "";
      if (rationale.trim() !== "") rationales[id] = rationale;
    }
    return { outcome, values, rationales };
  }

  private showErrors(errors: readonly FieldError[]): void {
    this.clearFieldError(true);
    for (const error of errors) {
      const field = this.article.querySelector<HTMLElement>(`[data-cf-field="${CSS.escape(error.field)}"]`);
      const line = field?.querySelector<HTMLElement>(".cf-field__error");
      if (!field || !line) continue;
      const rules = this.rules.fields.find((candidate) => candidate.id === error.field);
      line.textContent = errorText(error.code, rules);
      line.hidden = false;
      field.dataset.cfInvalid = "true";
      // Mark only the control the error is about: the reason or the answer.
      const concerned = error.code.startsWith("rationale_") ? "[data-cf-rationale-input]" : "[data-cf-value]";
      field.querySelectorAll<HTMLElement>(concerned).forEach((control) => control.setAttribute("aria-invalid", "true"));
    }
  }

  private clearFieldError(all = false): void {
    if (!all && !this.article.querySelector("[data-cf-invalid]")) return;
    this.article.querySelectorAll<HTMLElement>("[data-cf-invalid]").forEach((field) => {
      delete field.dataset.cfInvalid;
      const line = field.querySelector<HTMLElement>(".cf-field__error");
      if (line) {
        line.hidden = true;
        line.textContent = "";
      }
      field.querySelectorAll("[aria-invalid]").forEach((control) => control.removeAttribute("aria-invalid"));
    });
  }

  private render(message: string, state: FormState = this.state): void {
    if (state === "editing" && this.newer && !this.amending && this.original === null) state = "stale";
    this.state = state;
    this.article.dataset.cfFormState = state;
    this.stateLine.dataset.cfFormState = state;
    this.stateLine.textContent = message;
    const editable = (state === "editing" || state === "stale") && !this.gone;
    this.article.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("[data-cf-value], [data-cf-rationale-input], [data-cf-decline-reason]")
      .forEach((control) => { control.disabled = !editable; });
    const show: Record<Action, boolean> = {
      submit: editable && !this.stale && !this.declining,
      decline: editable && !this.stale,
      cancel: editable && !this.stale && !this.declining,
      resend: state === "failed" && this.sent !== null,
      confirm: state === "stale" && this.stale !== null,
      amend: state === "stored",
    };
    this.buttons.forEach((button, action) => {
      button.hidden = !show[action];
      button.disabled = state === "submitting";
    });
    const decline = this.buttons.get("decline");
    if (decline) decline.textContent = this.declining ? "Send decline" : "Decline to answer";
    const submit = this.buttons.get("submit");
    if (submit) submit.textContent = this.amending ? "Send correction" : "Submit answer";
    if (this.declineArea) this.declineArea.hidden = !(this.declining && editable);
  }
}

// The rules the page checks, as the service rendered them on the form.
function readRules(article: HTMLElement): FormRules {
  const fields = [...article.querySelectorAll<HTMLElement>("[data-cf-field]")].map((field): FieldRules => {
    const data = field.dataset;
    const kind = data.cfFieldKind as FieldKind;
    const number = (value: string | undefined): number | undefined => (value === undefined ? undefined : Number(value));
    const rules: Record<string, unknown> = {
      id: data.cfField ?? "",
      kind,
      rationale: (data.cfRationale as RationaleMode | undefined) ?? "none",
      minLength: number(data.cfMinLength),
      maxLength: number(data.cfMaxLength),
      format: data.cfFormat as TextFormat | undefined,
      minimum: number(data.cfMinimum),
      maximum: number(data.cfMaximum),
      minItems: number(data.cfMinItems),
      maxItems: number(data.cfMaxItems),
      options: kind === "choice" || kind === "choices"
        ? [...field.querySelectorAll<HTMLInputElement>("input[data-cf-value]")].map((input) => input.value)
        : undefined,
    };
    Object.keys(rules).forEach((key) => rules[key] === undefined && delete rules[key]);
    return rules as unknown as FieldRules;
  });
  const required = [...article.querySelectorAll<HTMLElement>("[data-cf-field][data-cf-required]")].map((field) => field.dataset.cfField ?? "");
  return { fields, required };
}

function readValue(field: HTMLElement, kind: FieldKind): unknown {
  const checked = [...field.querySelectorAll<HTMLInputElement>("input[data-cf-value]:checked")].map((input) => input.value);
  switch (kind) {
    case "boolean":
      return checked.length > 0 ? checked[0] === "true" : undefined;
    case "choice":
      return checked[0];
    case "choices":
      return checked.length > 0 ? checked : undefined;
    default: {
      const input = field.querySelector<HTMLInputElement | HTMLTextAreaElement>("[data-cf-value]");
      const text = input?.value ?? "";
      if (text === "") return undefined;
      if (kind === "text") return text;
      const trimmed = text.trim();
      return JSON_NUMBER.test(trimmed) ? Number(trimmed) : text;
    }
  }
}

function errorText(code: FieldError["code"], rules: FieldRules | undefined): string {
  switch (code) {
    case "required": return "Answer this question.";
    case "unknown_field": return "This is not a field of the form.";
    case "wrong_kind": return rules?.kind === "number" || rules?.kind === "integer" ? "Enter a number." : "This answer has the wrong shape.";
    case "too_short": return `Enter at least ${rules?.minLength ?? 0} characters.`;
    case "too_long": return `Enter at most ${rules?.maxLength ?? 0} characters.`;
    case "format": return formatText(rules?.format);
    case "below_minimum": return rules?.minimum !== undefined ? `Enter ${rules.minimum} or more.` : "Enter a larger number.";
    case "above_maximum": return rules?.maximum !== undefined ? `Enter ${rules.maximum} or less.` : "Enter a smaller number.";
    case "not_integer": return "Enter a whole number.";
    case "unknown_option": return "Choose one of the options.";
    case "too_few": return `Choose at least ${rules?.minItems ?? 1}.`;
    case "too_many": return `Choose at most ${rules?.maxItems ?? 1}.`;
    case "rationale_required": return "Say why.";
    case "rationale_not_allowed": return "This question takes no reason.";
  }
}

function formatText(format: TextFormat | undefined): string {
  switch (format) {
    case "email": return "Enter an email address, such as name@example.org.";
    case "uri": return "Enter an absolute address with a scheme, such as https://example.org.";
    case "date": return "Enter a date as YYYY-MM-DD.";
    case "date-time": return "Enter a date and time such as 2026-09-28T14:30:00Z.";
    default: return "Enter a valid value.";
  }
}
