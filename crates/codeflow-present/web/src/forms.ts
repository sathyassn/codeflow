// The form runtime (SPC-014 B6). The service renders each `form` block and
// v2 `decision` with its controls, rules and digest; this module reads them,
// checks the answer with the rules the server shares, and sends it to
// `POST /app/api/answers` under the v1 request checks. The draft is the
// controls themselves: it lives in page memory for the life of the page and
// is never written to any browser storage. Annotating a form never changes a
// value and never sends.

import { parseServiceError, REQUEST_HEADER, type AnswerDelivery, type AnswerStateEntry, type ChromeConfig } from "./contracts";
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
/** The chrome forwards the poll's answer states under this name (SPC-014 B8). */
export const ANSWER_STATE_EVENT = "cf-present:answer-state";
export type AnswerStateDetail = readonly AnswerStateEntry[];

type FormState = "editing" | "submitting" | "stored" | "delivered" | "acknowledged" | "failed" | "stale" | "changed" | "closed";

const STORED_TEXT = "Stored, waiting for agent";
// After "stored": the agent's read delivers the answer, then it acknowledges it.
const DELIVERY_TEXT: Readonly<Record<Exclude<AnswerDelivery, "pending">, string>> = {
  delivered: "Delivered to agent",
  acknowledged: "Acknowledged by agent",
};
const DELIVERY_ORDER: Readonly<Record<AnswerDelivery, number>> = { pending: 0, delivered: 1, acknowledged: 2 };
type Action = "submit" | "decline" | "cancel" | "resend" | "confirm" | "amend";

const ANSWERS_PATH = "/app/api/answers";
const MAX_RESPONSE_BYTES = 64 * 1024;
const JSON_NUMBER = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/u;
const CONTROLS = "input, textarea, select, button";
const CLOSED_TEXT = "This session is closed. Your draft is kept here; nothing can be sent.";

interface Receipt {
  readonly answer_id: string;
  readonly revision: number;
  readonly state: string;
}

// One request as sent: a resend after an uncertain outcome sends these bytes
// again, with the same request id and against the same revision.
interface Sent {
  readonly body: string;
  readonly outcome: Outcome;
  readonly target: StaleTarget | null;
}

interface StaleTarget {
  readonly revision: number;
  readonly digest: string | null;
}

export function enhanceForms(root: HTMLElement, config: ChromeConfig): void {
  // Every answer state the poll reported, never moving back: a state may
  // arrive before the receipt of the answer it names.
  const delivery = new Map<string, AnswerDelivery>();
  const forms = [...root.querySelectorAll<HTMLElement>("article[data-cf-form]")]
    .filter((article) => article.querySelector("[data-cf-form-action]"))
    .map((article) => new FormController(article, config, root, delivery));
  if (forms.length === 0) return;
  guardAnnotation(root);
  root.addEventListener(ANSWER_STATE_EVENT, (event) => {
    for (const entry of (event as CustomEvent<AnswerStateDetail>).detail) {
      const known = delivery.get(entry.answer_id) ?? "pending";
      if (DELIVERY_ORDER[entry.status] > DELIVERY_ORDER[known]) delivery.set(entry.answer_id, entry.status);
    }
    for (const form of forms) form.showDelivery();
  });
  // The chrome forwards the poll's events here; a form refused with
  // session_closed reports it here too, so every form and the chrome close.
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
  // The answer or correction this form stored last: its delivery is shown.
  private latest: string | null = null;
  private amending = false;
  private declining = false;
  private sent: Sent | null = null;
  private stale: StaleTarget | null = null;
  private gone = false;
  private newer = false;
  // Closure is latched: no later reply or action reopens the form.
  private closed = false;
  private kept: HTMLElement | null = null;

  public constructor(
    private readonly article: HTMLElement,
    private readonly config: ChromeConfig,
    private readonly root: HTMLElement,
    private readonly delivery: ReadonlyMap<string, AnswerDelivery>,
  ) {
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
    // An answer stored before this page loaded: the service renders it with
    // its state, so a reload keeps showing stored, delivered or acknowledged.
    const answered = article.dataset.cfAnswerState;
    const original = article.dataset.cfAnswerId;
    const latest = article.dataset.cfLatestAnswerId;
    if (original && latest && (answered === "stored" || answered === "delivered" || answered === "acknowledged")) {
      this.original = original;
      this.latest = latest;
      this.render(answered === "stored" ? STORED_TEXT : DELIVERY_TEXT[answered], answered);
    } else {
      this.render("");
    }
  }

  public newerRevision(): void {
    this.newer = true;
    if (this.state === "editing" || this.state === "failed") {
      this.render(this.state === "failed"
        ? "A newer revision exists. Your answer is kept; send it again, and the service checks it against the current revision."
        : "A newer revision exists. Your draft is kept; sending it checks it against the current revision.", this.state === "failed" ? "failed" : "stale");
    }
  }

  // Stored, then delivered, then acknowledged: only a stored answer moves on,
  // and never back.
  public showDelivery(): void {
    if (this.latest === null || this.closed) return;
    const status = this.delivery.get(this.latest) ?? "pending";
    if (status === "pending") return;
    const shown = this.state === "stored" ? 0 : this.state === "delivered" ? 1 : this.state === "acknowledged" ? 2 : -1;
    if (shown < 0 || DELIVERY_ORDER[status] <= shown) return;
    this.render(DELIVERY_TEXT[status], status);
  }

  // An answered form keeps its answer's state words before the closed
  // sentence; any other form keeps its draft, read only.
  public close(): void {
    if (this.closed) return;
    this.closed = true;
    const state = this.state;
    if (state === "stored" || state === "delivered" || state === "acknowledged") this.render(stateText(state), state);
    else this.render(CLOSED_TEXT, "closed");
  }

  private act(action: Action): void {
    if (this.closed || this.article.closest("[data-cf-commenting='true']")) return;
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
        if (this.sent) void this.post(this.sent);
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
    if (this.closed) return;
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
    await this.post({ body, outcome, target });
  }

  // One request: a new one, or the same bytes again for a resend, so the
  // service answers a resend with the original receipt (B6).
  private async post(sent: Sent): Promise<void> {
    if (this.closed) return;
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
      this.stored(text, sent.target);
      return;
    }
    this.refused(status, text, sent.target);
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
    this.latest = receipt.answer_id;
    this.sent = null;
    this.stale = null;
    this.amending = false;
    this.declining = false;
    this.render(STORED_TEXT, "stored");
    this.showDelivery();
  }

  private refused(status: number, text: string, target: StaleTarget | null): void {
    const error = parseServiceError(text);
    const details = (error?.details ?? {}) as Record<string, unknown>;
    switch (error?.error) {
      case "session_closed":
        this.close();
        this.root.dispatchEvent(new CustomEvent<SessionEventDetail>(SESSION_EVENT, { detail: "session_closed" }));
        return;
      case "stale_revision": {
        const revision = Number(details.current_revision);
        const present = details.form_present === true;
        const digest = typeof details.current_form_digest === "string" ? details.current_form_digest : null;
        this.stale = null;
        if (!present) {
          this.gone = true;
          this.render(`This question is not in revision ${revision}. Your draft is kept here; it cannot be sent.`, "stale");
        } else if (digest !== this.digest) {
          // The digest covers the whole block: a different one means the
          // reviewer has not seen the question as it is now.
          this.showChanged(revision);
        } else {
          this.stale = { revision, digest };
          this.render(`Revision ${revision} is current. This question is unchanged there. Confirm to send it against revision ${revision}.`, "stale");
        }
        return;
      }
      case "store_unavailable":
        // No receipt: the same request is safe to send again, and the
        // service stores it once or returns its receipt.
        this.render("Not confirmed as stored: the answer store was unavailable. Your answer is kept; send it again.", "failed");
        return;
      case "request_id_conflict":
        this.refusedDefinitively(target);
        this.render("That request was already used for a different answer. Send again to make a new request.", "editing");
        return;
      case "answer_exists": {
        // One form holds one original answer (SPC-014 B6): another page
        // stored it. Show that answer with its state and offer a
        // correction; the draft stays for it, unsent.
        if (typeof details.answer_id !== "string") break;
        this.refusedDefinitively(target);
        const answered: AnswerDelivery = details.state === "delivered" || details.state === "acknowledged" ? details.state : "pending";
        this.original = details.answer_id;
        this.latest = details.answer_id;
        this.article.dataset.cfAnswerId = details.answer_id;
        this.amending = false;
        this.declining = false;
        const words = answered === "pending" ? STORED_TEXT : DELIVERY_TEXT[answered];
        this.render(`${words}. This question was answered on another page; your draft was not sent.`, answered === "pending" ? "stored" : answered);
        return;
      }
      case "answer_too_large":
        this.refusedDefinitively(target);
        this.render(`This answer is over the ${MAX_ANSWER_REQUEST_BYTES / 1024} KiB limit; shorten it.`, "editing");
        return;
      case "invalid_answer": {
        this.refusedDefinitively(target);
        const fields = Array.isArray(details.fields) ? (details.fields as FieldError[]) : [];
        this.showErrors(fields);
        this.render("The service refused the answer; check the marked fields.", "editing");
        return;
      }
      case undefined:
        break;
      default:
        this.refusedDefinitively(target);
        this.render(`Not stored: ${error?.message ?? "refused"}`, "editing");
        return;
    }
    this.render(`Not confirmed as stored (${status}). Your answer is kept; send it again.`, "failed");
  }

  // A refusal that stored nothing for certain ends that request: the next
  // send is a new request. A refused confirmation settles its revision, so
  // the form edits against the current revision again, with its send.
  private refusedDefinitively(target: StaleTarget | null): void {
    this.sent = null;
    if (!target) return;
    this.revision = target.revision;
    if (target.digest) this.digest = target.digest;
    this.stale = null;
    this.newer = false;
  }

  // The question changed at the current revision: the draft stays, read
  // only, and is shown as text to enter again after a reload.
  private showChanged(revision: number): void {
    this.gone = true;
    if (!this.kept) {
      this.kept = document.createElement("ul");
      this.kept.className = "cf-form__kept";
      this.kept.dataset.cfReviewSkip = "";
      this.kept.setAttribute("aria-label", "Your draft");
      this.article.querySelector(".cf-form__actions")?.after(this.kept);
    }
    this.kept.replaceChildren(...this.draftLines().map((line) => {
      const item = document.createElement("li");
      item.textContent = line;
      return item;
    }));
    // A reload empties page memory, so the list is gone after it: copy first.
    this.render(`This question changed in revision ${revision}. Copy your draft from the list below, then reload the page to answer the current question.`, "changed");
  }

  // The draft as the reviewer entered it, one line per value or reason.
  private draftLines(): string[] {
    const outcome = this.sent?.outcome ?? (this.declining ? "decline" : "submit");
    if (outcome === "cancel") return ["Dismissed for now."];
    if (outcome === "decline") {
      const reason = this.declineArea?.querySelector("textarea")?.value.trim() ?? "";
      return [reason ? `Declined to answer: ${reason}` : "Declined to answer."];
    }
    const lines: string[] = [];
    for (const field of this.article.querySelectorAll<HTMLElement>("[data-cf-field]")) {
      const label = fieldLabel(field);
      const kind = field.dataset.cfFieldKind as FieldKind;
      const shown = kind === "boolean" || kind === "choice" || kind === "choices"
        ? [...field.querySelectorAll<HTMLInputElement>("input[data-cf-value]:checked")]
          .map((input) => input.closest(".cf-option")?.querySelector(".cf-option__label")?.textContent?.trim() ?? input.value)
          .join(", ")
        : field.querySelector<HTMLInputElement | HTMLTextAreaElement>("[data-cf-value]")?.value ?? "";
      if (shown !== "") lines.push(`${label}: ${shown}`);
      const rationale = field.querySelector<HTMLTextAreaElement>("[data-cf-rationale-input]")?.value.trim() ?? "";
      if (rationale !== "") lines.push(`${label}, why: ${rationale}`);
    }
    return lines.length > 0 ? lines : ["No answer entered."];
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
    let storedLate: string | null = null;
    if (this.closed && state !== "closed") {
      // A reply that lands after closure may still confirm a receipt, but
      // the form stays closed. An answer's state reads first, in the
      // stored colour.
      if (state === "stored" || state === "delivered" || state === "acknowledged") {
        storedLate = stateText(state);
        message = "This session is now closed; nothing more can be sent.";
      } else {
        message = CLOSED_TEXT;
      }
      state = "closed";
    }
    if (state === "editing" && this.newer && !this.amending && this.original === null) state = "stale";
    this.state = state;
    this.article.dataset.cfFormState = state;
    this.stateLine.dataset.cfFormState = state;
    if (storedLate) {
      // One status region: the stored part in the stored colour, then the
      // closed sentence.
      const stored = document.createElement("span");
      stored.className = "cf-form__state-stored";
      stored.textContent = storedLate;
      this.stateLine.replaceChildren(stored, `. ${message}`);
    } else {
      this.stateLine.textContent = message;
    }
    const editable = (state === "editing" || state === "stale") && !this.gone;
    this.article.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("[data-cf-value], [data-cf-rationale-input], [data-cf-decline-reason]")
      .forEach((control) => { control.disabled = !editable; });
    const show: Record<Action, boolean> = {
      submit: editable && !this.stale && !this.declining,
      decline: editable && !this.stale,
      cancel: editable && !this.stale && !this.declining,
      resend: state === "failed" && this.sent !== null,
      confirm: state === "stale" && this.stale !== null,
      amend: state === "stored" || state === "delivered" || state === "acknowledged",
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

// The words for an answer's state, as the form shows them.
function stateText(state: "stored" | "delivered" | "acknowledged"): string {
  return state === "stored" ? STORED_TEXT : DELIVERY_TEXT[state];
}

// The field's label as the reviewer read it, without the "(required)" flag.
function fieldLabel(field: HTMLElement): string {
  const label = field.querySelector<HTMLElement>(".cf-field__label");
  if (!label) return field.dataset.cfField ?? "";
  const copy = label.cloneNode(true) as HTMLElement;
  copy.querySelectorAll(".cf-field__flag").forEach((flag) => flag.remove());
  return copy.textContent?.trim() ?? "";
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
