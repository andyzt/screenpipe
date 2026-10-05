// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
import { localFetch } from "@/lib/api";
export type WorkContext = "work" | "personal" | "mixed" | "unspecified";
export type ContributionRole = "owner" | "contributor" | "reviewer" | "observer" | "unknown";
export type OutcomeStatus = "in_progress" | "completed" | "blocked" | "learning";
export interface WorkNote {
  id: string; revision: number; date: string; context: WorkContext; project: string; outcome: string;
  status: OutcomeStatus; role: ContributionRole; approved: boolean;
  source_activity_id: number | null; source_activity_key: string | null;
  source_start_at: string | null; source_end_at: string | null; updated_at: string;
}
export interface WorkNoteInput extends Omit<WorkNote, "approved" | "revision" | "source_activity_key" | "source_start_at" | "source_end_at" | "updated_at"> { approve: boolean; expected_revision: number | null }
export interface WorkLogSnapshot { entries: WorkNote[]; truncated: boolean; from: string; to: string }
export const contexts = ["unspecified", "work", "personal", "mixed"] as const;
export const roles = ["unknown", "owner", "contributor", "reviewer", "observer"] as const;
export const statuses = ["in_progress", "completed", "blocked", "learning"] as const;
/**
 * English name of a context / role / status value for the Markdown report,
 * which stays English by design (it is pasted into trackers and reviews that
 * do not follow the UI locale). The dialog renders the same values through
 * `t("workLog.<kind>.<value>")` instead.
 */
export function label(value: string) { return value.replaceAll("_", " ").replace(/^./, c => c.toUpperCase()); }
export function calendarDate(date = new Date()) {
  return `${date.getFullYear()}-${String(date.getMonth()+1).padStart(2,"0")}-${String(date.getDate()).padStart(2,"0")}`;
}
export function quarterRange(now = new Date(), offset = 0) {
  const start = new Date(now.getFullYear(), Math.floor(now.getMonth()/3)*3+offset*3, 1);
  const end = new Date(start.getFullYear(), start.getMonth()+3, 0);
  return { from: calendarDate(start), to: calendarDate(end) };
}
export function canApprove(note: Pick<WorkNoteInput,"context"|"outcome"|"role">) {
  return note.context === "work" && !!note.outcome.trim() && ["owner","contributor","reviewer"].includes(note.role);
}
export function blankNote(source?: {id: number; start_at: string}): WorkNoteInput {
  return { id: crypto.randomUUID(), date: source ? calendarDate(new Date(source.start_at)) : calendarDate(),
    context:"unspecified", project:"", outcome:"", status:"in_progress", role:"unknown", approve:false, expected_revision:null, source_activity_id:source?.id ?? null };
}
/** A work-log failure the dialog can name in the reader's language. */
export type WorkLogErrorCode = "conflict" | "unavailable" | "incomplete" | "empty";
export class WorkLogError extends Error {
  constructor(public code: WorkLogErrorCode, message: string, public status?: number) {
    super(message);
    this.name = "WorkLogError";
  }
}
async function json<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await localFetch(path, init);
  if (response.status === 409) throw new WorkLogError("conflict", "This note changed in another window. Cancel editing, refresh and reopen the latest note; your newer privacy choice was preserved.", 409);
  if (!response.ok) throw new WorkLogError("unavailable", `Work notes unavailable (${response.status}). Try again; nothing was exported.`, response.status);
  return response.json();
}
export function fetchWorkLog(from: string, to: string, signal?: AbortSignal) {
  return json<WorkLogSnapshot>(`/journal/work-log?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`, {signal, cache:"no-store"});
}
export function saveWorkNote(note: WorkNoteInput) {
  const {id,...input}=note;
  return json<{entry:WorkNote}>(`/journal/work-log/${encodeURIComponent(id)}`,{method:"PUT",headers:{"Content-Type":"application/json"},body:JSON.stringify(input)});
}
export function deleteWorkNote(id: string, revision: number) {
  return json(`/journal/work-log/${encodeURIComponent(id)}?revision=${revision}`,{method:"DELETE"});
}
// Authored text remains literal Markdown; no raw HTML, injected headings or links.
function escape(value: string) {
  return value.replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;")
    .replace(/[\\`*_\[\]{}!#|]/g,"\\$&").replace(/[\r\n]+/g," ");
}
function clock(iso: string) { const d=new Date(iso); return `${String(d.getHours()).padStart(2,"0")}:${String(d.getMinutes()).padStart(2,"0")}`; }
/**
 * The only thing a report says about a linked journal card is the span the
 * engine snapshotted when the note was saved. The card's title is model-written,
 * is rewritten on regeneration, was never under the approval checkbox and can
 * name people, files or a personal topic, so it stays out.
 */
export function sourceSpanSuffix(note: Pick<WorkNote,"source_start_at"|"source_end_at">) {
  if(!note.source_start_at||!note.source_end_at)return "";
  const start=new Date(note.source_start_at), end=new Date(note.source_end_at);
  if(Number.isNaN(start.getTime())||Number.isNaN(end.getTime()))return "";
  return ` (journal card ${calendarDate(start)} ${clock(note.source_start_at)}–${clock(note.source_end_at)})`;
}
export function reportMarkdown(snapshot: WorkLogSnapshot, from: string, to: string, project="") {
  if (snapshot.truncated || snapshot.from!==from || snapshot.to!==to) throw new WorkLogError("incomplete", "Report is incomplete. Narrow the dates and refresh before exporting.");
  const notes=snapshot.entries.filter(n=>n.date>=from && n.date<=to && n.approved && canApprove(n) && (!project || n.project===project));
  if (!notes.length) throw new WorkLogError("empty", "No approved work notes match this period and project.");
  const groups = new Map<string,WorkNote[]>();
  for (const note of [...notes].sort((a,b)=>a.date.localeCompare(b.date)||a.id.localeCompare(b.id))) {
    const key=note.project||"Unassigned project";groups.set(key,[...(groups.get(key)||[]),note]);
  }
  const line=(n:WorkNote)=>`- ${n.date} · ${label(n.status)} · ${label(n.role)} — ${escape(n.outcome)}${sourceSpanSuffix(n)}`;
  return [`# Reviewed work · ${from} to ${to}`,"",`${notes.length} user-reviewed contribution${notes.length === 1 ? "" : "s"}. Status and role are user-reported, not independently verified. This is a curated record, not a complete activity or productivity score.`,"",
    ...Array.from(groups).flatMap(([name,entries])=>[`## ${escape(name)}`,"",...entries.map(line),""])
  ].join("\n");
}
