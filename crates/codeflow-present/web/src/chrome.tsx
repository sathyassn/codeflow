import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type {
  AppearanceMode,
  ChromeConfig,
  FeedbackKind,
  PendingFeedback,
  ReviewRequest,
  ReviewResponse,
  ReviewVerdict,
  SessionEvent,
  UtilityTheme,
} from "./contracts";
import { followSessionEvents } from "./events";
import { postJson } from "./http";
import { captureSelection } from "./selection";
import {
  applyAppearance,
  initialAppearance,
  persistAppearance,
  watchSystemMode,
} from "./theme";

interface ChromeProps {
  readonly config: ChromeConfig;
  readonly documentRoot: HTMLElement;
}

interface Section {
  readonly id: string;
  readonly label: string;
  readonly level: 2 | 3;
}

const themeLabels: Readonly<Record<UtilityTheme, string>> = {
  editorial: "Editorial",
  technical: "Technical",
};
const modeLabels: Readonly<Record<AppearanceMode, string>> = {
  system: "Follow system",
  light: "Light",
  dark: "Dark",
};

export function Chrome({ config, documentRoot }: ChromeProps) {
  const [appearance, setAppearance] = useState(initialAppearance);
  const [notes, setNotes] = useState<readonly PendingFeedback[]>([]);
  const [verdict, setVerdict] = useState<ReviewVerdict>("approve_with_notes");
  const [instruction, setInstruction] = useState("");
  const [status, setStatus] = useState("Ready for review.");
  const [busy, setBusy] = useState(false);
  const [activeSection, setActiveSection] = useState<string | null>(null);
  const [eventMessage, setEventMessage] = useState<string | null>(null);
  const [reviewOpen, setReviewOpen] = useState(false);
  const feedbackRef = useRef<HTMLElement>(null);
  const submitAttemptRef = useRef<{ fingerprint: string; eventId: string } | null>(null);
  const sections = useMemo(() => readSections(documentRoot), [documentRoot, config.revision]);

  useEffect(() => {
    applyAppearance(appearance.theme, appearance.mode);
    persistAppearance(appearance.theme, appearance.mode);
    if (appearance.mode !== "system") return undefined;
    return watchSystemMode(() => applyAppearance(appearance.theme, appearance.mode));
  }, [appearance.theme, appearance.mode]);

  useEffect(() => observeSections(documentRoot, setActiveSection), [documentRoot, config.revision]);
  useEffect(
    () => followSessionEvents(`${config.revision}:${config.event_sequence}`, handleEvent, setEventMessage),
    [config.session_id, config.revision, config.event_sequence],
  );
  useEffect(() => {
    const handleAnchor = (event: Event): void => {
      if (busy || !(event.target instanceof Element)) return;
      const button = event.target.closest<HTMLButtonElement>("button[data-anchor-block]");
      const block = button?.closest<HTMLElement>("[data-cf-block-id]");
      const blockId = block?.dataset.cfBlockId;
      if (!button || !block || !blockId || !documentRoot.contains(block)) return;
      if (notes.length >= config.review_limits.max_notes) {
        setStatus(noteLimitMessage(config.review_limits.max_notes));
        setReviewOpen(true);
        return;
      }
      const blockLabel = block.dataset.cfBlockLabel ?? blockId;
      setNotes((current) => [
        ...current,
        {
          client_id: crypto.randomUUID(),
          block_id: blockId,
          block_label: blockLabel,
          kind: "comment",
          body: "",
        },
      ]);
      setReviewOpen(true);
      setVerdict((current) => (current === "approve" ? "approve_with_notes" : current));
      setStatus(`Note added for ${blockLabel}.`);
    };
    documentRoot.addEventListener("click", handleAnchor);
    return () => documentRoot.removeEventListener("click", handleAnchor);
  }, [busy, config.review_limits.max_notes, documentRoot, notes.length]);

  const addNote = (): void => {
    if (busy) return;
    if (notes.length >= config.review_limits.max_notes) {
      setStatus(noteLimitMessage(config.review_limits.max_notes));
      setReviewOpen(true);
      return;
    }
    const selected = captureSelection(documentRoot);
    if (!selected) {
      setStatus("Select text inside one reviewable block, then add a note.");
      return;
    }
    if (selected.selector.exact.length > config.review_limits.max_selector_utf16) {
      setStatus(`Selected text is too long. Select at most ${config.review_limits.max_selector_utf16} characters.`);
      return;
    }
    setNotes((current) => [
      ...current,
      {
        client_id: crypto.randomUUID(),
        block_id: selected.blockId,
        block_label: selected.blockLabel,
        kind: "comment",
        body: "",
        selector: selected.selector,
      },
    ]);
    setReviewOpen(true);
    setVerdict((current) => (current === "approve" ? "approve_with_notes" : current));
    setStatus(`Note added for ${selected.blockLabel}.`);
    requestAnimationFrame(() => {
      feedbackRef.current?.focus();
      feedbackRef.current?.querySelector<HTMLTextAreaElement>("textarea[data-empty='true']")?.focus();
    });
  };

  const submitReview = async (): Promise<void> => {
    const normalizedNotes = notes.map((note) => ({ ...note, body: note.body.trim() }));
    if (normalizedNotes.some((note) => !note.body)) {
      setStatus("Write each pending note before submitting the review.");
      return;
    }
    if (verdict === "request_changes" && !instruction.trim()) {
      setStatus("Request changes needs a clear instruction.");
      return;
    }
    if (
      instruction.length > config.review_limits.max_text_utf16 ||
      normalizedNotes.some((note) => note.body.length > config.review_limits.max_text_utf16)
    ) {
      setStatus(`Each note or review summary is limited to ${config.review_limits.max_text_utf16} characters.`);
      return;
    }
    const payload = {
      session_id: config.session_id,
      revision: config.revision,
      verdict,
      notes: normalizedNotes,
      ...(instruction.trim() ? { instruction: instruction.trim() } : {}),
    };
    const fingerprint = JSON.stringify(payload);
    const previous = submitAttemptRef.current;
    const eventId = previous?.fingerprint === fingerprint ? previous.eventId : crypto.randomUUID();
    submitAttemptRef.current = { fingerprint, eventId };
    const request: ReviewRequest = { event_id: eventId, ...payload };
    if (new TextEncoder().encode(JSON.stringify(request)).byteLength > config.review_limits.max_payload_bytes) {
      setStatus(`This review is too large to submit. Shorten it below ${config.review_limits.max_payload_bytes} bytes.`);
      return;
    }
    setBusy(true);
    setStatus("Submitting review…");
    try {
      const response = await postJson<ReviewResponse>("/app/api/reviews", request);
      setNotes([]);
      setInstruction("");
      submitAttemptRef.current = null;
      setStatus(`${response.state === "duplicate" ? "Review already received" : "Review received"} (${response.event_id}).`);
    } catch {
      setStatus("Review was not submitted. Your pending notes are unchanged.");
    } finally {
      setBusy(false);
    }
  };

  const handleShortcuts = (event: KeyboardEvent): void => {
    if (event.key === "Escape") {
      setReviewOpen(false);
      document.getElementById("cf-review-toggle")?.focus();
      return;
    }
    if (!config.shortcuts_enabled || isEditable(event.target)) return;
    const keymap = config.keymap ?? { next: "j", previous: "k", review: "r", edit: "e" };
    if (![keymap.next, keymap.previous, keymap.review, keymap.edit].includes(event.key)) return;
    event.preventDefault();
    if (event.key === keymap.review) {
      document.getElementById("cf-review-verdict")?.focus();
      return;
    }
    const fields = [...(feedbackRef.current?.querySelectorAll<HTMLElement>("textarea") ?? [])];
    if (event.key === keymap.edit) {
      fields[0]?.focus();
      return;
    }
    const direction = event.key === keymap.next ? 1 : -1;
    const current = fields.indexOf(document.activeElement as HTMLElement);
    const target = fields[Math.max(0, Math.min(fields.length - 1, current + direction))];
    target?.focus();
  };

  function handleEvent(event: SessionEvent): void {
    setEventMessage(event.message ?? null);
    if (event.kind === "revision") {
      setStatus("A newer document revision is available. Finish or discard this review before reloading.");
    } else if (event.kind === "session_closed") {
      setStatus("This review session is closed.");
    }
  }

  return (
    <div class="cf-chrome-frame">
      <a class="cf-skip-link" href="#cf-present-document">Skip to document</a>
      <header class="cf-topbar">
        {config.identity ? (
          <img class="cf-project-identity" src={config.identity.src} alt={config.identity.alt} />
        ) : null}
        <div class="cf-title-group">
          <span class="cf-kicker">Review document</span>
          <strong>{config.title}</strong>
          <span class="cf-revision">Revision {config.revision}</span>
        </div>
        <div class="cf-appearance" aria-label="Appearance">
          <label>
            <span>Theme</span>
            <select
              value={appearance.theme}
              onChange={(event) => setAppearance((current) => ({
                ...current,
                theme: event.currentTarget.value as UtilityTheme,
              }))}
            >
              {(Object.keys(themeLabels) as UtilityTheme[]).map((theme) => (
                <option value={theme}>{themeLabels[theme]}</option>
              ))}
            </select>
          </label>
          <label>
            <span>Mode</span>
            <select
              value={appearance.mode}
              onChange={(event) => setAppearance((current) => ({
                ...current,
                mode: event.currentTarget.value as AppearanceMode,
              }))}
            >
              {(Object.keys(modeLabels) as AppearanceMode[]).map((mode) => (
                <option value={mode}>{modeLabels[mode]}</option>
              ))}
            </select>
          </label>
          <button
            id="cf-review-toggle"
            class="cf-review-toggle"
            type="button"
            aria-controls="cf-feedback-panel"
            aria-expanded={reviewOpen}
            onClick={() => setReviewOpen((open) => !open)}
          >
            Review <span aria-label={`${notes.length} pending notes`}>{notes.length}</span>
          </button>
        </div>
      </header>

      <nav class="cf-section-route" aria-label="Document sections">
        <ol>
          {sections.map((section) => (
            <li data-level={section.level} data-active={section.id === activeSection ? "true" : "false"}>
              <a href={`#${section.id}`} aria-current={section.id === activeSection ? "location" : undefined}>
                {section.label}
              </a>
            </li>
          ))}
        </ol>
      </nav>

      <aside
        id="cf-feedback-panel"
        class="cf-feedback"
        data-open={reviewOpen ? "true" : "false"}
        aria-labelledby="cf-feedback-title"
        ref={feedbackRef}
        tabIndex={-1}
        onKeyDown={handleShortcuts}
      >
        <div class="cf-feedback-heading">
          <div>
            <span class="cf-kicker">Feedback</span>
            <h2 id="cf-feedback-title">Review queue</h2>
          </div>
          <span class="cf-count" aria-label={`${notes.length} pending notes`}>{notes.length}</span>
          <button
            class="cf-feedback-close"
            type="button"
            onClick={() => {
              setReviewOpen(false);
              document.getElementById("cf-review-toggle")?.focus();
            }}
          >
            Close
          </button>
        </div>
        <p class="cf-guidance">Select text within one block, then add a focused note.</p>
        {config.feedback?.items.length || config.feedback?.omitted_older ? (
          <section class="cf-feedback-history" aria-labelledby="cf-feedback-history-title">
            <div class="cf-history-heading">
              <h3 id="cf-feedback-history-title">Earlier feedback</h3>
              {config.feedback.omitted_older ? (
                <span>{config.feedback.omitted_older} older in session history</span>
              ) : null}
            </div>
            <ol>
              {config.feedback.items.map((item) => (
                <li key={item.event_id}>
                  <div class="cf-history-meta">
                    <strong>{item.verdict.replaceAll("_", " ")}</strong>
                    <span>{item.lifecycle}</span>
                    <span>Revision {item.source_revision}</span>
                    <span>Version {item.event_version}</span>
                  </div>
                  {item.instruction ? <p>{item.instruction}</p> : null}
                  {item.notes.length ? (
                    <ul>
                      {item.notes.map((note) => (
                        <li key={note.id} data-anchor-state={note.anchor.state}>
                          <div class="cf-note-heading">
                            <strong>{note.block_label}</strong>
                            <span>{note.anchor.state}</span>
                          </div>
                          {note.quote ? <blockquote>{note.quote}</blockquote> : null}
                          <p>{note.body}</p>
                          {note.anchor.state === "orphaned" ? (
                            <p class="cf-anchor-warning">Unpositioned: {note.anchor.reason}</p>
                          ) : note.anchor.state === "reanchored" ? (
                            <p class="cf-anchor-note">Matched uniquely in this revision.</p>
                          ) : null}
                        </li>
                      ))}
                    </ul>
                  ) : null}
                </li>
              ))}
            </ol>
          </section>
        ) : null}
        <button class="cf-secondary-action" type="button" disabled={busy} onClick={addNote}>
          Add selected text
        </button>

        <ol class="cf-notes" aria-label="Pending review notes">
          {notes.map((note, index) => (
            <li key={note.client_id}>
              <div class="cf-note-heading">
                <a href={`#${encodeURIComponent(note.block_id)}`}>{note.block_label}</a>
                <button
                  type="button"
                  class="cf-text-action"
                  disabled={busy}
                  onClick={() => setNotes((current) => current.filter((item) => item.client_id !== note.client_id))}
                  aria-label={`Remove note for ${note.block_label}`}
                >
                  Remove
                </button>
              </div>
              <blockquote>{note.selector?.exact}</blockquote>
              <label>
                <span>Intent</span>
                <select
                  disabled={busy}
                  value={note.kind}
                  onChange={(event) => updateNote(index, { kind: event.currentTarget.value as FeedbackKind })}
                >
                  <option value="comment">Comment</option>
                  <option value="question">Question</option>
                  <option value="decision">Decision</option>
                  <option value="suggestion">Suggestion</option>
                </select>
              </label>
              <label>
                <span>Note</span>
                <textarea
                  disabled={busy}
                  rows={3}
                  maxLength={config.review_limits.max_text_utf16}
                  value={note.body}
                  data-empty={note.body ? "false" : "true"}
                  onInput={(event) => updateNote(index, { body: event.currentTarget.value })}
                />
              </label>
            </li>
          ))}
        </ol>

        <div class="cf-review-submit">
          <label>
            <span>Verdict</span>
            <select id="cf-review-verdict" disabled={busy} value={verdict} onChange={(event) => setVerdict(event.currentTarget.value as ReviewVerdict)}>
              <option value="approve">Approve</option>
              <option value="approve_with_notes">Approve with notes</option>
              <option value="request_changes">Request changes</option>
            </select>
          </label>
          <label>
            <span>{verdict === "request_changes" ? "Required change" : "Review summary (optional)"}</span>
            <textarea
              rows={3}
              maxLength={config.review_limits.max_text_utf16}
              disabled={busy}
              value={instruction}
              onInput={(event) => setInstruction(event.currentTarget.value)}
            />
          </label>
          <button type="button" class="cf-primary-action" disabled={busy} onClick={() => void submitReview()}>
            {busy ? "Submitting…" : "Submit review"}
          </button>
          <p class="cf-status" role="status" aria-live="polite">{status}</p>
          {eventMessage ? <p class="cf-event-message">{eventMessage}</p> : null}
        </div>
      </aside>
    </div>
  );

  function updateNote(index: number, patch: Partial<Pick<PendingFeedback, "body" | "kind">>): void {
    setNotes((current) => current.map((note, noteIndex) => (noteIndex === index ? { ...note, ...patch } : note)));
  }
}

function noteLimitMessage(limit: number): string {
  return `A review can contain at most ${limit} ${limit === 1 ? "note" : "notes"}.`;
}

function readSections(root: HTMLElement): readonly Section[] {
  return sectionElements(root).map((section) => ({
    id: section.dataset.cfBlockId || section.id,
    label: section.dataset.cfBlockLabel || section.id,
    level: section.parentElement?.closest("section[data-cf-block-id]") ? 3 : 2,
  }));
}

function observeSections(root: HTMLElement, onChange: (id: string) => void): () => void {
  const sections = sectionElements(root);
  if (!sections.length || !("IntersectionObserver" in globalThis)) return () => undefined;
  const visible = new Map<string, number>();
  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) visible.set(entry.target.id, entry.boundingClientRect.top);
        else visible.delete(entry.target.id);
      });
      const first = [...visible.entries()].sort((left, right) => left[1] - right[1])[0];
      if (first) onChange(first[0]);
    },
    { rootMargin: "-15% 0px -70% 0px" },
  );
  sections.forEach((section) => observer.observe(section));
  return () => observer.disconnect();
}

function sectionElements(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>("section[data-cf-block-id][data-cf-block-label]")];
}

function isEditable(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && Boolean(target.closest("input, textarea, select, [contenteditable='true']"));
}
