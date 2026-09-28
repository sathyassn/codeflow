/**
 * Present review chrome — utility presentation system Comment SM + craft.
 * Rust owns #cf-present-document; this Preact tree owns chrome only (ADR-0049).
 * Feedback still posts /app/api/reviews for harness-agnostic delivery.
 */
import { createPortal } from "preact/compat";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type {
  AppearanceMode,
  ChromeConfig,
  FeedbackAnchor,
  FeedbackKind,
  PendingFeedback,
  ReviewRequest,
  ReviewResponse,
  ReviewVerdict,
  SessionEvent,
  TypeScale,
  Typeface,
  UtilityTheme,
} from "./contracts";
import { parseServiceError } from "./contracts";
import type { EntitySelector } from "./contracts";
import { followSessionEvents } from "./events";
import { postJson, PresentRequestError } from "./http";
import {
  annotatableAncestor,
  annotatableElements,
  captureDocument,
  captureElement,
  captureRegion,
  captureResolution,
  captureSelection,
  enclosingTarget,
  isTextualTarget,
  resolveElement,
  resolveEntity,
  resolveRegion,
  resolveTarget,
  STROKE_PADDING_PX,
} from "./selection";
import type { CapturedTarget, Point } from "./selection";
import { fitCrops } from "./budget";
import { captureRectJpeg, entityCropPadding, paddedRect, userSpaceBox } from "./excerpt";
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
type TargetKind = "text" | "element" | "region";
interface RegionDraft {
  readonly start: Point;
  readonly current: Point;
}
interface PendingPin {
  readonly captured: CapturedTarget;
  readonly clientX: number;
  readonly clientY: number;
  /** The element an element pin resolved to, for "select enclosing". */
  readonly element?: Element;
  /** The top of the selected line, where a text note's marker points. */
  readonly lineTop?: number;
}
type Focusable = HTMLElement | SVGElement;
interface PinOptions {
  readonly openComposer?: boolean;
  readonly element?: Element;
  readonly lineTop?: number;
}
interface DragGesture {
  x0: number;
  y0: number;
  moved: boolean;
  region: boolean;
  forceRegion: boolean;
  proseOnly: boolean;
  // The press landed on the highlight of the text pin it discarded.
  releasedText: boolean;
}

function targetKindOf(target: Pick<PendingFeedback, "selector" | "element_selector" | "region_selector">): TargetKind {
  if (target.selector) return "text";
  if (target.region_selector) return "region";
  return "element";
}

// One word per kind everywhere the reviewer reads it: float, composer, rail,
// marker and status (the summaries in selection.ts start with the same word).
const kindLabels: Readonly<Record<TargetKind, string>> = { text: "Text", element: "Element", region: "Area" };

// The one instruction the hint, the empty rail and the status line share.
const COMMENT_INSTRUCTION = "Select words, click any part, or drag a box; hold Shift to start a box on words.";


/** Short float/composer quote: the summary minus its "Text:/Element:/Area:" prefix. */
function captureQuote(captured: CapturedTarget): string {
  return captured.summary.replace(/^(Text|Element|Area):\s*/, "").slice(0, 48);
}

const skinPills: readonly UtilityTheme[] = ["graphite", "slate", "sage"];
const themeLabels: Readonly<Record<UtilityTheme, string>> = {
  graphite: "Graphite",
  slate: "Slate",
  sage: "Sage",
};
const typefacePills: readonly Typeface[] = ["archivo", "inter", "plex"];
const typefaceLabels: Readonly<Record<Typeface, string>> = {
  archivo: "Archivo",
  inter: "Inter",
  plex: "Plex Sans",
};
const scalePills: readonly TypeScale[] = ["compact", "default", "large"];
const scaleLabels: Readonly<Record<TypeScale, string>> = {
  compact: "Compact",
  default: "Default",
  large: "Large",
};
const modeLabels: Readonly<Record<AppearanceMode, string>> = {
  light: "Light",
  dark: "Dark",
  system: "System",
};

const SPEECH_PATH =
  "M4 3.5A3.5 3.5 0 0 1 7.5 0h9A3.5 3.5 0 0 1 20 3.5v8A3.5 3.5 0 0 1 16.5 15H11l-4.2 3.4c-.7.55-1.8.05-1.8-.85V15H7.5A3.5 3.5 0 0 1 4 11.5v-8Z";

export function Chrome({ config, documentRoot }: ChromeProps) {
  const [appearance, setAppearance] = useState(initialAppearance);
  // Unsent notes survive a reload of this tab (QA defect 4), until a submit
  // succeeds. They stay in this browser's session storage for this session.
  const [draft] = useState(() => readDraft(config.session_id));
  const [notes, setNotes] = useState<readonly PendingFeedback[]>(draft?.notes ?? []);
  const [verdict, setVerdict] = useState<ReviewVerdict>(draft?.verdict ?? "approve_with_notes");
  const [instruction, setInstruction] = useState(draft?.instruction ?? "");
  const [status, setStatus] = useState(() => draftNotice(draft, config.revision) ?? "Ready for review.");
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

  const [markerLayer, setMarkerLayer] = useState<HTMLElement | null>(null);
  // Toast: confirmations and session notices must stay visible when the rail
  // (and its status line) is closed — pass10 grammar. Sticky for session-level
  // notices; timed for confirmations.
  const [toast, setToast] = useState<string | null>(null);
  const toastTimerRef = useRef(0);
  const toastStickyRef = useRef(false);
  const showToast = (message: string, opts?: { sticky?: boolean }): void => {
    window.clearTimeout(toastTimerRef.current);
    setToast(message);
    toastStickyRef.current = Boolean(opts?.sticky);
    if (!opts?.sticky) toastTimerRef.current = window.setTimeout(() => setToast(null), 3400);
  };

  const dockRef = useRef<HTMLElement>(null);
  const floatRef = useRef<HTMLDivElement>(null);
  const composerTextRef = useRef<HTMLTextAreaElement>(null);
  const regionDraftRef = useRef<RegionDraft | null>(null);
  const submitAttemptRef = useRef<{ fingerprint: string; eventId: string } | null>(null);
  // Held from the first step of a submit to its end (C071-1): crop fitting
  // awaits, and nothing that goes into the review may change meanwhile.
  const submittingRef = useRef(false);
  const notesRef = useRef<readonly PendingFeedback[]>(notes);
  const commentModeRef = useRef(false);
  const captureModeRef = useRef<CaptureMode>(null);
  const notesCountRef = useRef(0);
  const dragGestureRef = useRef<DragGesture | null>(null);
  // Only the toolbar uses this stash: focusing its button can collapse the
  // native selection. Debounced gestures always recapture the live range.
  const toolbarSelectionRef = useRef<CapturedTarget | null>(null);
  const lastPinnedSelectionRef = useRef<string>("");
  const composerOpenRef = useRef(false);
  const pendingPinRef = useRef<PendingPin | null>(null);
  const pinCaptureRef = useRef<(c: CapturedTarget, x: number, y: number, o?: PinOptions) => void>(() => undefined);
  const saveComposerRef = useRef<() => void>(() => undefined);
  const settingsOpenRef = useRef(false);
  const shiftRef = useRef(false);
  const hotRef = useRef<Element | null>(null);
  const hotSelRef = useRef<Element | null>(null);
  // Client-side marker placement hints (never sent to the server): for a text
  // note, the vertical fraction of the selection's first line in its block.
  const markerMetaRef = useRef(new Map<string, { ay: number }>());
  // Where an edit was initiated (marker or note row) — positions the composer.
  const editAtRef = useRef<{ x: number; y: number } | null>(null);

  const sections = useMemo(() => readSections(documentRoot), [documentRoot, config.revision]);
  commentModeRef.current = commentMode;
  captureModeRef.current = captureMode;
  notesCountRef.current = notes.length;
  notesRef.current = notes;
  composerOpenRef.current = composerOpen;
  pendingPinRef.current = pendingPin;
  // The float is placed near the pointer, then kept inside the viewport by
  // its measured width, so ESC is never cut off (QA defect 7).
  useLayoutEffect(() => {
    const float = floatRef.current;
    if (!float) return;
    const overflow = float.getBoundingClientRect().right - (window.innerWidth - 8);
    if (overflow > 0) float.style.left = `${Math.max(8, float.offsetLeft - overflow)}px`;
  }, [pendingPin, composerOpen]);
  settingsOpenRef.current = settingsOpen;
  const railVisible = commentMode && panelOpen;
  // A timed hint about the sheet ("the Comment button opens your notes") is
  // stale once the sheet is open; a sticky notice stays.
  useEffect(() => {
    if (!railVisible || !sheetLayout() || toastStickyRef.current) return;
    window.clearTimeout(toastTimerRef.current);
    setToast(null);
  }, [railVisible]);

  const clearHot = (): void => {
    hotRef.current?.classList.remove("cf-hot");
    hotRef.current = null;
    hotSelRef.current?.classList.remove("cf-hot-sel");
    hotSelRef.current = null;
  };
  // Discarding a text capture also releases its native highlight. A press on a
  // live highlight (even at its edge) starts a native text drag instead of a
  // new selection, at once on Linux and Windows, so no new capture follows.
  const releaseCapturedSelection = (pin: PendingPin | null): void => {
    if (!pin?.captured.selector) return;
    const selection = window.getSelection();
    if (selection?.rangeCount && documentRoot.contains(selection.getRangeAt(0).commonAncestorContainer)) {
      selection.removeAllRanges();
    }
  };
  // Whether a text pin's live highlight covers a viewport point.
  const capturedSelectionCovers = (pin: PendingPin | null, x: number, y: number): boolean => {
    if (!pin?.captured.selector) return false;
    const selection = window.getSelection();
    if (!selection?.rangeCount) return false;
    const range = selection.getRangeAt(0);
    if (!documentRoot.contains(range.commonAncestorContainer)) return false;
    return Array.from(range.getClientRects()).some(
      (rect) => x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom,
    );
  };
  const setHot = (element: Element | null): void => {
    if (hotRef.current === element) return;
    hotRef.current?.classList.remove("cf-hot");
    hotRef.current = element;
    element?.classList.add("cf-hot");
  };
  const setHotSel = (element: Element | null): void => {
    hotSelRef.current?.classList.remove("cf-hot-sel");
    hotSelRef.current = element;
    element?.classList.add("cf-hot-sel");
  };

  const armComment = (on: boolean): void => {
    setCommentMode(on);
    commentModeRef.current = on;
    if (!on) {
      lastPinnedSelectionRef.current = "";
      toolbarSelectionRef.current = null;
      setPanelOpen(false);
      setCaptureMode(null);
      captureModeRef.current = null;
      setRegionDraft(null);
      regionDraftRef.current = null;
      setPendingPin(null);
      setComposerOpen(false);
      setComposerBody("");
      setEditingId(null);
      clearHot();
      dragGestureRef.current = null;
      window.getSelection()?.removeAllRanges();
      setStatus(notesCountRef.current ? `${notesCountRef.current} note${notesCountRef.current === 1 ? "" : "s"} queued · Comment off` : "Ready for review.");
    } else {
      // Where the rail is a bottom sheet it would cover half the document;
      // the Comment button opens it on request (QA defect 7).
      setPanelOpen(!sheetLayout());
      setHintMode("element");
      setStatus(COMMENT_INSTRUCTION);
    }
  };

  useEffect(() => {
    applyAppearance(appearance);
    persistAppearance(appearance);
    if (appearance.mode !== "system") return undefined;
    return watchSystemMode(() => applyAppearance(appearance));
  }, [appearance]);

  // Each palette pill shows the canvas and accent of the skin it selects,
  // read from the live custom properties rather than from a second hard coded
  // copy of the token table: the root attribute is moved, the computed value
  // is read, and the attribute is put back within the same task, so no
  // intermediate state is ever painted.
  useEffect(() => {
    if (!settingsOpen) return;
    const element = document.documentElement;
    const computed = getComputedStyle(element);
    const current = element.dataset.cfTheme;
    for (const pill of document.querySelectorAll<HTMLButtonElement>("[data-testid=\"skin-pills\"] button[data-skin]")) {
      element.dataset.cfTheme = pill.dataset.skin!;
      pill.style.setProperty("--pill-canvas", computed.getPropertyValue("--cf-canvas").trim());
      pill.style.setProperty("--pill-accent", computed.getPropertyValue("--cf-accent").trim());
    }
    if (current === undefined) delete element.dataset.cfTheme;
    else element.dataset.cfTheme = current;
    return undefined;
  }, [appearance, settingsOpen]);

  useEffect(() => writeDraft(config.session_id, config.revision, notes, verdict, instruction), [notes, verdict, instruction]);
  useEffect(() => {
    const notice = draftNotice(draft, config.revision);
    if (notice) showToast(notice, { sticky: true });
  }, []);
  useEffect(() => observeSections(documentRoot, setActiveSection), [documentRoot, config.revision]);
  useEffect(
    () => followSessionEvents(`${config.revision}:${config.event_sequence}`, handleEvent, setEventMessage),
    [config.session_id, config.revision, config.event_sequence],
  );

  useEffect(() => {
    documentRoot.dataset.cfCommenting = commentMode ? "true" : "false";
    if (!commentMode) {
      delete documentRoot.dataset.cfCaptureMode;
      delete documentRoot.dataset.cfCursor;
    }
    return () => {
      delete documentRoot.dataset.cfCommenting;
      delete documentRoot.dataset.cfCursor;
    };
  }, [commentMode, documentRoot]);

  // In-document marker host: markers live in the document coordinate space so
  // they scroll with what they annotate (pass10 gutter behavior).
  useEffect(() => {
    const layer = document.createElement("div");
    layer.className = "cf-marker-layer";
    layer.setAttribute("aria-hidden", "false");
    documentRoot.append(layer);
    setMarkerLayer(layer);
    return () => {
      layer.remove();
      setMarkerLayer(null);
    };
  }, [documentRoot, config.revision]);

  // While commenting, native hit-testing reaches the containing figure/block
  // instead of the iframe document. No overlay can obscure neighboring prose,
  // and the browser retains clipping, transforms and scroll semantics.
  // Only frames present when mode/revision changes are managed, as before.
  useEffect(() => {
    if (!commentMode) return undefined;
    const frames = [...documentRoot.querySelectorAll("iframe")].map((frame) => {
      const value = frame.style.getPropertyValue("pointer-events");
      const priority = frame.style.getPropertyPriority("pointer-events");
      frame.style.setProperty("pointer-events", "none", "important");
      return { frame, value, priority };
    });
    return () => frames.forEach(({ frame, value, priority }) => {
      if (value) frame.style.setProperty("pointer-events", value, priority);
      else frame.style.removeProperty("pointer-events");
    });
  }, [commentMode, documentRoot, config.revision]);

  /* ─── Explicit capture modes (a11y / advanced tools) ─── */
  useEffect(() => {
    if (captureMode !== "element") return undefined;
    // Keyboard stops (SPC-014 B3): entities and annotatable elements in
    // document order. Arrows move, Enter pins, Shift+Enter climbs to the
    // enclosing entity, then the block. The stops are read again on every
    // move: a code block highlighted or a figure drawn while the mode is on
    // replaces its elements, and the stop that had focus can leave the page.
    const stops = (): Focusable[] => annotatableElements(documentRoot).filter((el): el is Focusable => el instanceof HTMLElement || el instanceof SVGElement);
    const initial = stops();
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const touched = new Map<Focusable, string | null>();
    let activeIndex = Math.max(0, initial.findIndex((el) => el.contains(previousFocus)));
    // The stop last moved to, focused or not: one inside a closed disclosure
    // takes no focus, and the next move still goes past it.
    let current: Element | null = null;
    const focusCandidate = (candidates: Focusable[], index: number): void => {
      activeIndex = (index + candidates.length) % candidates.length;
      current = candidates[activeIndex] ?? null;
      candidates.forEach((el, i) => {
        if (!touched.has(el)) touched.set(el, el.getAttribute("tabindex"));
        el.tabIndex = i === activeIndex ? 0 : -1;
      });
      candidates[activeIndex]?.focus();
      setHot(candidates[activeIndex] ?? null);
    };
    const focusEnclosing = (from: Element): void => {
      const outer = enclosingTarget(documentRoot, from)?.element;
      if (!(outer instanceof HTMLElement || outer instanceof SVGElement)) return;
      if (!touched.has(outer)) touched.set(outer, outer.getAttribute("tabindex"));
      outer.tabIndex = -1;
      outer.focus();
      setHot(outer);
      current = outer;
      const index = stops().indexOf(outer);
      if (index >= 0) activeIndex = index;
    };
    // A pick opens the composer, which takes focus; leaving the mode then
    // must not hand focus back to the tool that started it.
    let pinned = false;
    const complete = (target: Element): void => {
      const captured = captureElement(documentRoot, target);
      if (captured) {
        const r = target.getBoundingClientRect();
        pinned = true;
        pinCaptureRef.current(captured, r.left, r.top, { openComposer: true });
      } else setStatus("That element cannot be anchored. Choose content inside one review block.");
      setHot(null);
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
      if (event.key === "Enter" && event.shiftKey && document.activeElement instanceof Element && documentRoot.contains(document.activeElement)) {
        event.preventDefault();
        focusEnclosing(document.activeElement);
        return;
      }
      if ((event.key === "Enter" || event.key === " ") && document.activeElement instanceof Element && documentRoot.contains(document.activeElement)) {
        event.preventDefault();
        complete(document.activeElement);
        return;
      }
      const offset =
        event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : event.key === "ArrowUp" || event.key === "ArrowLeft" ? -1 : 0;
      if (offset === 0) return;
      const active = document.activeElement;
      // Focus on the page itself means the stop that had it was replaced;
      // the move resumes from that stop's place in the order.
      const lost = active === null || active === document.body;
      if (!lost && !(active instanceof Element && documentRoot.contains(active))) return;
      const candidates = stops();
      if (candidates.length === 0) return;
      event.preventDefault();
      const at = current instanceof HTMLElement || current instanceof SVGElement ? candidates.indexOf(current) : -1;
      focusCandidate(candidates, (at >= 0 ? at : activeIndex) + offset);
    };
    documentRoot.dataset.cfCaptureMode = "element";
    documentRoot.addEventListener("click", pick, { capture: true });
    documentRoot.ownerDocument.addEventListener("keydown", keydown, { capture: true });
    if (initial.length > 0) focusCandidate(initial, activeIndex);
    return () => {
      delete documentRoot.dataset.cfCaptureMode;
      documentRoot.removeEventListener("click", pick, { capture: true });
      documentRoot.ownerDocument.removeEventListener("keydown", keydown, { capture: true });
      touched.forEach((prev, el) => {
        if (prev === null) el.removeAttribute("tabindex");
        else el.setAttribute("tabindex", prev);
      });
      setHot(null);
      if (!pinned && previousFocus?.isConnected) previousFocus.focus();
    };
  }, [captureMode, documentRoot, notes.length]);

  useEffect(() => {
    if (captureMode !== "region") return undefined;
    let pointerId: number | null = null;
    let startClient: Point | null = null;
    const docPoint = (event: PointerEvent): Point => {
      const root = documentRoot.getBoundingClientRect();
      return { x: event.clientX - root.left, y: event.clientY - root.top };
    };
    const down = (event: PointerEvent): void => {
      if (event.button !== 0) return;
      pointerId = event.pointerId;
      documentRoot.setPointerCapture(pointerId);
      startClient = { x: event.clientX, y: event.clientY };
      const point = docPoint(event);
      const draft = { start: point, current: point };
      regionDraftRef.current = draft;
      setRegionDraft(draft);
      event.preventDefault();
    };
    const move = (event: PointerEvent): void => {
      if (pointerId !== event.pointerId) return;
      const draft = regionDraftRef.current;
      if (draft) {
        const next = { ...draft, current: docPoint(event) };
        regionDraftRef.current = next;
        setRegionDraft(next);
      }
      event.preventDefault();
    };
    const finish = (event: PointerEvent): void => {
      if (pointerId !== event.pointerId) return;
      const point = { x: event.clientX, y: event.clientY };
      const captured = startClient ? captureRegion(documentRoot, startClient, point) : null;
      startClient = null;
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

  /* ─── Pass10 default gestures while Comment is armed (no tool forced) ───
   * Prose (textual, no Shift): native selection only — never start a marquee.
   * Diagram / empty: drag draws a region. Shift+drag forces region anywhere.
   * Priority on release: text selection > deliberate region > element click.
   */
  useEffect(() => {
    if (!commentMode || captureMode || busy || composerOpen) return undefined;

    shiftRef.current = false;
    let pinTimer = 0;
    const onSelection = (): void => {
      window.clearTimeout(pinTimer);
      if (!commentModeRef.current || captureModeRef.current) return;
      const drag = dragGestureRef.current;
      if (drag && !drag.proseOnly && drag.region) return;
      const selected = captureSelection(documentRoot);
      if (!selected?.selector) return;
      if (selected.selector.exact.length > config.review_limits.max_selector_utf16) {
        setStatus(`Selected text is too long. Select at most ${config.review_limits.max_selector_utf16} characters.`);
        return;
      }
      setHintMode("text");
      pinTimer = window.setTimeout(() => {
        if (!commentModeRef.current || captureModeRef.current) return;
        const live = captureSelection(documentRoot);
        const selection = window.getSelection();
        if (!live?.selector || live.selector.exact.length > config.review_limits.max_selector_utf16 || selection?.rangeCount !== 1) return;
        const identity = JSON.stringify([live.blockId, live.selector]);
        if (identity === lastPinnedSelectionRef.current) return;
        lastPinnedSelectionRef.current = identity;
        const rect = selection.getRangeAt(0).getBoundingClientRect();
        const cx = rect.left + rect.width / 2 - 40;
        pinCaptureRef.current(live, cx, rect.bottom, { openComposer: false, lineTop: rect.top });
      }, 160);
    };

    const onShift = (event: KeyboardEvent): void => {
      if (event.key !== "Shift") return;
      shiftRef.current = event.type === "keydown";
      if (commentModeRef.current && !dragGestureRef.current) {
        setHintMode(shiftRef.current ? "region" : "element");
      }
    };

    const onPointerDown = (event: PointerEvent): void => {
      if (event.button !== 0 || !(event.target instanceof Element)) return;
      if (!documentRoot.contains(event.target)) return;
      if (event.target.closest(".cf-marker, .cf-marker-layer")) return;
      if (event.target.closest("button, a, input, textarea, select")) return;
      let releasedText = false;
      if (pendingPinRef.current) {
        releasedText = capturedSelectionCovers(pendingPinRef.current, event.clientX, event.clientY);
        releaseCapturedSelection(pendingPinRef.current);
        setPendingPin(null);
        lastPinnedSelectionRef.current = "";
        setHotSel(null);
      }
      regionDraftRef.current = null;
      setRegionDraft(null);
      const shift = shiftRef.current || event.shiftKey;
      const textual = isTextualTarget(event.target) && !shift;
      if (textual) setHintMode("text");
      else if (shift) setHintMode("region");
      else setHintMode(annotatableAncestor(documentRoot, event.target) ? "element" : "region");
      dragGestureRef.current = {
        x0: event.clientX,
        y0: event.clientY,
        moved: false,
        region: false,
        forceRegion: shift,
        proseOnly: textual,
        releasedText,
      };
    };

    const onPointerMove = (event: PointerEvent): void => {
      const g = dragGestureRef.current;
      if (!g) {
        // Idle-armed hover: text cursor over prose, candidate outline elsewhere.
        if (pendingPinRef.current || composerOpenRef.current) return;
        const target = event.target instanceof Element ? event.target : null;
        if (!target || !documentRoot.contains(target) || target.closest(".cf-marker, .cf-marker-layer")) {
          setHot(null);
          return;
        }
        const textual = isTextualTarget(target) && !shiftRef.current;
        documentRoot.dataset.cfCursor = textual ? "text" : "cross";
        // SVG words are selectable and still name the part they label, so
        // hover shows the part a click would pin.
        const drawing = target instanceof SVGElement;
        const point = { x: event.clientX, y: event.clientY };
        const candidate = textual && !drawing ? null : annotatableAncestor(documentRoot, target, point);
        setHot(candidate);
        if (textual) setHintMode("text");
        else if (shiftRef.current) setHintMode("region");
        else setHintMode(candidate ? "element" : "region");
        return;
      }
      if (g.proseOnly) return;
      const w = Math.abs(event.clientX - g.x0);
      const h = Math.abs(event.clientY - g.y0);
      if (w > 5 || h > 5) g.moved = true;
      const force = g.forceRegion || shiftRef.current || event.shiftKey;
      const marquee = force ? w > 10 || h > 10 : (w >= 28 && h >= 28) || w > 14 || h > 14;
      if (marquee) {
        // Non-prose marquee: never leave a stray selection behind (diagram
        // <text> labels select as the pointer crosses them).
        window.getSelection()?.removeAllRanges();
        g.region = true;
        setHintMode("region");
        setHot(null);
        // Draft is stored in document coordinates so the persistent marquee
        // scrolls with the content it covers.
        const root = documentRoot.getBoundingClientRect();
        const draft = {
          start: { x: g.x0 - root.left, y: g.y0 - root.top },
          current: { x: event.clientX - root.left, y: event.clientY - root.top },
        };
        regionDraftRef.current = draft;
        setRegionDraft(draft);
      }
    };

    const onPointerUp = (event: PointerEvent): void => {
      const g = dragGestureRef.current;
      dragGestureRef.current = null;
      if (!g) return;
      const w = Math.abs(event.clientX - g.x0);
      const h = Math.abs(event.clientY - g.y0);
      const dist = Math.hypot(w, h);
      // The gesture decides, not the side effect: a marquee begun off prose is
      // a region even if it swept across a label on the way.
      const regionGesture = !g.proseOnly && g.region && ((w >= 20 && h >= 20) || (g.forceRegion && dist > 16));

      if (regionGesture) {
        window.getSelection()?.removeAllRanges();
        // The browser still sends a click when the box ends on the element it
        // began on; on a disclosure summary that click would open it and move
        // the area just marked. A drawn box activates nothing.
        const swallow = (click: MouseEvent): void => {
          click.preventDefault();
          click.stopPropagation();
        };
        window.addEventListener("click", swallow, { capture: true, once: true });
        window.setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
        const captured = captureRegion(documentRoot, { x: g.x0, y: g.y0 }, { x: event.clientX, y: event.clientY });
        if (captured) {
          // Keep the marquee visible under the float/composer — it shows what
          // the pending region note covers. Cleared on save, cancel, or Esc.
          setHintMode("region");
          pinCapture(captured, event.clientX, event.clientY, { openComposer: false });
        } else {
          regionDraftRef.current = null;
          setRegionDraft(null);
        }
        return;
      }
      regionDraftRef.current = null;
      setRegionDraft(null);

      // Text selection wins — the selectionchange pin already owns the float.
      const sel = window.getSelection();
      if (sel && !sel.isCollapsed && String(sel).trim().length >= 2) {
        if (captureSelection(documentRoot)?.selector) return;
        // A selection the review text cannot hold, such as a figure's label,
        // pins the part the label names; it is never a dead gesture (QA
        // defect 5 in its figure form).
        const node = sel.anchorNode;
        const anchor = node instanceof Element ? node : node?.parentElement ?? null;
        sel.removeAllRanges();
        const labelled = anchor && documentRoot.contains(anchor) ? resolveTarget(documentRoot, anchor) : null;
        const pinned = labelled ? captureResolution(labelled) : null;
        if (!labelled || !pinned) {
          setStatus("That selection cannot be anchored. Select text inside one review block, or click the part.");
          return;
        }
        setHintMode("element");
        setHot(null);
        setHotSel(labelled.element);
        pinCapture(pinned, event.clientX, event.clientY, { openComposer: false, element: labelled.element });
        return;
      }

      // A click on the captured highlight only dismisses its text pin.
      // Pointerdown cleared the range, so the browser placed a caret here.
      // The painted glyph boxes decide (TSK-096): a click past the last
      // glyph, even in the same line box, pins the element under it.
      if (g.releasedText) {
        setHot(null);
        return;
      }

      // Abandoned non-region drag
      if (!g.proseOnly && dist > 22) {
        setHot(null);
        return;
      }

      // Click → element pin on the block under the pointer
      const under = document.elementFromPoint(event.clientX, event.clientY);
      const target =
        (under && documentRoot.contains(under) && !under.closest(".cf-marker, .cf-marker-layer") ? under : null) ??
        (event.target instanceof Element ? event.target : null);
      if (!target || target.closest(".cf-marker, .cf-marker-layer")) return;
      const point = { x: event.clientX, y: event.clientY };
      const resolved = annotatableAncestor(documentRoot, target, point);
      const captured = captureElement(documentRoot, target, point);
      if (!captured || !resolved) return;
      setHintMode("element");
      setHot(null);
      setHotSel(resolved);
      pinCapture(captured, event.clientX, event.clientY, { openComposer: false, element: resolved });
    };

    // An image's native drag would take the pointer (dragstart, then
    // pointercancel), so a box drawn over it never pinned and its marquee
    // stayed behind (QA defect 1). While commenting, the gesture is ours.
    const onDragStart = (event: DragEvent): void => {
      if (event.target instanceof Element && event.target.closest("img, video, a")) event.preventDefault();
    };
    const onPointerCancel = (): void => {
      dragGestureRef.current = null;
      regionDraftRef.current = null;
      setRegionDraft(null);
    };
    document.addEventListener("selectionchange", onSelection);
    documentRoot.addEventListener("dragstart", onDragStart);
    window.addEventListener("pointercancel", onPointerCancel);
    document.addEventListener("keydown", onShift);
    document.addEventListener("keyup", onShift);
    documentRoot.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    return () => {
      window.clearTimeout(pinTimer);
      document.removeEventListener("selectionchange", onSelection);
      documentRoot.removeEventListener("dragstart", onDragStart);
      window.removeEventListener("pointercancel", onPointerCancel);
      document.removeEventListener("keydown", onShift);
      document.removeEventListener("keyup", onShift);
      documentRoot.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      setHot(null);
    };
  }, [busy, captureMode, commentMode, composerOpen, config.review_limits.max_selector_utf16, documentRoot]);

  // Markers hold document coordinates; anything that reflows the document
  // (viewport resize, type scale, rail open) must re-measure their anchors.
  useEffect(() => {
    let raf = 0;
    const refresh = (): void => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => setMarkerEpoch((v) => v + 1));
    };
    addEventListener("resize", refresh, { passive: true });
    const observer = "ResizeObserver" in globalThis ? new ResizeObserver(refresh) : null;
    observer?.observe(documentRoot);
    return () => {
      cancelAnimationFrame(raf);
      removeEventListener("resize", refresh);
      observer?.disconnect();
    };
  }, [documentRoot]);

  useEffect(() => {
    if (composerOpen) requestAnimationFrame(() => composerTextRef.current?.focus());
  }, [composerOpen]);

  // A pin that is dropped without a note (Escape, the chip's esc, a click on
  // its own highlight) takes its "Pinned:" status with it, so the rail never
  // names a pin that is gone. Before paint, so the two never show together.
  useLayoutEffect(() => {
    if (pendingPin) return;
    setStatus((current) => (current.startsWith("Pinned: ") ? (commentModeRef.current ? COMMENT_INSTRUCTION : "Ready for review.") : current));
  }, [pendingPin]);

  /* ─── Pin → float → composer (qualified Comment flow) ─── */
  function pinCapture(captured: CapturedTarget, clientX: number, clientY: number, opts?: PinOptions): void {
    if (submittingRef.current) {
      setStatus("The review is being sent. Add the note when it is done.");
      return;
    }
    if (notesCountRef.current >= config.review_limits.max_notes) {
      setStatus(noteLimitMessage(config.review_limits.max_notes));
      if (!commentModeRef.current) armComment(true);
      return;
    }
    if (!commentModeRef.current) armComment(true);
    else if (!sheetLayout()) setPanelOpen(true);
    setPendingPin({
      captured,
      clientX,
      clientY,
      ...(opts?.element ? { element: opts.element } : {}),
      ...(opts?.lineTop !== undefined ? { lineTop: opts.lineTop } : {}),
    });
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

  async function saveComposer(): Promise<void> {
    if (submittingRef.current) return;
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
      const clientId = crypto.randomUUID();
      // Text markers park at the selection's height within the anchor block —
      // remember the fraction so re-measures keep pointing at the right line.
      if (c.selector) {
        const block = documentRoot.querySelector<HTMLElement>(`[data-cf-block-id="${CSS.escape(c.blockId)}"]`);
        const rect = block?.getBoundingClientRect();
        const lineY = pendingPin.lineTop ?? pendingPin.clientY;
        const ay = rect && rect.height > 0 ? Math.min(1, Math.max(0, (lineY - rect.top) / rect.height)) : 0;
        markerMetaRef.current.set(clientId, { ay });
      }
      let excerptText = c.excerptText?.trim() || c.selector?.exact?.trim() || "";
      let excerptImage = null;
      let cropBox: EntitySelector["crop_box"] | null = null;
      if (c.region_selector) {
        const region = resolveRegion(documentRoot, c.region_selector);
        // A whole-document note shows what was on screen: the full page
        // squeezed into one crop was a sliver (QA defect 8).
        const whole = c.region_selector.scope === "document" && c.region_selector.height_ppm === 1_000_000 && c.region_selector.width_ppm === 1_000_000;
        const box = region && whole ? visiblePart(region) : region;
        if (box) excerptImage = await captureRectJpeg(documentRoot, box);
      } else if (c.element_selector) {
        const el = (c.entity_selector ? resolveEntity(documentRoot, c.blockId, c.entity_selector.entity_id) : null)
          ?? resolveElement(documentRoot, c.blockId, c.element_selector);
        // A drawing part is cropped with the thin-stroke padding (SPC-014 B4),
        // in its own drawing, and an entity note records that crop in the
        // drawing's user space.
        const owner = el instanceof SVGElement ? el.closest("svg") : null;
        const rect = el?.getBoundingClientRect();
        const box = rect && owner ? paddedRect(rect, entityCropPadding(owner, STROKE_PADDING_PX)) : rect;
        if (box && box.width >= 4 && box.height >= 4) {
          excerptImage = await captureRectJpeg(documentRoot, box, owner, { png: Boolean(c.entity_selector) });
          if (excerptImage && owner && c.entity_selector) cropBox = userSpaceBox(owner, box);
        }
      }
      const excerpt = excerptText || excerptImage ? { ...(excerptText ? { text: excerptText } : {}), ...(excerptImage ? { image: excerptImage } : {}) } : undefined;
      const entitySelector: EntitySelector | undefined = c.entity_selector ? { ...c.entity_selector, ...(cropBox ? { crop_box: cropBox } : {}) } : undefined;
      setNotes((current) => [
        ...current,
        {
          client_id: clientId,
          block_id: c.blockId,
          block_label: c.blockLabel,
          kind: "comment" as FeedbackKind,
          body,
          ...(c.selector ? { selector: c.selector } : {}),
          ...(c.element_selector ? { element_selector: c.element_selector } : {}),
          ...(entitySelector ? { entity_selector: entitySelector } : {}),
          ...(c.region_selector ? { region_selector: c.region_selector } : {}),
          target_summary: c.summary,
          ...(excerpt ? { excerpt } : {}),
        },
      ]);
      setVerdict((v) => (v === "approve" ? "approve_with_notes" : v));
      setStatus(`Note saved for ${c.summary}.`);
      lastPinnedSelectionRef.current = "";
      window.getSelection()?.removeAllRanges();
    }
    clearHot();
    setRegionDraft(null);
    regionDraftRef.current = null;
    setPendingPin(null);
    setComposerOpen(false);
    setComposerBody("");
    setEditingId(null);
    if (!sheetLayout()) setPanelOpen(true);
    else if (!panelOpen) showToast("Note saved. The Comment button opens your notes and Submit.");
  }
  saveComposerRef.current = () => {
    void saveComposer();
  };

  function cancelComposer(): void {
    releaseCapturedSelection(pendingPinRef.current);
    setComposerOpen(false);
    setComposerBody("");
    setEditingId(null);
    setPendingPin(null);
    lastPinnedSelectionRef.current = "";
    clearHot();
    setRegionDraft(null);
    regionDraftRef.current = null;
  }

  function openNoteEditor(note: PendingFeedback, at?: { x: number; y: number }): void {
    if (submittingRef.current) return;
    editAtRef.current = at ?? null;
    setEditingId(note.client_id);
    setComposerBody(note.body);
    setPendingPin(null);
    setComposerOpen(true);
    if (!sheetLayout()) setPanelOpen(true);
  }

  /* ─── Tool helpers (secondary path; selection captured on pointerdown so click does not clear it) ─── */
  const stashSelection = (): void => {
    const selected = captureSelection(documentRoot);
    toolbarSelectionRef.current = selected?.selector && selected.selector.exact.length <= config.review_limits.max_selector_utf16
      ? selected : null;
  };
  const addNoteFromSelection = (): void => {
    if (busy) return;
    if (!commentMode) armComment(true);
    const selected = toolbarSelectionRef.current ?? captureSelection(documentRoot);
    toolbarSelectionRef.current = null;
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
    if (submittingRef.current) return;
    // What this review carries: exactly these notes and this instruction.
    const sentNotes = notes;
    const sentInstruction = instruction;
    const normalizedNotes = sentNotes.map(({ target_summary: _s, ...note }) => ({ ...note, body: note.body.trim() }));
    if (normalizedNotes.some((n) => !n.body)) {
      setStatus("Write each pending note before submitting the review.");
      return;
    }
    if (verdict === "request_changes" && !sentInstruction.trim()) {
      setStatus("Request changes needs a clear instruction.");
      return;
    }
    if (
      sentInstruction.length > config.review_limits.max_text_utf16 ||
      normalizedNotes.some((n) => n.body.length > config.review_limits.max_text_utf16)
    ) {
      setStatus(`Each note or review summary is limited to ${config.review_limits.max_text_utf16} characters.`);
      return;
    }
    // The guard and the disabled controls come before the first await, so
    // nothing is edited or sent twice while the crops are fitted (C071-1).
    submittingRef.current = true;
    setBusy(true);
    try {
      const payloadOf = (list: readonly (typeof normalizedNotes)[number][]) => ({
        session_id: config.session_id,
        revision: config.revision,
        verdict,
        notes: [...list],
        ...(sentInstruction.trim() ? { instruction: sentInstruction.trim() } : {}),
      });
      // Crops share what the notes leave of the body limit (QA defect 3): a
      // crop is made smaller, or left out, and no note is lost.
      const requestBytes = (list: readonly (typeof normalizedNotes)[number][]): number =>
        new TextEncoder().encode(JSON.stringify({ event_id: crypto.randomUUID(), ...payloadOf(list) })).byteLength;
      setStatus("Preparing the review…");
      const fitted = await fitCrops(normalizedNotes, config.review_limits.max_payload_bytes, requestBytes);
      const payload = payloadOf(fitted.notes);
      const cropNotice = fitted.reduced || fitted.dropped
        ? ` To fit the review limit, ${[
          fitted.reduced ? `${fitted.reduced} ${fitted.reduced === 1 ? "picture was" : "pictures were"} made smaller` : "",
          fitted.dropped ? `${fitted.dropped} ${fitted.dropped === 1 ? "picture was" : "pictures were"} left out` : "",
        ].filter(Boolean).join(" and ")}; every note was kept.`
        : "";
      const fingerprint = JSON.stringify(payload);
      const previous = submitAttemptRef.current;
      const eventId = previous?.fingerprint === fingerprint ? previous.eventId : crypto.randomUUID();
      submitAttemptRef.current = { fingerprint, eventId };
      const request: ReviewRequest = { event_id: eventId, ...payload };
      if (new TextEncoder().encode(JSON.stringify(request)).byteLength > config.review_limits.max_payload_bytes) {
        setStatus(`This review is too large to submit. Shorten it below ${config.review_limits.max_payload_bytes} bytes.`);
        return;
      }
      setStatus("Submitting review…");
      const response = await postJson<ReviewResponse>("/app/api/reviews", request);
      // Only what was sent is cleared: a note whose save finished while the
      // review was on its way stays pending, and so does its draft.
      const sent = new Set(sentNotes);
      const left = notesRef.current.filter((note) => !sent.has(note)).length;
      setNotes((current) => current.filter((note) => !sent.has(note)));
      setInstruction((current) => (current === sentInstruction ? "" : current));
      submitAttemptRef.current = null;
      if (left === 0 && !composerOpenRef.current) armComment(false);
      const pending = left ? ` ${left} ${left === 1 ? "note saved while it was sent is" : "notes saved while it was sent are"} still pending.` : "";
      const confirmation = `${response.state === "duplicate" ? "Review already received" : "Review received"}.${cropNotice}${pending}`;
      setStatus(confirmation);
      showToast(confirmation);
    } catch (error) {
      // SPC-014 I3: a refused review names its reason; the pending notes stay
      // for every refusal, so the reviewer can correct and resend them.
      const refusal = error instanceof PresentRequestError ? parseServiceError(error.message) : null;
      const reason = refusal?.message ?? (error instanceof Error ? error.message : String(error));
      const notice = `Review was not submitted: ${reason} Your pending notes are unchanged.`;
      setStatus(notice);
      showToast(notice, { sticky: true });
    } finally {
      submittingRef.current = false;
      setBusy(false);
    }
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        // Ladder: settings → composer → float → capture tool → exit Comment.
        if (settingsOpenRef.current) {
          event.preventDefault();
          setSettingsOpen(false);
          return;
        }
        if (composerOpenRef.current) {
          event.preventDefault();
          cancelComposer();
          return;
        }
        if (pendingPinRef.current) {
          event.preventDefault();
          releaseCapturedSelection(pendingPinRef.current);
          pendingPinRef.current = null;
          setPendingPin(null);
          lastPinnedSelectionRef.current = "";
          clearHot();
          setRegionDraft(null);
          regionDraftRef.current = null;
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
      // "Select enclosing" for a pin made by pointer or touch: climb to the
      // entity around the pinned part, then the block.
      const pinned = pendingPinRef.current;
      if (event.key === "Enter" && event.shiftKey && !composerOpenRef.current && pinned?.element && !isEditable(event.target)) {
        event.preventDefault();
        const outer = enclosingTarget(documentRoot, pinned.element);
        const captured = outer ? captureResolution(outer) : null;
        if (!outer || !captured) return;
        setHotSel(outer.element);
        pinCaptureRef.current(captured, pinned.clientX, pinned.clientY, { openComposer: false, element: outer.element });
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key === "Enter" && composerOpenRef.current) {
        event.preventDefault();
        saveComposerRef.current();
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
      const notice = "A newer document revision is available. Finish or discard this review before reloading.";
      setStatus(notice);
      showToast(notice, { sticky: true });
    } else if (event.kind === "session_closed") {
      const notice = "This review session is closed.";
      setStatus(notice);
      showToast(notice, { sticky: true });
    }
  }

  return (
    <div class="cf-chrome-frame" data-commenting={commentMode ? "true" : "false"} data-rail={railVisible ? "open" : "closed"}>
      {commentMode && markerLayer
        ? createPortal(
            <>
              {regionDraft ? <div class="cf-region-draft" style={regionDraftStyle(regionDraft)} aria-hidden="true" /> : null}
              {/* A saved area keeps a faint outline while Comment is on, so its
                  extent stays visible after the marquee is gone. */}
              {notes.map((note) => {
                const outline = savedRegionStyle(documentRoot, note, markerEpoch);
                return outline ? <div key={`area-${note.client_id}`} class="cf-region-saved" data-testid="saved-area" style={outline} aria-hidden="true" /> : null;
              })}
              {notes.map((note, index) => {
                const at = markerPlacement(documentRoot, note, markerMetaRef.current.get(note.client_id), index, markerEpoch);
                if (!at) return null;
                return (
                  <button
                    key={note.client_id}
                    type="button"
                    class="cf-marker"
                    data-testid="note-marker"
                    style={`left:${at.left}px;top:${at.top}px;width:${38 + 8 * (String(index + 1).length - 1)}px`}
                    aria-label={`Note ${index + 1}, ${kindLabels[targetKindOf(note)]}: ${noteQuote(note)}`}
                    title={`#${index + 1} ${kindLabels[targetKindOf(note)]}: ${noteQuote(note)}`}
                    onClick={(e) => openNoteEditor(note, { x: e.clientX, y: e.clientY })}
                  >
                    <svg viewBox="0 0 24 24" aria-hidden="true">
                      <path d={SPEECH_PATH} />
                    </svg>
                    <span class="n">{index + 1}</span>
                  </button>
                );
              })}
            </>,
            markerLayer,
          )
        : null}

      <a class="cf-skip-link" href="#cf-present-document">
        Skip to document
      </a>

      <header class="cf-topbar">
        {config.identity ? <img class="cf-project-identity" src={config.identity.src} alt={config.identity.alt} /> : <span class="cf-brand-mark" aria-hidden="true" />}
        <div class="cf-title-group">
          <span class="cf-kicker">Review document</span>
          <div class="cf-title-line"><strong>{config.title}</strong><span class="cf-revision">Revision {config.revision}</span></div>
        </div>
        <div class="cf-appearance" aria-label="Appearance">
          <span class={`cf-note-count-meta${notes.length > 0 ? " has" : ""}`} data-testid="note-count" hidden={notes.length === 0}>
            {notes.length} {notes.length === 1 ? "note" : "notes"}
          </span>
          <button
            type="button"
            class="cf-settings-btn"
            data-testid="settings-btn"
            aria-label="Settings"
            title="Settings"
            aria-haspopup="dialog"
            aria-expanded={settingsOpen}
            aria-controls="cf-settings-panel"
            onClick={() => setSettingsOpen((open) => !open)}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="m10 3-1 3-3 1-2-1-2 4 2 2v3l-2 1 2 4 3-1 3 1 1 3h4l1-3 3-1 2 1 2-4-2-2v-3l2-1-2-4-3 1-3-1-1-3Z"/></svg>
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
            <div id="cf-settings-panel" class="cf-settings-panel open" data-testid="settings-panel" role="dialog" aria-label="Display" onKeyDown={(event) => {
              if (!(event.target instanceof HTMLButtonElement)) return;
              const group = event.target.closest(".pills");
              if (!group) return;
              const buttons = Array.from(group.querySelectorAll("button"));
              const index = buttons.indexOf(event.target);
              const next = event.key === "ArrowRight" || event.key === "ArrowDown" ? index + 1
                : event.key === "ArrowLeft" || event.key === "ArrowUp" ? index - 1
                : event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : null;
              if (next === null) return;
              event.preventDefault();
              const button = buttons[(next + buttons.length) % buttons.length];
              button?.focus(); button?.click();
            }}>
              <div class="preview" aria-hidden="true">
                <span class="swatch swatch-canvas" /><span class="swatch swatch-surface" /><span class="swatch swatch-accent" />
                <span class="body">Body Aa</span>
                <span class="mono">Mono 012</span>
              </div>
              <div>
                <div class="lbl">
                  Font<span class="d">Body and headings</span>
                </div>
                <div class="pills" data-testid="typeface-pills">
                  {typefacePills.map((face) => (
                    <button
                      type="button"
                      data-typeface={face}
                      data-testid={`typeface-${face}`}
                      aria-pressed={appearance.typeface === face}
                      onClick={() => setAppearance((c) => ({ ...c, typeface: face }))}
                    >
                      {typefaceLabels[face]}
                    </button>
                  ))}
                </div>
              </div>
              <div>
                <div class="lbl">
                  Size<span class="d">Text and controls</span>
                </div>
                <div class="pills" data-testid="scale-pills">
                  {scalePills.map((scale) => (
                    <button
                      type="button"
                      data-scale={scale}
                      aria-pressed={appearance.scale === scale}
                      onClick={() => setAppearance((c) => ({ ...c, scale }))}
                    >
                      {scaleLabels[scale]}
                    </button>
                  ))}
                </div>
              </div>
              <div>
                <div class="lbl">
                  Palette<span class="d">Surfaces and accent</span>
                </div>
                <div class="pills" data-testid="skin-pills">
                  {skinPills.map((t) => (
                    <button
                      type="button"
                      data-skin={t}
                      aria-pressed={appearance.theme === t}
                      onClick={() => setAppearance((c) => ({ ...c, theme: t }))}
                    >
                      <span class="sw" aria-hidden="true">
                        <i class="sw-canvas" />
                        <i class="sw-accent" />
                      </span>
                      {themeLabels[t]}
                    </button>
                  ))}
                </div>
              </div>
              <div>
                <div class="lbl">
                  Appearance<span class="d">Light, dark or the system</span>
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
        <div class="cf-hint on" data-testid="comment-hint" data-capture-mode={hintMode}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5h-7l-4.5 3.5v-3.5H5A1.5 1.5 0 0 1 3.5 15V7A1.5 1.5 0 0 1 5 5.5Z" /></svg>
          <span role="status">{COMMENT_INSTRUCTION} Esc leaves.</span>
          <button type="button" class="cf-hint-leave" data-testid="comment-leave" onClick={() => armComment(false)}>
            Done
          </button>
        </div>
      ) : null}

      <nav class="cf-section-route" aria-label="Document sections" data-rail={railVisible ? "open" : "closed"}>
        <span class="cf-kicker">Sections</span>
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
          ref={floatRef}
          style={`left:${Math.max(8, pendingPin.clientX - 40)}px;top:${Math.min(Math.max(8, pendingPin.clientY + 8), window.innerHeight - 48)}px`}
        >
          <button type="button" class="main" data-testid="float-comment" onClick={openComposerFromFloat}>
            <span class="ico" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M21 12a8 8 0 0 1-8 8H7l-4 3V12a8 8 0 1 1 18 0Z" />
              </svg>
            </span>
            <span class="lab">{kindLabels[targetKindOf(pendingPin.captured)]}</span>
            <span class="q">{captureQuote(pendingPin.captured)}</span>
          </button>
          <button
            type="button"
            class="esc"
            data-testid="float-esc"
            onClick={() => {
              releaseCapturedSelection(pendingPin);
              setPendingPin(null);
              lastPinnedSelectionRef.current = "";
              clearHot();
              setRegionDraft(null);
              regionDraftRef.current = null;
            }}
          >
            esc
          </button>
        </div>
      ) : null}

      {/* Composer — note body */}
      {composerOpen ? (
        <div
          class="cf-composer on"
          data-testid="composer"
          style={composerPositionStyle(
            pendingPin?.clientX ?? editAtRef.current?.x ?? 40,
            pendingPin?.clientY ?? editAtRef.current?.y ?? 80,
          )}
        >
          <div class="title">{editingId ? "Edit note" : "Comment"}</div>
          <div class="q">
            {(() => {
              const editing = editingId ? notes.find((n) => n.client_id === editingId) : null;
              if (editing) return `${kindLabels[targetKindOf(editing)]} · ${noteQuote(editing)}`;
              if (pendingPin) return `${kindLabels[targetKindOf(pendingPin.captured)]} · ${captureQuote(pendingPin.captured)}`;
              return "";
            })()}
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
            <button type="button" class="pri" data-testid="composer-save" disabled={busy} onClick={saveComposer}>
              Save note
            </button>
            {editingId ? (
              <button
                type="button"
                class="danger"
                data-testid="composer-delete"
                disabled={busy}
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

        <div class="list" data-testid="notes-list">
          {!notes.length ? (
            <div class="empty" data-testid="notes-empty">
              <span class="t">Nothing noted yet</span>
              <span class="h">{COMMENT_INSTRUCTION}</span>
            </div>
          ) : (
            notes.map((note, index) => (
              <article
                key={note.client_id}
                class="cf-note-row"
                data-testid="note-row"
                data-id={note.client_id}
                tabindex={0}
                onClick={(e) => openNoteEditor(note, { x: e.clientX, y: e.clientY })}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    const rect = e.currentTarget.getBoundingClientRect();
                    openNoteEditor(note, { x: rect.left, y: rect.bottom });
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
                    {kindLabels[targetKindOf(note)]} · #{index + 1}
                    <button
                      type="button"
                      class="cf-text-action"
                      data-testid="note-remove"
                      disabled={busy}
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

        {config.feedback?.items.length || config.feedback?.omitted_older ? (
          <details class="cf-feedback-history" data-testid="feedback-history">
            <summary id="cf-feedback-history-title">
              Earlier feedback
              {config.feedback.omitted_older ? (
                <span> · {config.feedback.omitted_older} older in session history</span>
              ) : null}
            </summary>
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
                            <span>{anchorWords(note.anchor)}</span>
                          </div>
                          {note.quote ? <blockquote>{note.quote}</blockquote> : null}
                          <p>{note.body}</p>
                          {anchorNotice(note.anchor)}
                        </li>
                      ))}
                    </ul>
                  ) : null}
                </li>
              ))}
            </ol>
          </details>
        ) : null}

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
            disabled={busy || (notes.length === 0 && verdict === "approve_with_notes")}
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

      {toast ? (
        <div class="cf-toast on" role="status" data-testid="toast">
          {toast}
        </div>
      ) : null}
    </div>
  );
}

function regionDraftStyle(draft: RegionDraft): string {
  const left = Math.min(draft.start.x, draft.current.x);
  const top = Math.min(draft.start.y, draft.current.y);
  return `left:${left}px;top:${top}px;width:${Math.abs(draft.current.x - draft.start.x)}px;height:${Math.abs(draft.current.y - draft.start.y)}px`;
}

/** The width at which the notes rail becomes a bottom sheet (styles.css). */
function sheetLayout(): boolean {
  return window.matchMedia("(max-width: 879.98px)").matches;
}

function composerPositionStyle(clientX: number, clientY: number): string {
  const width = Math.min(360, Math.max(240, window.innerWidth - 24));
  const left = Math.min(Math.max(12, clientX - 20), Math.max(12, window.innerWidth - width - 12));
  const top = Math.min(Math.max(12, clientY + 16), Math.max(12, window.innerHeight - 280));
  return `left:${left}px;top:${top}px;width:${width}px`;
}

function noteQuote(note: PendingFeedback): string {
  return (note.target_summary ?? note.block_label).replace(/^(Text|Element|Area):\s*/, "").slice(0, 96);
}

/**
 * Pass10 marker placement in document coordinates: every marker parks in the
 * gutter immediately left of the line it points at (a text note's selected
 * line, else the top of its element or area). With no gutter, as for a full
 * width block on a phone, it sits right of a narrow anchor, else above the
 * line at the anchor's right end: never on the words or area it marks.
 */
function markerPlacement(
  documentRoot: HTMLElement,
  note: PendingFeedback,
  meta: { ay: number } | undefined,
  index: number,
  markerEpoch: number,
): { left: number; top: number } | null {
  const rect = targetRect(documentRoot, note, markerEpoch);
  if (!rect) return null;
  const root = documentRoot.getBoundingClientRect();
  if (root.width <= 0) return null;
  const left = rect.left - root.left;
  const top = rect.top - root.top;
  const width = 38 + 8 * (String(index + 1).length - 1);
  const clamp = (x: number): number => Math.max(2 - root.left, Math.min(x, innerWidth - root.left - width * 1.03 - 8));
  const line = targetKindOf(note) === "text" ? top + (meta?.ay ?? 0) * rect.height : top;
  const gutter = left - width - 8;
  if (gutter >= 2) return { left: clamp(gutter), top: Math.max(2, line - 2) };
  const right = left + rect.width + 8;
  if (right + width <= root.width - 2) return { left: clamp(right), top: Math.max(2, line - 2) };
  const end = Math.min(left + rect.width, root.width - 2) - width;
  return { left: clamp(Math.max(2, end)), top: Math.max(2, line - markerHeight() - 4) };
}

/** A marker's height: 26 px, raised by the button minimum of 2.25rem (styles.css). */
function markerHeight(): number {
  return Math.max(26, 2.25 * (parseFloat(getComputedStyle(document.documentElement).fontSize) || 16));
}

/** The faint outline of a saved area, in document coordinates; none for a whole-document note. */
function savedRegionStyle(documentRoot: HTMLElement, note: PendingFeedback, markerEpoch: number): string | null {
  const selector = note.region_selector;
  if (!selector || (selector.scope === "document" && selector.width_ppm === 1_000_000 && selector.height_ppm === 1_000_000)) return null;
  const rect = targetRect(documentRoot, note, markerEpoch);
  if (!rect || rect.width <= 0 || rect.height <= 0) return null;
  const root = documentRoot.getBoundingClientRect();
  return `left:${rect.left - root.left}px;top:${rect.top - root.top}px;width:${rect.width}px;height:${rect.height}px`;
}

function targetRect(documentRoot: HTMLElement, note: PendingFeedback, _markerEpoch: number): DOMRect | null {
  if (note.region_selector) return resolveRegion(documentRoot, note.region_selector);
  if (note.entity_selector) {
    const entity = resolveEntity(documentRoot, note.block_id, note.entity_selector.entity_id);
    if (entity) return entity.getBoundingClientRect();
  }
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

// The state of an earlier note in words, never its enum value (the detail
// line below it, `anchorNotice`, says why): "moved" only when the anchor
// reports a change.
function anchorWords(anchor: FeedbackAnchor): string {
  switch (anchor.state) {
    case "orphaned": return "unpositioned";
    case "block_fallback": return "on the block";
    case "reanchored": return anchor.changed ? "moved" : "anchored";
    case "entity_reanchored": return anchor.label_changed ? "moved" : "anchored";
    default: return "anchored";
  }
}

// What the rail says about where an earlier note now sits (SPC-014 B1): a
// note that moved or lost its target says so, never silently.
function anchorNotice(anchor: FeedbackAnchor) {
  switch (anchor.state) {
    case "orphaned": return <p class="cf-anchor-warning">Unpositioned: {anchor.reason}</p>;
    case "block_fallback": return <p class="cf-anchor-warning">Shown on the block: {anchor.reason}</p>;
    case "reanchored": return anchor.changed
      ? <p class="cf-anchor-warning">Moved: the closest match in this revision differs from the quote.</p>
      : <p class="cf-anchor-note">Matched uniquely in this revision.</p>;
    case "entity_reanchored": return anchor.label_changed
      ? <p class="cf-anchor-warning">The part it names was relabelled in this revision.</p>
      : <p class="cf-anchor-note">Still names the same part; the block around it changed in this revision.</p>;
    case "element_reanchored": return <p class="cf-anchor-note">The same element: its block is unchanged in this revision.</p>;
    case "region_reanchored": return anchor.scope === "document"
      ? <p class="cf-anchor-note">Still the whole document in this revision.</p>
      : <p class="cf-anchor-note">The same area: its block is unchanged in this revision.</p>;
    default: return null;
  }
}

function visiblePart(rect: DOMRect): DOMRect | null {
  const top = Math.max(rect.top, 0);
  const bottom = Math.min(rect.bottom, window.innerHeight);
  const left = Math.max(rect.left, 0);
  const right = Math.min(rect.right, window.innerWidth);
  return bottom - top >= 4 && right - left >= 4 ? new DOMRect(left, top, right - left, bottom - top) : null;
}

interface Draft {
  readonly revision: number;
  readonly notes: readonly PendingFeedback[];
  readonly verdict: ReviewVerdict;
  readonly instruction: string;
}

const draftKey = (sessionId: string): string => `cf-present-draft:${sessionId}`;

function readDraft(sessionId: string): Draft | null {
  try {
    const stored = JSON.parse(sessionStorage.getItem(draftKey(sessionId)) ?? "null") as Draft | null;
    return stored && Array.isArray(stored.notes) && stored.notes.length > 0 ? stored : null;
  } catch {
    return null;
  }
}

// A draft too large for the storage keeps its notes without their pictures.
function writeDraft(sessionId: string, revision: number, notes: readonly PendingFeedback[], verdict: ReviewVerdict, instruction: string): void {
  if (notes.length === 0) {
    clearDraft(sessionId);
    return;
  }
  const withoutPictures = notes.map((note) => (note.excerpt?.image ? { ...note, excerpt: { ...(note.excerpt.text ? { text: note.excerpt.text } : {}) } } : note));
  for (const kept of [notes, withoutPictures]) {
    try {
      sessionStorage.setItem(draftKey(sessionId), JSON.stringify({ revision, notes: kept, verdict, instruction }));
      return;
    } catch {
      // Try the smaller draft, then give up quietly: the notes stay on screen.
    }
  }
}

function clearDraft(sessionId: string): void {
  try {
    sessionStorage.removeItem(draftKey(sessionId));
  } catch {
    // Storage unavailable: nothing was kept.
  }
}

function draftNotice(draft: Draft | null, revision: number): string | null {
  if (!draft) return null;
  const count = `${draft.notes.length} unsent ${draft.notes.length === 1 ? "note" : "notes"}`;
  return draft.revision === revision
    ? `Restored ${count}.`
    : `Restored ${count} from revision ${draft.revision}. A note on a block that changed may be refused when you submit; edit or remove it then.`;
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

// A block inside a disclosure or a tab is hidden until opened, so the
// sections list names the disclosure or tabs block only (QA defect 8).
function sectionElements(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>("section[data-cf-block-id][data-cf-block-label]")]
    .filter((section) => !section.parentElement?.closest("details"));
}

function isEditable(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && Boolean(target.closest("input, textarea, select, [contenteditable='true']"));
}
