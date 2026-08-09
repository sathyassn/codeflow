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
import {
  annotatableElements,
  captureDocument,
  captureElement,
  captureRegion,
  captureSelection,
  resolveElement,
  resolveRegion,
} from "./selection";
import type { CapturedTarget, Point } from "./selection";
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

type CaptureMode = "element" | "region" | null;
interface RegionDraft { readonly start: Point; readonly current: Point }

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
  /** Annotate gestures only while Comment mode is armed. */
  const [commentMode, setCommentMode] = useState(false);
  /** Notes rail visibility; can collapse on narrow screens without leaving Comment mode. */
  const [panelOpen, setPanelOpen] = useState(false);
  const [captureMode, setCaptureMode] = useState<CaptureMode>(null);
  const [regionDraft, setRegionDraft] = useState<RegionDraft | null>(null);
  const [markerEpoch, setMarkerEpoch] = useState(0);
  const [hintMode, setHintMode] = useState<"text" | "element" | "region">("element");
  const feedbackRef = useRef<HTMLElement>(null);
  const regionDraftRef = useRef<RegionDraft | null>(null);
  const submitAttemptRef = useRef<{ fingerprint: string; eventId: string } | null>(null);
  const commentModeRef = useRef(false);
  const captureModeRef = useRef<CaptureMode>(null);
  const notesCountRef = useRef(0);
  const sections = useMemo(() => readSections(documentRoot), [documentRoot, config.revision]);

  commentModeRef.current = commentMode;
  captureModeRef.current = captureMode;
  notesCountRef.current = notes.length;

  const armComment = (on: boolean): void => {
    setCommentMode(on);
    commentModeRef.current = on;
    if (!on) {
      setPanelOpen(false);
      setCaptureMode(null);
      captureModeRef.current = null;
      setRegionDraft(null);
      regionDraftRef.current = null;
      const n = notesCountRef.current;
      setStatus(n ? `${n} note${n === 1 ? "" : "s"} queued · Comment off` : "Ready for review.");
    } else {
      setPanelOpen(true);
      setHintMode("element");
      setStatus("Comment on: select text, pick an element, or drag an area.");
    }
  };

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
    documentRoot.dataset.cfCommenting = commentMode ? "true" : "false";
    if (!commentMode) delete documentRoot.dataset.cfCaptureMode;
    return () => {
      delete documentRoot.dataset.cfCommenting;
    };
  }, [commentMode, documentRoot]);

  useEffect(() => {
    const handleAnchor = (event: Event): void => {
      if (!commentMode || busy || captureMode || !(event.target instanceof Element)) return;
      const button = event.target.closest<HTMLButtonElement>("button[data-anchor-block]");
      const block = button?.closest<HTMLElement>("[data-cf-block-id]");
      const blockId = block?.dataset.cfBlockId;
      if (!button || !block || !blockId || !documentRoot.contains(block)) return;
      if (notes.length >= config.review_limits.max_notes) {
        setStatus(noteLimitMessage(config.review_limits.max_notes));
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
          target_summary: `Block: ${blockLabel}`,
        },
      ]);
      setVerdict((current) => (current === "approve" ? "approve_with_notes" : current));
      setStatus(`Note added for ${blockLabel}.`);
    };
    documentRoot.addEventListener("click", handleAnchor);
    return () => documentRoot.removeEventListener("click", handleAnchor);
  }, [busy, captureMode, commentMode, config.review_limits.max_notes, documentRoot, notes.length]);

  useEffect(() => {
    if (captureMode !== "element") return undefined;
    const candidates = annotatableElements(documentRoot);
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const previousTabIndexes = candidates.map((element) => element.getAttribute("tabindex"));
    let activeIndex = Math.max(0, candidates.findIndex((element) => element.contains(previousFocus)));
    const focusCandidate = (index: number): void => {
      activeIndex = (index + candidates.length) % candidates.length;
      candidates.forEach((element, candidateIndex) => {
        element.tabIndex = candidateIndex === activeIndex ? 0 : -1;
      });
      candidates[activeIndex]?.focus();
    };
    const complete = (target: Element): void => {
      const captured = captureElement(documentRoot, target);
      if (captured) addCaptured(captured);
      else setStatus("That element cannot be anchored. Choose content inside one review block.");
      setCaptureMode(null);
    };
    const pick = (event: MouseEvent): void => {
      if (!(event.target instanceof Element)) return;
      event.preventDefault();
      event.stopPropagation();
      complete(event.target);
    };
    const keydown = (event: KeyboardEvent): void => {
      // Escape is owned by the global ladder (capture → comment mode).
      if (event.key === "Escape") return;
      if ((event.key === "Enter" || event.key === " ") && document.activeElement instanceof Element && documentRoot.contains(document.activeElement)) {
        event.preventDefault();
        complete(document.activeElement);
        return;
      }
      const offset = event.key === "ArrowDown" || event.key === "ArrowRight"
        ? 1
        : event.key === "ArrowUp" || event.key === "ArrowLeft" ? -1 : 0;
      if (
        offset !== 0
        && candidates.length > 0
        && document.activeElement instanceof Element
        && documentRoot.contains(document.activeElement)
      ) {
        event.preventDefault();
        focusCandidate(activeIndex + offset);
      }
    };
    documentRoot.dataset.cfCaptureMode = "element";
    documentRoot.addEventListener("click", pick, { capture: true });
    documentRoot.ownerDocument.addEventListener("keydown", keydown, { capture: true });
    if (candidates.length > 0) focusCandidate(activeIndex);
    return () => {
      delete documentRoot.dataset.cfCaptureMode;
      documentRoot.removeEventListener("click", pick, { capture: true });
      documentRoot.ownerDocument.removeEventListener("keydown", keydown, { capture: true });
      candidates.forEach((element, index) => {
        const previous = previousTabIndexes[index] ?? null;
        if (previous === null) element.removeAttribute("tabindex");
        else element.setAttribute("tabindex", previous);
      });
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  }, [captureMode, documentRoot, notes.length]);

  useEffect(() => {
    if (captureMode !== "region") return undefined;
    let pointerId: number | null = null;
    const down = (event: PointerEvent): void => {
      if (event.button !== 0) return;
      pointerId = event.pointerId;
      documentRoot.setPointerCapture(pointerId);
      const point = { x: event.clientX, y: event.clientY };
      const draft = { start: point, current: point };
      regionDraftRef.current = draft;
      setRegionDraft(draft);
      event.preventDefault();
    };
    const move = (event: PointerEvent): void => {
      if (pointerId !== event.pointerId) return;
      const draft = regionDraftRef.current;
      if (draft) {
        const next = { ...draft, current: { x: event.clientX, y: event.clientY } };
        regionDraftRef.current = next;
        setRegionDraft(next);
      }
      event.preventDefault();
    };
    const finish = (event: PointerEvent): void => {
      if (pointerId !== event.pointerId) return;
      const point = { x: event.clientX, y: event.clientY };
      const draft = regionDraftRef.current;
      const captured = draft ? captureRegion(documentRoot, draft.start, point) : null;
      regionDraftRef.current = null;
      setRegionDraft(null);
      if (captured) addCaptured(captured);
      else setStatus("Drag a visible area at least 4×4 pixels inside the document.");
      if (documentRoot.hasPointerCapture(pointerId)) documentRoot.releasePointerCapture(pointerId);
      pointerId = null;
      setCaptureMode(null);
      event.preventDefault();
    };
    documentRoot.dataset.cfCaptureMode = "region";
    documentRoot.addEventListener("pointerdown", down);
    documentRoot.addEventListener("pointermove", move);
    documentRoot.addEventListener("pointerup", finish);
    documentRoot.addEventListener("pointercancel", finish);
    return () => {
      delete documentRoot.dataset.cfCaptureMode;
      documentRoot.removeEventListener("pointerdown", down);
      documentRoot.removeEventListener("pointermove", move);
      documentRoot.removeEventListener("pointerup", finish);
      documentRoot.removeEventListener("pointercancel", finish);
      regionDraftRef.current = null;
      setRegionDraft(null);
    };
  }, [captureMode, documentRoot, notes.length]);

  useEffect(() => {
    const refresh = (): void => setMarkerEpoch((value) => value + 1);
    addEventListener("scroll", refresh, { passive: true });
    addEventListener("resize", refresh, { passive: true });
    return () => {
      removeEventListener("scroll", refresh);
      removeEventListener("resize", refresh);
    };
  }, []);

  const addNote = (): void => {
    if (busy) return;
    if (!commentMode) armComment(true);
    if (notes.length >= config.review_limits.max_notes) {
      setStatus(noteLimitMessage(config.review_limits.max_notes));
      return;
    }
    const selected = captureSelection(documentRoot);
    if (!selected?.selector) {
      setStatus("Select text inside one reviewable block, then add a note.");
      return;
    }
    if (selected.selector.exact.length > config.review_limits.max_selector_utf16) {
      setStatus(`Selected text is too long. Select at most ${config.review_limits.max_selector_utf16} characters.`);
      return;
    }
    setHintMode("text");
    addCaptured(selected);
  };

  const submitReview = async (): Promise<void> => {
    const normalizedNotes = notes.map(({ target_summary: _summary, ...note }) => ({ ...note, body: note.body.trim() }));
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
    // Escape / C are owned by the document-level ladder.
    if (event.key === "Escape" || event.key === "c" || event.key === "C") return;
    if (!config.shortcuts_enabled || isEditable(event.target) || !commentModeRef.current) return;
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

  // Present session window only — single Esc / C ladder (never jumps other apps).
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        // Capture tools first — even when focus is in a note textarea.
        if (captureModeRef.current) {
          event.preventDefault();
          setCaptureMode(null);
          captureModeRef.current = null;
          setHintMode("element");
          return;
        }
        if (isEditable(event.target)) return;
        if (commentModeRef.current) {
          event.preventDefault();
          armComment(false);
          document.getElementById("cf-comment-toggle")?.focus();
        }
        return;
      }
      if (isEditable(event.target) || !config.shortcuts_enabled) return;
      if ((event.key === "c" || event.key === "C") && !event.metaKey && !event.ctrlKey && !event.altKey) {
        event.preventDefault();
        armComment(!commentModeRef.current);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [config.shortcuts_enabled]);

  function handleEvent(event: SessionEvent): void {
    setEventMessage(event.message ?? null);
    if (event.kind === "revision") {
      setStatus("A newer document revision is available. Finish or discard this review before reloading.");
    } else if (event.kind === "session_closed") {
      setStatus("This review session is closed.");
    }
  }

  const railVisible = commentMode && panelOpen;

  return (
    <div class="cf-chrome-frame" data-commenting={commentMode ? "true" : "false"} data-rail={railVisible ? "open" : "closed"}>
      {regionDraft ? <div class="cf-region-draft" style={regionDraftStyle(regionDraft)} aria-hidden="true" /> : null}
      {commentMode
        ? notes.map((note, index) => {
            const rect = targetRect(documentRoot, note, markerEpoch);
            return rect ? (
              <div key={note.client_id} class="cf-region-marker" style={regionMarkerStyle(rect)} aria-hidden="true">
                <span>{index + 1}</span>
              </div>
            ) : null;
          })
        : null}
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
            id="cf-comment-toggle"
            class="cf-comment-toggle"
            type="button"
            data-testid="comment-btn"
            aria-controls="cf-feedback-panel"
            aria-expanded={railVisible}
            aria-pressed={commentMode}
            title={commentMode ? "Exit Comment mode (C or Esc)" : "Comment mode (C)"}
            onClick={() => {
              if (commentMode && !panelOpen) setPanelOpen(true);
              else armComment(!commentMode);
            }}
          >
            Comment
            {notes.length > 0 ? (
              <span class="cf-comment-count" aria-label={`${notes.length} pending notes`}>{notes.length}</span>
            ) : null}
          </button>
        </div>
      </header>

      {commentMode ? (
        <div class="cf-mode-strip" data-testid="comment-hint" role="status">
          <span class="cf-mode-chip" data-active={hintMode === "text" ? "true" : "false"}><b>Text</b> select</span>
          <span class="cf-mode-chip" data-active={hintMode === "element" ? "true" : "false"}><b>Click</b> element</span>
          <span class="cf-mode-chip" data-active={hintMode === "region" ? "true" : "false"}><b>Drag</b> area</span>
          <span class="cf-mode-esc">C toggles · Esc exits</span>
        </div>
      ) : null}

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
        data-open={railVisible ? "true" : "false"}
        aria-labelledby="cf-feedback-title"
        hidden={!railVisible}
        ref={feedbackRef}
        tabIndex={-1}
        onKeyDown={handleShortcuts}
      >
        <div class="cf-feedback-heading">
          <div>
            <span class="cf-kicker">Feedback</span>
            <h2 id="cf-feedback-title">Notes</h2>
          </div>
          <span class="cf-count" aria-label={`${notes.length} pending notes`}>{notes.length}</span>
          <button
            class="cf-feedback-close"
            type="button"
            onClick={() => {
              // Collapse rail without leaving Comment mode (narrow layouts / capture over document).
              setPanelOpen(false);
              document.getElementById("cf-comment-toggle")?.focus();
            }}
          >
            Close
          </button>
        </div>
        <p class="cf-guidance">Comment on selected text, one semantic element, a dragged visual area, one block, or the whole document.</p>
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
        <div class="cf-capture-tools" aria-label="Choose feedback target">
          <button class="cf-secondary-action" type="button" disabled={busy} onClick={addNote}>Add selected text</button>
          <button
            class="cf-secondary-action"
            type="button"
            disabled={busy}
            aria-pressed={captureMode === "element"}
            onClick={() => {
              setHintMode("element");
              setCaptureMode(captureMode === "element" ? null : "element");
            }}
          >
            Pick element
          </button>
          <button
            class="cf-secondary-action"
            type="button"
            disabled={busy}
            aria-pressed={captureMode === "region"}
            onClick={() => {
              setHintMode("region");
              setCaptureMode(captureMode === "region" ? null : "region");
            }}
          >
            Select area
          </button>
          <button class="cf-secondary-action" type="button" disabled={busy} onClick={() => {
            const captured = captureDocument(documentRoot);
            if (captured) addCaptured(captured);
            else setStatus("The document is not ready for whole-document feedback.");
          }}>Whole document</button>
        </div>

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
              <blockquote>{note.target_summary ?? note.selector?.exact ?? `Block: ${note.block_label}`}</blockquote>
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
                  <option value="adjustment">Adjustment</option>
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

  function addCaptured(captured: CapturedTarget): void {
    if (notes.length >= config.review_limits.max_notes) {
      setStatus(noteLimitMessage(config.review_limits.max_notes));
      if (!commentMode) armComment(true);
      return;
    }
    if (!commentMode) armComment(true);
    else setPanelOpen(true);
    setNotes((current) => [...current, {
      client_id: crypto.randomUUID(),
      block_id: captured.blockId,
      block_label: captured.blockLabel,
      kind: "comment",
      body: "",
      ...(captured.selector ? { selector: captured.selector } : {}),
      ...(captured.element_selector ? { element_selector: captured.element_selector } : {}),
      ...(captured.region_selector ? { region_selector: captured.region_selector } : {}),
      target_summary: captured.summary,
    }]);
    setVerdict((current) => current === "approve" ? "approve_with_notes" : current);
    setStatus(`Note added for ${captured.summary}.`);
    requestAnimationFrame(() => feedbackRef.current?.querySelector<HTMLTextAreaElement>("textarea[data-empty='true']")?.focus());
  }
}

function regionDraftStyle(draft: RegionDraft): string {
  const left = Math.min(draft.start.x, draft.current.x);
  const top = Math.min(draft.start.y, draft.current.y);
  return `left:${left}px;top:${top}px;width:${Math.abs(draft.current.x - draft.start.x)}px;height:${Math.abs(draft.current.y - draft.start.y)}px`;
}

function regionMarkerStyle(rect: DOMRect): string {
  return `left:${rect.left}px;top:${rect.top}px;width:${rect.width}px;height:${rect.height}px`;
}

function targetRect(documentRoot: HTMLElement, note: PendingFeedback, _markerEpoch: number): DOMRect | null {
  if (note.region_selector) return resolveRegion(documentRoot, note.region_selector);
  if (note.element_selector) {
    return resolveElement(documentRoot, note.block_id, note.element_selector)?.getBoundingClientRect() ?? null;
  }
  return documentRoot
    .querySelector<HTMLElement>(`[data-cf-block-id="${CSS.escape(note.block_id)}"]`)
    ?.getBoundingClientRect() ?? null;
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
