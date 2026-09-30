import { useEffect, useRef, useState } from "preact/hooks";
import { isFeedbackSnapshot, isRecord, type ChromeConfig, type FeedbackAnchor, type FeedbackSnapshot } from "./contracts";
import { postJson } from "./http";

export const THREAD_EVENT = "cf-present-thread";
interface Transition { target: string; note_id?: string; revision: number; at_unix: number }
interface Reply extends Transition { reply_id: string; text: string }
interface Reopen extends Transition { reopen_id: string }
interface Tombstone extends Transition { tombstone_id: string }
interface Answer { answer_id: string; revision: number; form_id: string; outcome: string; values: Record<string, unknown>; question: {title: string} }
interface Snapshot {
  feedback: FeedbackSnapshot; answers: Answer[]; replies: Reply[]; reopens: Reopen[];
  tombstones: Tombstone[]; acknowledged: string[]; omitted_older: number;
}
function transition(value: unknown): value is Transition {
  return isRecord(value) && typeof value.target === "string" && (value.note_id === undefined || typeof value.note_id === "string")
    && Number.isSafeInteger(value.revision) && Number(value.revision) > 0 && Number.isSafeInteger(value.at_unix);
}
function snapshot(value: unknown, config: ChromeConfig): value is Snapshot {
  if (!isRecord(value) || !isFeedbackSnapshot(value.feedback, config.review_limits)) return false;
  return Array.isArray(value.answers) && value.answers.every(a => isRecord(a) && typeof a.answer_id === "string"
    && Number.isSafeInteger(a.revision) && typeof a.form_id === "string" && typeof a.outcome === "string"
    && isRecord(a.values) && isRecord(a.question) && typeof a.question.title === "string")
    && Array.isArray(value.replies) && value.replies.every(r => transition(r) && isRecord(r) && typeof r.reply_id === "string" && typeof r.text === "string")
    && Array.isArray(value.reopens) && value.reopens.every(r => transition(r) && isRecord(r) && typeof r.reopen_id === "string")
    && Array.isArray(value.tombstones) && value.tombstones.every(r => transition(r) && isRecord(r) && typeof r.tombstone_id === "string")
    && Array.isArray(value.acknowledged) && value.acknowledged.every(id => typeof id === "string")
    && Number.isSafeInteger(value.omitted_older) && Number(value.omitted_older) >= 0;
}

/** Stored conversation lives separately from the unsent comment draft. */
export function ThreadRail({config, documentRoot}: {config: ChromeConfig; documentRoot: HTMLElement}) {
  const [data, setData] = useState<Snapshot>({feedback:config.feedback ?? {items:[],omitted_older:0},answers:[],replies:[],reopens:[],tombstones:[],acknowledged:[],omitted_older:0});
  const [error,setError] = useState("");
  const [busy,setBusy] = useState(false);
  const [closed,setClosed] = useState(false);
  const generation = useRef(0);
  async function refresh(): Promise<void> {
    const current = ++generation.current;
    try {
      const next = await postJson<unknown>("/app/api/threads/list", {});
      if (!snapshot(next,config)) throw new Error("Malformed conversation response");
      if (current === generation.current) { setData(next); setError(""); }
    } catch (e) { if (current === generation.current) setError(e instanceof Error ? e.message : "Conversation could not load"); }
  }
  useEffect(() => {
    const update = (e: Event): void => {
      if ((e as CustomEvent).detail === "session_closed") setClosed(true);
      void refresh();
    };
    documentRoot.addEventListener(THREAD_EVENT,update);
    void refresh();
    return () => { ++generation.current; documentRoot.removeEventListener(THREAD_EVENT,update); };
  },[config.session_id,config.revision]);
  const matches = (r: Transition,target: string,note?: string): boolean => r.target === target && r.note_id === note;
  const deleted = (target: string,note?: string): boolean => data.tombstones.some(r => matches(r,target,note));
  const reopened = (target: string,note?: string): boolean => data.reopens.some(r => matches(r,target,note) && !data.acknowledged.includes(r.reopen_id));
  async function change(target: string,note: string | undefined, remove: boolean): Promise<void> {
    setBusy(true);
    try {
      await postJson(`/app/api/${remove ? "notes/tombstone" : "threads/reopen"}`, {
        session_id:config.session_id,revision:config.revision,target,...(note ? {note_id:note} : {}),
      });
      await refresh();
    } catch(e) { setError(e instanceof Error ? e.message : "The thread was not changed"); }
    finally { setBusy(false); }
  }
  const replies = (target: string,note?: string) => data.replies.filter(r => matches(r,target,note)).map(r => (
    <div class="cf-thread-reply" data-testid="thread-reply" key={r.reply_id}>
      <strong>Agent reply</strong><span> · Revision {r.revision}</span><p>{r.text}</p>
    </div>
  ));
  const controls = (target: string,note: string | undefined, canReopen: boolean) => <div class="cf-thread-actions">
    {reopened(target,note) ? <span data-testid="thread-reopened">Reopened</span> : canReopen ?
      <button type="button" disabled={busy || closed} onClick={() => void change(target,note,false)}>Reopen</button> : null}
    {note ? <button type="button" disabled={busy || closed} onClick={() => void change(target,note,true)}>Delete note</button> : null}
  </div>;
  if (!data.feedback.items.length && !data.answers.length && !error) return null;
  return <details class="cf-feedback-history" data-testid="feedback-history">
    <summary id="cf-feedback-history-title">Earlier feedback</summary>
    {error ? <p role="alert">{error}</p> : null}
    {data.omitted_older + data.feedback.omitted_older > 0 ? <p>{data.omitted_older + data.feedback.omitted_older} older entries in session history</p> : null}
    <ol>
      {data.feedback.items.map(item => <li key={item.event_id} data-review="true" data-thread={item.event_id}>
        <div class="cf-history-meta"><strong>{item.verdict.replaceAll("_"," ")}</strong><span>{item.lifecycle}</span>
          {item.acknowledged ? <span data-testid="feedback-acknowledged">acknowledged by agent</span> : null}
          <span>Revision {item.source_revision}</span><span>Version {item.event_version}</span>
          {item.source_revision < config.revision ? <span>Carried from revision {item.source_revision}</span> : null}
        </div>
        {item.instruction ? <p>{item.instruction}</p> : null}
        {replies(item.event_id)}
        {controls(item.event_id,undefined,!!item.acknowledged || ["addressed","dismissed"].includes(item.lifecycle))}
        <ul>{item.notes.map(note => <li key={note.id} data-note={note.id} data-anchor-state={note.anchor.state}>
          {deleted(item.event_id,note.id) || note.block_label === "Deleted note" && note.body === "Deleted by the reviewer" ?
            <p data-testid="thread-tombstone">Deleted by the reviewer</p> : <>
              <div class="cf-note-heading"><strong>{note.block_label}</strong><span>{anchorWords(note.anchor)}</span></div>
              {note.quote ? <blockquote>{note.quote}</blockquote> : null}<p>{note.body}</p>{anchorNotice(note.anchor)}
              {replies(item.event_id,note.id)}
              {controls(item.event_id,note.id,!!item.acknowledged || ["addressed","dismissed"].includes(item.lifecycle))}
            </>}
        </li>)}</ul>
      </li>)}
      {data.answers.map(answer => <li key={answer.answer_id} data-thread={answer.answer_id}>
        <div class="cf-history-meta"><strong>{answer.question.title}</strong><span>Revision {answer.revision}</span></div>
        <p>{answer.outcome}</p><dl>{Object.entries(answer.values).map(([key,value]) => <div key={key}><dt>{key}</dt><dd>{JSON.stringify(value)}</dd></div>)}</dl>
        {replies(answer.answer_id)}{controls(answer.answer_id,undefined,data.acknowledged.includes(answer.answer_id))}
      </li>)}
    </ol>
  </details>;
}

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
