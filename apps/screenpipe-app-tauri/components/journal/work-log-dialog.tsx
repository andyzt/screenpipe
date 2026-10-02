// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
"use client";
import React, {useEffect, useMemo, useState} from "react";
import {Button} from "@/components/ui/button";
import {commands} from "@/lib/utils/tauri";
import {Input} from "@/components/ui/input";
import {Textarea} from "@/components/ui/textarea";
import {Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger} from "@/components/ui/dialog";
import {blankNote, canApprove, contexts, roles, statuses, quarterRange, fetchWorkLog, saveWorkNote, deleteWorkNote, reportMarkdown,
  WorkLogError, type WorkNote, type WorkNoteInput, type WorkContext, type WorkLogSnapshot} from "@/lib/journal/work-log";
import {fetchJournalDay, fetchRecap} from "@/lib/journal/api";
import {contributionCandidates, recentDates, type ContributionCandidate} from "@/lib/journal/contribution-candidates";
import {journalDayToday} from "@/lib/journal/format";
import type {JournalDay, JournalRecap} from "@/lib/journal/types";
import {useT, type TranslateFn} from "@/lib/i18n";

/** The journal window behind the suggestions: read once per window choice, never per notes reload. */
type JournalWindow = { days: JournalDay[]; recaps: Map<string, JournalRecap> };
async function readJournalWindow(dates: string[], signal: AbortSignal): Promise<JournalWindow> {
  const days=await Promise.all(dates.map(date=>fetchJournalDay(date,signal)));
  const recaps=new Map<string,JournalRecap>();
  await Promise.all(days.filter(day=>day.recap.status==="ready"||day.recap.status==="stale").map(async day=>{
    try { recaps.set(day.date, await fetchRecap(day.date,signal)); } catch { /* recap is optional */ }
  }));
  return {days,recaps};
}

/**
 * The on-screen name of a context / role / status value. The Markdown report
 * keeps using `label()` from `lib/journal/work-log` because the export is
 * English by design; only what the dialog renders follows the UI locale.
 */
const enumLabel=(t:TranslateFn,kind:"context"|"role"|"status",value:string)=>t(`workLog.${kind}.${value}`);
const fieldClass="h-9 rounded-md border border-input bg-background px-2 text-sm";
export function WorkLogDialog() {
  const t=useT();
  // Library errors carry a code; show them in the reader's language. Anything
  // else (a network failure, an unexpected throw) falls back to the action's
  // own message rather than raw English.
  const errorText=(e:unknown,fallback:string)=>e instanceof WorkLogError?t(`workLog.error.${e.code}`,{status:e.status??""}):t(fallback);
  const [open,setOpen]=useState(false);
  const [range,setRange]=useState(()=>quarterRange());
  const [context,setContext]=useState<WorkContext|"all">("all");
  const [project,setProject]=useState("");
  const [snapshot,setSnapshot]=useState<WorkLogSnapshot|null>(null);
  const [note,setNote]=useState<WorkNoteInput|null>(null);
  const [busy,setBusy]=useState(false), [loading,setLoading]=useState(false);
  const [error,setError]=useState(""), [message,setMessage]=useState(""), [preview,setPreview]=useState("");
  const [revision,setRevision]=useState(0), [deleteId,setDeleteId]=useState<string|null>(null);
  const [sourceHint,setSourceHint]=useState<string>("");
  const [suggestDays,setSuggestDays]=useState<0|7|14>(0);
  const [journalWindow,setJournalWindow]=useState<JournalWindow|null>(null);
  const [suggestError,setSuggestError]=useState("");
  useEffect(()=>{
    const add=(event:Event)=>{
      const source=(event as CustomEvent<{id:number;start_at:string}>).detail;
      setNote(blankNote(source));setSourceHint("");setRange(quarterRange(new Date(source.start_at)));setOpen(true);setMessage("");setPreview("");
    };
    window.addEventListener("journal-add-work-note",add);
    return ()=>window.removeEventListener("journal-add-work-note",add);
  },[]);
  useEffect(()=>{
    if(!open)return;
    const controller=new AbortController();setSnapshot(null);setLoading(true);setError("");setPreview("");
    void fetchWorkLog(range.from,range.to,controller.signal).then(setSnapshot).catch(e=>{
      if(!controller.signal.aborted)setError(errorText(e,"workLog.loadFailed"));
    }).finally(()=>{if(!controller.signal.aborted)setLoading(false)});
    return ()=>controller.abort();
  },[open,range.from,range.to,revision]);
  useEffect(()=>{
    if(!open||!suggestDays){setJournalWindow(null);return;}
    const controller=new AbortController();setJournalWindow(null);setSuggestError("");
    readJournalWindow(recentDates(journalDayToday(),suggestDays),controller.signal)
      .then(window=>{if(!controller.signal.aborted)setJournalWindow(window)})
      .catch(e=>{if(!controller.signal.aborted){setJournalWindow({days:[],recaps:new Map()});setSuggestError(e instanceof Error?e.message:t("workLog.journalReadFailed"))}});
    return ()=>controller.abort();
  },[open,suggestDays,t]);
  // `noted` follows the notes list, so a fresh snapshot re-marks without re-reading the journal.
  const suggestions=useMemo(()=>journalWindow&&snapshot?contributionCandidates(journalWindow.days,journalWindow.recaps,snapshot.entries):null,[journalWindow,snapshot]);
  const draftFrom=(candidate:ContributionCandidate)=>{
    const draft=candidate.kind==="card"?blankNote({id:candidate.card.id,start_at:candidate.card.start_at}):{...blankNote(),date:candidate.date};
    if(candidate.kind==="card"&&candidate.project)draft.project=candidate.project;
    // Keep the list on the note's quarter, like the day-view handoff does, so the saved note stays visible.
    const [y,m,d]=draft.date.split("-").map(Number);setRange(quarterRange(new Date(y,m-1,d)));
    setNote(draft);setDeleteId(null);setPreview("");setMessage("");
    setSourceHint(candidate.kind==="card"
      ?t("workLog.sourceHint.card",{title:candidate.title,date:candidate.date,minutes:Math.round(candidate.activeMinutes)})
      :t("workLog.sourceHint.recap",{title:candidate.title,date:candidate.date}));
  };
  const change=(patch:Partial<WorkNoteInput>)=>{setNote(n=>n?{...n,...patch,approve:false}:null);setPreview("");setMessage("");};
  const edit=(entry:WorkNote)=>{
    setNote({id:entry.id,date:entry.date,context:entry.context,project:entry.project,outcome:entry.outcome,status:entry.status,role:entry.role,source_activity_id:entry.source_activity_id,expected_revision:entry.revision,approve:false});
    setPreview("");setMessage("");setDeleteId(null);setSourceHint("");
  };
  const save=async()=>{
    if(!note)return;setBusy(true);setError("");setPreview("");
    try { await saveWorkNote(note);setNote(null);setRevision(n=>n+1);setMessage(t("workLog.savedNotice")); }
    catch(e){setError(errorText(e,"workLog.saveFailed"))}
    finally{setBusy(false)}
  };
  const remove=async(id:string,revision:number)=>{
    setBusy(true);setError("");setPreview("");
    try{await deleteWorkNote(id,revision);setDeleteId(null);setNote(null);setRevision(n=>n+1);setMessage(t("workLog.deleted"))}
    catch(e){setError(errorText(e,"workLog.deleteFailed"))}
    finally{setBusy(false)}
  };
  const exportReport=async(copy:boolean)=>{
    setBusy(true);setError("");setPreview("");setMessage("");
    try {
      // Never export the cached list. Deletion/approval changes must be observed.
      const fresh=await fetchWorkLog(range.from,range.to);
      const text=reportMarkdown(fresh,range.from,range.to,project);
      setSnapshot(fresh);setPreview(text);
      if(copy){
        if("__TAURI_INTERNALS__" in window){
          const result=await commands.copyTextToClipboard(text);
          if(result.status!=="ok")throw new Error(t("workLog.clipboardFailed"));
        }else{await navigator.clipboard.writeText(text)}
        setMessage(t("workLog.copied"));}
    }catch(e){setError(e instanceof Error&&e.message===t("workLog.clipboardFailed")?e.message:errorText(e,"workLog.reportFailed"))}
    finally{setBusy(false)}
  };
  const entries=(snapshot?.entries||[]).filter(e=>(context==="all"||e.context===context)&&(!project||e.project===project));
  return <Dialog open={open} onOpenChange={value=>{if(busy)return;if(!value&&note){setMessage(t("workLog.closeBlocked"));return;}setOpen(value);if(value){setNote(null);setMessage("");setPreview("");setDeleteId(null)}}}>
    <DialogTrigger asChild><Button size="sm" variant="outline">{t("workLog.button")}</Button></DialogTrigger>
    <DialogContent className="max-w-4xl max-h-[90vh] overflow-y-auto">
      <DialogHeader><DialogTitle>{t("workLog.title")}</DialogTitle><DialogDescription>{t("workLog.description")}</DialogDescription></DialogHeader>
      <p className="text-xs text-muted-foreground">{t("workLog.scopeNote")}</p>
      <fieldset disabled={busy} className="space-y-4 min-w-0">
        <div className="flex flex-wrap gap-2 items-end">
          <Button variant="outline" size="sm" onClick={()=>{setRange(quarterRange());setProject("")}}>{t("workLog.thisQuarter")}</Button>
          <Button variant="outline" size="sm" onClick={()=>{setRange(quarterRange(new Date(),-1));setProject("")}}>{t("workLog.previousQuarter")}</Button>
          <label className="text-xs">{t("workLog.from")}<Input type="date" aria-label={t("workLog.fromAria")} value={range.from} onChange={e=>setRange({...range,from:e.target.value})}/></label>
          <label className="text-xs">{t("workLog.to")}<Input type="date" aria-label={t("workLog.toAria")} value={range.to} onChange={e=>setRange({...range,to:e.target.value})}/></label>
          <label className="flex flex-col text-xs gap-1">{t("workLog.showNotes")}<select className={fieldClass} aria-label={t("workLog.contextFilterAria")} value={context} onChange={e=>{setContext(e.target.value as WorkContext|"all");setPreview("")}}>
            <option value="all">{t("workLog.allContexts")}</option>{contexts.map(v=><option key={v} value={v}>{enumLabel(t,"context",v)}</option>)}
          </select></label>
          <label className="flex flex-col text-xs gap-1">{t("workLog.project")}<select className={fieldClass} aria-label={t("workLog.projectFilterAria")} value={project} onChange={e=>{setProject(e.target.value);setPreview("")}}>
            <option value="">{t("workLog.allProjects")}</option>{Array.from(new Set((snapshot?.entries||[]).map(n=>n.project).filter(Boolean))).sort().map(v=><option key={v} value={v}>{v}</option>)}
          </select></label>
        </div>
        <p className="text-xs text-muted-foreground">{t("workLog.datesNote")}</p>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="outline" disabled={!!note} onClick={()=>{setNote(blankNote());setSourceHint("");setDeleteId(null);setPreview("")}}>{t("workLog.newNote")}</Button>
          <Button size="sm" variant="outline" disabled={!!note||loading||!snapshot} aria-pressed={suggestDays===7} onClick={()=>setSuggestDays(d=>d===7?0:7)}>{t("workLog.suggest",{count:7})}</Button>
          <Button size="sm" variant="outline" disabled={!!note||loading||!snapshot} aria-pressed={suggestDays===14} onClick={()=>setSuggestDays(d=>d===14?0:14)}>{t("workLog.suggest",{count:14})}</Button>
          <Button size="sm" variant="outline" onClick={()=>setRevision(n=>n+1)}>{t("workLog.refresh")}</Button>
          <Button size="sm" variant="outline" disabled={loading||!snapshot||snapshot.truncated||!!note||(context!=="all"&&context!=="work")} onClick={()=>void exportReport(false)}>{t("workLog.preview")}</Button>
          <Button size="sm" disabled={loading||!snapshot||snapshot.truncated||!!note||(context!=="all"&&context!=="work")} onClick={()=>void exportReport(true)}>{t("workLog.copy")}</Button>
        </div>
        <p className="text-xs text-muted-foreground">{t("workLog.exportNote")}</p>
        {note && <section className="border rounded-md p-4 space-y-3" aria-label={t("workLog.editAria")}>
          <h3 className="font-medium">{t("workLog.yourContribution")}</h3>
          {sourceHint&&<p className="text-xs text-muted-foreground" data-testid="work-log-source-hint">{sourceHint}</p>}
          {note.source_activity_id!=null&&<p className="text-xs text-muted-foreground">{t("workLog.linkedNote")}</p>}
          <div className="grid grid-cols-2 gap-3">
            <label className="text-sm">{t("workLog.date")}<Input aria-label={t("workLog.dateAria")} type="date" value={note.date} onChange={e=>change({date:e.target.value})}/></label>
            <label className="text-sm">{t("workLog.project")}<Input aria-label={t("workLog.projectAria")} maxLength={120} value={note.project} onChange={e=>change({project:e.target.value})}/></label>
            <label className="flex flex-col text-sm">{t("workLog.context")}<select aria-label={t("workLog.contextAria")} className={fieldClass} value={note.context} onChange={e=>change({context:e.target.value as WorkContext})}>{contexts.map(v=><option key={v} value={v}>{enumLabel(t,"context",v)}</option>)}</select></label>
            <label className="flex flex-col text-sm">{t("workLog.role")}<select aria-label={t("workLog.role")} className={fieldClass} value={note.role} onChange={e=>change({role:e.target.value as WorkNoteInput["role"]})}>{roles.map(v=><option key={v} value={v}>{enumLabel(t,"role",v)}</option>)}</select></label>
            <label className="flex flex-col text-sm">{t("workLog.status")}<select aria-label={t("workLog.status")} className={fieldClass} value={note.status} onChange={e=>change({status:e.target.value as WorkNoteInput["status"]})}>{statuses.map(v=><option key={v} value={v}>{enumLabel(t,"status",v)}</option>)}</select></label>
          </div>
          <label className="block text-sm">{t("workLog.outcome")}<Textarea aria-label={t("workLog.outcomeAria")} maxLength={2000} rows={4} value={note.outcome} placeholder={t("workLog.outcomePlaceholder")} onChange={e=>change({outcome:e.target.value})}/></label>
          <p className="text-xs text-muted-foreground">{t("workLog.outcomeHelp")}</p>
          <label className="flex gap-2 text-sm items-start"><input type="checkbox" aria-label={t("workLog.approveAria")} checked={note.approve} disabled={!canApprove(note)} onChange={e=>setNote({...note,approve:e.target.checked})}/>{t("workLog.approve")}</label>
          <div className="flex gap-2"><Button size="sm" onClick={()=>void save()}>{t("workLog.save")}</Button><Button size="sm" variant="outline" onClick={()=>{setNote(null);setSourceHint("")}}>{t("workLog.cancel")}</Button></div>
        </section>}
        {suggestDays>0&&!note&&<section className="border rounded-md p-3 space-y-2" aria-label={t("workLog.suggestionsAria")} data-testid="work-log-suggestions">
          <h3 className="font-medium text-sm">{t("workLog.suggestionsTitle",{count:suggestDays})}</h3>
          <p className="text-xs text-muted-foreground">{t("workLog.suggestionsHelp")}</p>
          {suggestions===null&&!suggestError&&<p role="status" className="text-sm">{t("workLog.suggestionsLoading")}</p>}
          {suggestError&&<p role="alert" className="text-sm text-destructive">{suggestError}</p>}
          {suggestions&&!suggestions.length&&!suggestError&&<p className="text-sm text-muted-foreground">{t("workLog.suggestionsEmpty")}</p>}
          {suggestions&&suggestions.length>0&&<ul className="space-y-1" aria-label={t("workLog.candidatesAria")}>{suggestions.map(c=><li key={c.key} className="flex items-start gap-2 text-sm">
            <span className="min-w-0 flex-1"><span className="text-xs text-muted-foreground">{c.date} · {c.kind==="card"?t("workLog.candidateMinutes",{minutes:Math.round(c.activeMinutes)}):t("workLog.candidateRecap")}{c.noted?` · ${t("workLog.noted")}`:""}</span><br/><span className="break-words">{c.title}</span></span>
            <Button size="sm" variant={c.noted?"ghost":"outline"} disabled={!!note} onClick={()=>draftFrom(c)}>{c.noted?t("workLog.addAnother"):t("workLog.draft")}</Button>
          </li>)}</ul>}
        </section>}
        {loading?<p role="status">{t("workLog.loading")}</p>:snapshot?.truncated?<p role="alert">{t("workLog.truncated")}</p>:null}
        {!loading&&snapshot&&!entries.length&&<p className="text-sm text-muted-foreground">{t("workLog.empty")}</p>}
        <ul className="space-y-2" aria-label={t("workLog.savedAria")}>{entries.map(entry=><li key={entry.id} className="border rounded-md p-3 space-y-2">
          <div className="text-xs text-muted-foreground">{entry.date} · {enumLabel(t,"context",entry.context)} · {enumLabel(t,"role",entry.role)} · {enumLabel(t,"status",entry.status)} · {entry.approved?t("workLog.approved"):t("workLog.notApproved")}</div>
          <p className="text-sm font-medium break-words">{entry.project||t("workLog.unassignedProject")}</p><p className="text-sm whitespace-pre-wrap break-words">{entry.outcome||t("workLog.noOutcome")}</p>
          <div className="flex gap-2"><Button size="sm" variant="outline" disabled={!!note} onClick={()=>edit(entry)}>{t("workLog.edit")}</Button><Button size="sm" variant="ghost" disabled={!!note} onClick={()=>setDeleteId(entry.id)}>{t("workLog.delete")}</Button>
          {deleteId===entry.id&&<><Button size="sm" variant="destructive" disabled={!!note} onClick={()=>void remove(entry.id,entry.revision)}>{t("workLog.confirmDelete")}</Button><Button size="sm" variant="ghost" onClick={()=>setDeleteId(null)}>{t("workLog.keep")}</Button></>}</div>
        </li>)}</ul>
      </fieldset>
      {error&&<p role="alert" className="text-sm text-destructive">{error}</p>}{message&&<p role="status" className="text-sm">{message}</p>}
      {preview&&<pre aria-label={t("workLog.previewAria")} className="border p-4 whitespace-pre-wrap break-words text-xs">{preview}</pre>}
    </DialogContent>
  </Dialog>;
}
