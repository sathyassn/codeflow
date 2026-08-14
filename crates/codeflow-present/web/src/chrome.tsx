/**
 * Present review chrome — utility presentation system Comment SM + craft.
 * Rust owns #cf-present-document; this Preact tree owns chrome only (ADR-0049).
 * Feedback still posts /app/api/reviews for harness-agnostic delivery.
 */
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
type HintMode = "text" | "element" | "region";
interface RegionDraft {
  readonly start: Point;
  readonly current: Point;
}
interface PendingPin {
  readonly captured: CapturedTarget;
  readonly clientX: number;
  readonly clientY: number;
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

const SPEECH_PATH =
  "M4 3.5A3.5 3.5 0 0 1 7.5 0h9A3.5 3.5 0 0 1 20 3.5v8A3.5 3.5 0 0 1 16.5 15H11l-4.2 3.4c-.7.55-1.8.05-1.8-.85V15H7.5A3.5 3.5 0 0 1 4 11.5v-8Z";

export function Chrome({ config, documentRoot }: ChromeProps) {
  const [appearance, setAppearance] = useState(initialAppearance);
  const [notes, setNotes] = useState<readonly PendingFeedback[]>([]);
  const [verdict, setVerdict] = useState<ReviewVerdict>("approve_with_notes");
  const [instruction, setInstruction] = useState("");
  const [status, setStatus] = useState("Ready for review.");
  const [busy, setBusy] = useState(false);
  const [activeSection, setActiveSection] = useState<string | null>(null);
  const [eventMessage, setEventMessage] = useState<string | null>(null);
  const [commentMode, setCommentMode] = useState(false);
  const [panelOpen, setPanelOpen] = useState(false);
  const [captureMode, setCaptureMode] = useState<CaptureMode>(null);
  const [regionDraft, setRegionDraft] = useState<RegionDraft | null>(null);
  const [markerEpoch, setMarkerEpoch] = useState(0);
  const [hintMode, setHintMode] = useState<HintMode>("element");
  const [pendingPin, setPendingPin] = useState<PendingPin | null>(null);
  const [composerOpen, setComposerOpen] = useState(false);
  const [composerBody, setComposerBody] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);

  const dockRef = useRef<HTMLElement>(null);
  const composerTextRef = useRef<HTMLTextAreaElement>(null);
  const regionDraftRef = useRef<RegionDraft | null>(null);
  const submitAttemptRef = useRef<{ fingerprint: string; eventId: string } | null>(null);
  const commentModeRef = useRef(false);
  const captureModeRef = useRef<CaptureMode>(null);
  const notesCountRef = useRef(0);
  const dragGestureRef = useRef<{ x0: number; y0: number; moved: boolean } | null>(null);
  const lastSelectionRef = useRef<CapturedTarget | null>(null);
  const pinCaptureRef = useRef<(c: CapturedTarget, x: number, y: number, o?: { openComposer?: boolean }) => void>(() => undefined);

  const sections = useMemo(() => readSections(documentRoot), [documentRoot, config.revision]);
  commentModeRef.current = commentMode;
  captureModeRef.current = captureMode;
  notesCountRef.current = notes.length;
  const railVisible = commentMode && panelOpen;

  const armComment = (on: boolean): void => {
    setCommentMode(on);
    commentModeRef.current = on;
    if (!on) {
      setPanelOpen(false);
      setCaptureMode(null);
      captureModeRef.current = null;
      setRegionDraft(null);
      regionDraftRef.current = null;
      setPendingPin(null);
      setComposerOpen(false);
      setComposerBody("");
      setEditingId(null);
      setStatus(notesCountRef.current ? `${notesCountRef.current} note${notesCountRef.current === 1 ? "" : "s"} queued · Comment off` : "Ready for review.");
    } else {
      setPanelOpen(true);
      setHintMode("element");
      setStatus("Comment on: select text, click a figure, or drag an area.");
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

  /* ─── Explicit capture modes (a11y / advanced tools) ─── */
  useEffect(() => {
    if (captureMode !== "element") return undefined;
    const candidates = annotatableElements(documentRoot);
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const previousTabIndexes = candidates.map((el) => el.getAttribute("tabindex"));
    let activeIndex = Math.max(0, candidates.findIndex((el) => el.contains(previousFocus)));
    const focusCandidate = (index: number): void => {
      activeIndex = (index + candidates.length) % candidates.length;
      candidates.forEach((el, i) => {
        el.tabIndex = i === activeIndex ? 0 : -1;
      });
      candidates[activeIndex]?.focus();
    };
    const complete = (target: Element): void => {
      const captured = captureElement(documentRoot, target);
      if (captured) {
        const r = target.getBoundingClientRect();
        pinCaptureRef.current(captured, r.left, r.top, { openComposer: true });
      } else setStatus("That element cannot be anchored. Choose content inside one review block.");
      setCaptureMode(null);
      captureModeRef.current = null;
    };
    const pick = (event: MouseEvent): void => {
      if (!(event.target instanceof Element)) return;
      if (event.target.closest("button[data-anchor-block]")) return;
      event.preventDefault();
      event.stopPropagation();
      complete(event.target);
    };
    const keydown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") return;
      if ((event.key === "Enter" || event.key === " ") && document.activeElement instanceof Element && documentRoot.contains(document.activeElement)) {
        event.preventDefault();
        complete(document.activeElement);
        return;
      }
      const offset =
        event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : event.key === "ArrowUp" || event.key === "ArrowLeft" ? -1 : 0;
      if (offset !== 0 && candidates.length > 0 && document.activeElement instanceof Element && documentRoot.contains(document.activeElement)) {
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
      candidates.forEach((el, i) => {
        const prev = previousTabIndexes[i] ?? null;
        if (prev === null) el.removeAttribute("tabindex");
        else el.setAttribute("tabindex", prev);
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
      if (captured) pinCaptureRef.current(captured, point.x, point.y, { openComposer: true });
      else setStatus("Drag a visible area at least 4×4 pixels inside the document.");
      if (documentRoot.hasPointerCapture(pointerId)) documentRoot.releasePointerCapture(pointerId);
      pointerId = null;
      setCaptureMode(null);
      captureModeRef.current = null;
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

  /* ─── Pass10 default gestures while Comment is armed (no tool forced) ─── */
  useEffect(() => {
    if (!commentMode || captureMode || busy || composerOpen) return undefined;

    const onSelection = (): void => {
      if (!commentModeRef.current || captureModeRef.current) return;
      const selected = captureSelection(documentRoot);
      // Always stash for the Tools "Add selected text" path (button focus clears live selection).
      if (selected?.selector) lastSelectionRef.current = selected;
      if (!selected?.selector) return;
      if (selected.selector.exact.length > config.review_limits.max_selector_utf16) {
        setStatus(`Selected text is too long. Select at most ${config.review_limits.max_selector_utf16} characters.`);
        return;
      }
      // Auto-pin on selection only when there is no open composer/float already.
      // Tools and explicit gestures still drive the full float/composer path.
      setHintMode("text");
    };

    const onPointerDown = (event: PointerEvent): void => {
      if (event.button !== 0 || !(event.target instanceof Element)) return;
      if (!documentRoot.contains(event.target)) return;
      if (event.target.closest("button, a, input, textarea, select")) return;
      dragGestureRef.current = { x0: event.clientX, y0: event.clientY, moved: false };
    };
    const onPointerMove = (event: PointerEvent): void => {
      const g = dragGestureRef.current;
      if (!g) return;
      if (Math.hypot(event.clientX - g.x0, event.clientY - g.y0) > 8) {
        g.moved = true;
        setHintMode("region");
        const draft = {
          start: { x: g.x0, y: g.y0 },
          current: { x: event.clientX, y: event.clientY },
        };
        regionDraftRef.current = draft;
        setRegionDraft(draft);
      }
    };
    const onPointerUp = (event: PointerEvent): void => {
      const g = dragGestureRef.current;
      dragGestureRef.current = null;
      if (!g) return;
      if (g.moved) {
        window.getSelection()?.removeAllRanges();
        const captured = captureRegion(documentRoot, { x: g.x0, y: g.y0 }, { x: event.clientX, y: event.clientY });
        regionDraftRef.current = null;
        setRegionDraft(null);
        if (captured) {
          setHintMode("region");
          pinCapture(captured, event.clientX, event.clientY, { openComposer: false });
        }
        return;
      }
      regionDraftRef.current = null;
      setRegionDraft(null);
      // Bare click → element pin (if no meaningful selection)
      const sel = window.getSelection();
      if (sel && !sel.isCollapsed && String(sel).trim().length >= 2) return;
      if (!(event.target instanceof Element)) return;
      setHintMode("element");
      const captured = captureElement(documentRoot, event.target);
      if (captured) pinCapture(captured, event.clientX, event.clientY, { openComposer: false });
    };

    document.addEventListener("selectionchange", onSelection);
    documentRoot.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    return () => {
      document.removeEventListener("selectionchange", onSelection);
      documentRoot.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
    };
  }, [busy, captureMode, commentMode, composerOpen, config.review_limits.max_selector_utf16, documentRoot]);

  useEffect(() => {
    const refresh = (): void => setMarkerEpoch((v) => v + 1);
    addEventListener("scroll", refresh, { passive: true });
    addEventListener("resize", refresh, { passive: true });
    return () => {
      removeEventListener("scroll", refresh);
      removeEventListener("resize", refresh);
    };
  }, []);

  useEffect(() => {
    if (composerOpen) requestAnimationFrame(() => composerTextRef.current?.focus());
  }, [composerOpen]);

  /* ─── Pin → float → composer (qualified Comment flow) ─── */
  function pinCapture(captured: CapturedTarget, clientX: number, clientY: number, opts?: { openComposer?: boolean }): void {
    if (notesCountRef.current >= config.review_limits.max_notes) {
      setStatus(noteLimitMessage(config.review_limits.max_notes));
      if (!commentModeRef.current) armComment(true);
      return;
    }
    if (!commentModeRef.current) armComment(true);
    else setPanelOpen(true);
    setPendingPin({ captured, clientX, clientY });
    setComposerBody("");
    setEditingId(null);
    // Default open composer (tools + a11y). Gestures pass openComposer:false for float-first.
    setComposerOpen(opts?.openComposer !== false);
    setStatus(`Pinned: ${captured.summary}`);
  }
  pinCaptureRef.current = pinCapture;

  function openComposerFromFloat(): void {
    if (!pendingPin) return;
    setComposerOpen(true);
  }

  function saveComposer(): void {
    const body = composerBody.trim();
    if (!body) {
      setStatus("Write a note before saving.");
      return;
    }
    if (body.length > config.review_limits.max_text_utf16) {
      setStatus(`Each note is limited to ${config.review_limits.max_text_utf16} characters.`);
      return;
    }
    if (editingId) {
      setNotes((current) => current.map((n) => (n.client_id === editingId ? { ...n, body } : n)));
      setStatus("Note updated.");
    } else if (pendingPin) {
      const c = pendingPin.captured;
      setNotes((current) => [
        ...current,
        {
          client_id: crypto.randomUUID(),
          block_id: c.blockId,
          block_label: c.blockLabel,
          kind: "comment" as FeedbackKind,
          body,
          ...(c.selector ? { selector: c.selector } : {}),
          ...(c.element_selector ? { element_selector: c.element_selector } : {}),
          ...(c.region_selector ? { region_selector: c.region_selector } : {}),
          target_summary: c.summary,
        },
      ]);
      setVerdict((v) => (v === "approve" ? "approve_with_notes" : v));
      setStatus(`Note saved for ${c.summary}.`);
      window.getSelection()?.removeAllRanges();
    }
    setPendingPin(null);
    setComposerOpen(false);
    setComposerBody("");
    setEditingId(null);
    setPanelOpen(true);
  }

  function cancelComposer(): void {
    setComposerOpen(false);
    setComposerBody("");
    setEditingId(null);
    setPendingPin(null);
  }

  function openNoteEditor(note: PendingFeedback): void {
    setEditingId(note.client_id);
    setComposerBody(note.body);
    setPendingPin(null);
    setComposerOpen(true);
    setPanelOpen(true);
  }

  /* ─── Tool helpers (secondary path; selection captured on pointerdown so click does not clear it) ─── */
  const stashSelection = (): void => {
    const selected = captureSelection(documentRoot);
    if (selected?.selector) lastSelectionRef.current = selected;
  };
  const addNoteFromSelection = (): void => {
    if (busy) return;
    if (!commentMode) armComment(true);
    const selected = lastSelectionRef.current ?? captureSelection(documentRoot);
    lastSelectionRef.current = null;
    if (!selected?.selector) {
      setStatus("Select text inside one reviewable block, then add a note.");
      return;
    }
    if (selected.selector.exact.length > config.review_limits.max_selector_utf16) {
      setStatus(`Selected text is too long. Select at most ${config.review_limits.max_selector_utf16} characters.`);
      return;
    }
    setHintMode("text");
    // Tools path opens composer directly (selection would be lost after button focus).
    pinCapture(selected, 80, 120, { openComposer: true });
  };

  const submitReview = async (): Promise<void> => {
    const normalizedNotes = notes.map(({ target_summary: _s, ...note }) => ({ ...note, body: note.body.trim() }));
    if (normalizedNotes.some((n) => !n.body)) {
      setStatus("Write each pending note before submitting the review.");
      return;
    }
    if (verdict === "request_changes" && !instruction.trim()) {
      setStatus("Request changes needs a clear instruction.");
      return;
    }
    if (
      instruction.length > config.review_limits.max_text_utf16 ||
      normalizedNotes.some((n) => n.body.length > config.review_limits.max_text_utf16)
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
      armComment(false);
      setStatus(`${response.state === "duplicate" ? "Review already received" : "Review received"} (${response.event_id}).`);
    } catch {
      setStatus("Review was not submitted. Your pending notes are unchanged.");
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        if (composerOpen) {
          event.preventDefault();
          cancelComposer();
          return;
        }
        if (pendingPin) {
          event.preventDefault();
          setPendingPin(null);
          return;
        }
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
      if ((event.metaKey || event.ctrlKey) && event.key === "Enter" && composerOpen) {
        event.preventDefault();
        saveComposer();
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
  }, [composerOpen, config.shortcuts_enabled, pendingPin]);

  function handleEvent(event: SessionEvent): void {
    setEventMessage(event.message ?? null);
    if (event.kind === "revision") {
      setStatus("A newer document revision is available. Finish or discard this review before reloading.");
    } else if (event.kind === "session_closed") {
      setStatus("This review session is closed.");
    }
  }

  return (
    <div class="cf-chrome-frame" data-commenting={commentMode ? "true" : "false"} data-rail={railVisible ? "open" : "closed"}>
      {regionDraft ? <div class="cf-region-draft" style={regionDraftStyle(regionDraft)} aria-hidden="true" /> : null}

      {commentMode
        ? notes.map((note, index) => {
            const rect = targetRect(documentRoot, note, markerEpoch);
            if (!rect) return null;
            return (
              <button
                key={note.client_id}
                type="button"
                class="cf-marker"
                data-testid="note-marker"
                style={speechMarkerStyle(rect)}
                aria-label={`Note ${index + 1}: ${note.target_summary ?? note.block_label}`}
                onClick={() => openNoteEditor(note)}
              >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d={SPEECH_PATH} />
                </svg>
                <span class="n">{index + 1}</span>
              </button>
            );
          })
        : null}

      <a class="cf-skip-link" href="#cf-present-document">
        Skip to document
      </a>

      <header class="cf-topbar">
        {config.identity ? <img class="cf-project-identity" src={config.identity.src} alt={config.identity.alt} /> : null}
        <div class="cf-title-group">
          <span class="cf-kicker">Review document</span>
          <strong>{config.title}</strong>
          <span class="cf-revision">Revision {config.revision}</span>
        </div>
        <div class="cf-appearance" aria-label="Appearance">
          <span class={`cf-note-count-meta${notes.length > 0 ? " has" : ""}`} data-testid="note-count">
            {notes.length} {notes.length === 1 ? "note" : "notes"}
          </span>
          <button
            type="button"
            class="cf-settings-btn"
            data-testid="settings-btn"
            aria-expanded={settingsOpen}
            aria-controls="cf-settings-panel"
            onClick={() => setSettingsOpen((open) => !open)}
          >
            Settings
          </button>
          <button
            id="cf-comment-toggle"
            class="cf-comment-btn"
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
            <span class="cf-dot" aria-hidden="true">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M21 12a8 8 0 0 1-8 8H7l-4 3V12a8 8 0 1 1 18 0Z" />
              </svg>
            </span>
            <span>Comment</span>
            <span class={`cf-count${notes.length > 0 ? " on" : ""}`} aria-label={`${notes.length} pending notes`}>
              {notes.length}
            </span>
          </button>
          {settingsOpen ? (
            <div id="cf-settings-panel" class="cf-settings-panel open" data-testid="settings-panel">
              <h3>Display</h3>
              <div>
                <div class="lbl">
                  Theme<span class="d">utility skins</span>
                </div>
                <div class="pills">
                  {(Object.keys(themeLabels) as UtilityTheme[]).map((t) => (
                    <button
                      type="button"
                      aria-pressed={appearance.theme === t}
                      onClick={() => setAppearance((c) => ({ ...c, theme: t }))}
                    >
                      {themeLabels[t]}
                    </button>
                  ))}
                </div>
              </div>
              <div>
                <div class="lbl">
                  Appearance<span class="d">light, dark, or follow OS</span>
                </div>
                <div class="pills">
                  {(Object.keys(modeLabels) as AppearanceMode[]).map((m) => (
                    <button
                      type="button"
                      aria-pressed={appearance.mode === m}
                      onClick={() => setAppearance((c) => ({ ...c, mode: m }))}
                    >
                      {modeLabels[m]}
                    </button>
                  ))}
                </div>
              </div>
              <p class="hint">Applies to this review surface and is remembered in this browser.</p>
            </div>
          ) : null}
        </div>
      </header>

      {commentMode ? (
        <div class="cf-hint on" data-testid="comment-hint" role="status">
          <span class="mode" data-active={hintMode === "text" ? "true" : "false"}>
            <b>Text</b> select
          </span>
          <span class="mode" data-active={hintMode === "element" ? "true" : "false"}>
            <b>Click</b> figure
          </span>
          <span class="mode" data-active={hintMode === "region" ? "true" : "false"}>
            <b>Drag</b> area
          </span>
          <span class="esc-note">Shift+drag forces region · Esc</span>
        </div>
      ) : null}

      <nav class="cf-section-route" aria-label="Document sections" data-rail={railVisible ? "open" : "closed"}>
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

      {/* Quiet float — Comment capture chip */}
      {pendingPin && !composerOpen ? (
        <div
          class="cf-float on"
          data-testid="float-chip"
          style={`left:${Math.min(Math.max(8, pendingPin.clientX - 40), window.innerWidth - 200)}px;top:${Math.min(Math.max(8, pendingPin.clientY + 8), window.innerHeight - 48)}px`}
        >
          <button type="button" class="main" data-testid="float-comment" onClick={openComposerFromFloat}>
            <span class="ico" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M21 12a8 8 0 0 1-8 8H7l-4 3V12a8 8 0 1 1 18 0Z" />
              </svg>
            </span>
            <span class="lab">Comment</span>
            <span class="q">{pendingPin.captured.summary}</span>
          </button>
          <button type="button" class="esc" data-testid="float-esc" onClick={() => setPendingPin(null)}>
            esc
          </button>
        </div>
      ) : null}

      {/* Composer — note body */}
      {composerOpen ? (
        <div
          class="cf-composer on"
          data-testid="composer"
          style={composerPositionStyle(pendingPin?.clientX ?? 40, pendingPin?.clientY ?? 80)}
        >
          <div class="title">{editingId ? "Edit note" : "Comment"}</div>
          <div class="q">
            {editingId
              ? notes.find((n) => n.client_id === editingId)?.target_summary ?? ""
              : pendingPin?.captured.summary ?? ""}
          </div>
          <textarea
            ref={composerTextRef}
            data-testid="composer-text"
            placeholder="What should change?"
            maxLength={config.review_limits.max_text_utf16}
            value={composerBody}
            onInput={(e) => setComposerBody(e.currentTarget.value)}
          />
          <div class="row">
            <button type="button" class="pri" data-testid="composer-save" onClick={saveComposer}>
              Save note
            </button>
            {editingId ? (
              <button
                type="button"
                class="danger"
                data-testid="composer-delete"
                onClick={() => {
                  setNotes((c) => c.filter((n) => n.client_id !== editingId));
                  cancelComposer();
                  setStatus("Note removed.");
                }}
              >
                Delete
              </button>
            ) : null}
            <button type="button" data-testid="composer-cancel" onClick={cancelComposer}>
              Cancel
            </button>
          </div>
        </div>
      ) : null}

      {/* Notes rail — dock grammar (Comment mode only) */}
      <aside
        id="cf-feedback-panel"
        class="cf-dock"
        data-open={railVisible ? "true" : "false"}
        data-testid="notes-dock"
        aria-label="Review notes"
        aria-labelledby="cf-feedback-title"
        hidden={!railVisible}
        ref={dockRef}
      >
        <div class="hd">
          <b id="cf-feedback-title">Notes</b>
          <span id="dockCount" class="cf-count" aria-label={`${notes.length} pending notes`}>
            {notes.length}
          </span>
          <button
            class="cf-feedback-close"
            type="button"
            onClick={() => {
              setPanelOpen(false);
              document.getElementById("cf-comment-toggle")?.focus();
            }}
          >
            Close
          </button>
        </div>

        {config.feedback?.items.length || config.feedback?.omitted_older ? (
          <section class="cf-feedback-history" aria-labelledby="cf-feedback-history-title">
            <div class="cf-history-heading">
              <h3 id="cf-feedback-history-title">Earlier feedback</h3>
              {config.feedback.omitted_older ? <span>{config.feedback.omitted_older} older in session history</span> : null}
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

        <div class="list" data-testid="notes-list">
          {!notes.length ? (
            <div class="empty" data-testid="notes-empty">
              <span class="t">Nothing noted yet</span>
              <span class="h">Select text, click a figure, or drag across empty area.</span>
            </div>
          ) : (
            notes.map((note, index) => (
              <article
                key={note.client_id}
                class="cf-note-row"
                data-testid="note-row"
                data-id={note.client_id}
                tabindex={0}
                onClick={() => openNoteEditor(note)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    openNoteEditor(note);
                  }
                }}
              >
                <span class="glyph" aria-hidden="true">
                  <svg viewBox="0 0 24 24">
                    <path fill="currentColor" d={SPEECH_PATH} />
                  </svg>
                </span>
                <div>
                  <div class="k">
                    comment · #{index + 1}
                    <button
                      type="button"
                      class="cf-text-action"
                      data-testid="note-remove"
                      aria-label={`Remove note ${index + 1}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        setNotes((c) => c.filter((n) => n.client_id !== note.client_id));
                        setStatus("Note removed.");
                      }}
                    >
                      Remove
                    </button>
                  </div>
                  <div class="a">{note.target_summary ?? note.block_label}</div>
                  <div class="b">{note.body || "…"}</div>
                </div>
              </article>
            ))
          )}
        </div>

        {/* Advanced tools — secondary path for a11y + qualification bridges */}
        <details class="cf-tools">
          <summary>Tools</summary>
          <div class="cf-capture-tools" aria-label="Choose feedback target">
            <button
              class="cf-secondary-action"
              type="button"
              data-testid="tool-add-text"
              disabled={busy}
              onPointerDown={stashSelection}
              onClick={addNoteFromSelection}
            >
              Add selected text
            </button>
            <button
              class="cf-secondary-action"
              type="button"
              data-testid="tool-pick-element"
              disabled={busy}
              aria-pressed={captureMode === "element"}
              onClick={() => {
                setHintMode("element");
                const next = captureMode === "element" ? null : "element";
                setCaptureMode(next);
                captureModeRef.current = next;
              }}
            >
              Pick element
            </button>
            <button
              class="cf-secondary-action"
              type="button"
              data-testid="tool-select-area"
              disabled={busy}
              aria-pressed={captureMode === "region"}
              onClick={() => {
                setHintMode("region");
                const next = captureMode === "region" ? null : "region";
                setCaptureMode(next);
                captureModeRef.current = next;
              }}
            >
              Select area
            </button>
            <button
              class="cf-secondary-action"
              type="button"
              data-testid="tool-whole-doc"
              disabled={busy}
              onClick={() => {
                const captured = captureDocument(documentRoot);
                if (captured) pinCapture(captured, 80, 120, { openComposer: true });
                else setStatus("The document is not ready for whole-document feedback.");
              }}
            >
              Whole document
            </button>
          </div>
        </details>

        <div class="ft">
          <div class="path">
            Goes to <b>session store</b> → present feedback → harness/chat
          </div>
          <label class="cf-verdict">
            <span>Verdict</span>
            <select
              id="cf-review-verdict"
              disabled={busy}
              value={verdict}
              onChange={(e) => setVerdict(e.currentTarget.value as ReviewVerdict)}
            >
              <option value="approve">Approve</option>
              <option value="approve_with_notes">Approve with notes</option>
              <option value="request_changes">Request changes</option>
            </select>
          </label>
          <label class="cf-verdict">
            <span>{verdict === "request_changes" ? "Required change" : "Review summary (optional)"}</span>
            <textarea
              rows={2}
              maxLength={config.review_limits.max_text_utf16}
              disabled={busy}
              value={instruction}
              onInput={(e) => setInstruction(e.currentTarget.value)}
            />
          </label>
          <button
            type="button"
            class={`pri${notes.length === 0 ? " is-empty" : ""}`}
            id="submitAllBtn"
            data-testid="submit-all"
            disabled={busy || notes.length === 0}
            onClick={() => void submitReview()}
          >
            {busy ? "Submitting…" : notes.length === 0 ? "Submit review" : `Submit review (${notes.length})`}
          </button>
          <p class="cf-status" role="status" aria-live="polite">
            {status}
          </p>
          {eventMessage ? <p class="cf-event-message">{eventMessage}</p> : null}
        </div>
      </aside>
    </div>
  );
}

function regionDraftStyle(draft: RegionDraft): string {
  const left = Math.min(draft.start.x, draft.current.x);
  const top = Math.min(draft.start.y, draft.current.y);
  return `left:${left}px;top:${top}px;width:${Math.abs(draft.current.x - draft.start.x)}px;height:${Math.abs(draft.current.y - draft.start.y)}px`;
}

function composerPositionStyle(clientX: number, clientY: number): string {
  const width = Math.min(360, Math.max(240, window.innerWidth - 24));
  const left = Math.min(Math.max(12, clientX - 20), Math.max(12, window.innerWidth - width - 12));
  const top = Math.min(Math.max(12, clientY + 16), Math.max(12, window.innerHeight - 280));
  return `left:${left}px;top:${top}px;width:${width}px`;
}

function speechMarkerStyle(rect: DOMRect): string {
  // Park left of the anchor — speech marker placement
  const x = Math.max(4, rect.left - 34);
  const y = Math.max(4, rect.top - 4);
  return `left:${x}px;top:${y}px`;
}

function targetRect(documentRoot: HTMLElement, note: PendingFeedback, _markerEpoch: number): DOMRect | null {
  if (note.region_selector) return resolveRegion(documentRoot, note.region_selector);
  if (note.element_selector) {
    return resolveElement(documentRoot, note.block_id, note.element_selector)?.getBoundingClientRect() ?? null;
  }
  if (note.selector) {
    const block = documentRoot.querySelector<HTMLElement>(`[data-cf-block-id="${CSS.escape(note.block_id)}"]`);
    return block?.getBoundingClientRect() ?? null;
  }
  return (
    documentRoot.querySelector<HTMLElement>(`[data-cf-block-id="${CSS.escape(note.block_id)}"]`)?.getBoundingClientRect() ?? null
  );
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
      const first = [...visible.entries()].sort((a, b) => a[1] - b[1])[0];
      if (first) onChange(first[0]);
    },
    { rootMargin: "-15% 0px -70% 0px" },
  );
  sections.forEach((s) => observer.observe(s));
  return () => observer.disconnect();
}

function sectionElements(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>("section[data-cf-block-id][data-cf-block-label]")];
}

function isEditable(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && Boolean(target.closest("input, textarea, select, [contenteditable='true']"));
}
