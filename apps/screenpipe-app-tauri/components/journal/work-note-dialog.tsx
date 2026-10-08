// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
"use client";

import React from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { useT } from "@/lib/i18n";
import { fetchWorkNotes, saveWorkNote, type WorkNote, type WorkNoteInput } from "@/lib/journal/work-note";
import type { ActivityCard } from "@/lib/journal/types";

/** Notes belong to the person, independently of the model-written card. */
export function WorkNoteDialog({ card }: { card: ActivityCard }) {
  const t = useT();
  const [open, setOpen] = React.useState(false);
  const [loading, setLoading] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const [entries, setEntries] = React.useState<WorkNote[]>([]);
  const [draft, setDraft] = React.useState<WorkNoteInput | null>(null);
  const [error, setError] = React.useState("");
  const [saved, setSaved] = React.useState(false);
  const start = new Date(card.start_at);
  const date = `${start.getFullYear()}-${String(start.getMonth() + 1).padStart(2, "0")}-${String(start.getDate()).padStart(2, "0")}`;

  const blank = (): WorkNoteInput => ({
    id: crypto.randomUUID(), expected_revision: null, date,
    context: "work", project: "", outcome: "", status: "in_progress",
    role: "unknown", source_activity_id: card.id, approve: false,
  });

  React.useEffect(() => {
    if (!open) return;
    const controller = new AbortController();
    setLoading(true);
    setError("");
    void fetchWorkNotes(date, controller.signal).then((result) => {
      if (controller.signal.aborted) return;
      if (result.truncated) throw new Error("truncated");
      setEntries(result.entries.filter((entry) => entry.source_activity_id === card.id ||
        (entry.source_start_at === card.start_at && entry.source_end_at === card.end_at)));
    }).catch(() => {
      if (!controller.signal.aborted) setError(t("workNote.loadFailed"));
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false);
    });
    return () => controller.abort();
  }, [open, date, card.id, card.start_at, card.end_at, t]);

  const save = async () => {
    if (!draft || !draft.outcome.trim() || saving) return;
    setSaving(true);
    setError("");
    try {
      const { entry } = await saveWorkNote({ ...draft, outcome: draft.outcome.trim(), project: draft.project.trim() });
      setEntries((current) => [...current.filter((note) => note.id !== entry.id), entry]);
      setDraft(null);
      setSaved(true);
    } catch (reason) {
      setError(t(reason instanceof Error && reason.message === "409" ? "workNote.conflict" : "workNote.saveFailed"));
    } finally {
      setSaving(false);
    }
  };

  return <>
    <Button size="sm" variant="outline" onClick={() => { setDraft(blank()); setSaved(false); setOpen(true); }}>
      {t("resume.addNote")}
    </Button>
    <Dialog open={open} onOpenChange={(value) => { if (!saving) setOpen(value); }}>
      <DialogContent className="max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("resume.addNote")}</DialogTitle>
          <DialogDescription>{t("workNote.description")}</DialogDescription>
        </DialogHeader>
        {loading ? <p role="status" className="text-sm">{t("workNote.loading")}</p> : null}
        <ul className="space-y-3" aria-label={t("workNote.savedNotes")}>
          {entries.map((entry) => <li key={entry.id} className="rounded-md border p-3 text-sm">
            {entry.project ? <p className="font-medium break-words">{entry.project}</p> : null}
            <p className="whitespace-pre-wrap break-words">{entry.outcome}</p>
            <Button size="sm" variant="ghost" disabled={saving} onClick={() => {
              setDraft({ id: entry.id, expected_revision: entry.revision, date: entry.date,
                context: entry.context, project: entry.project, outcome: entry.outcome,
                status: entry.status, role: entry.role, source_activity_id: entry.source_activity_id, approve: false });
              setSaved(false);
            }}>{t("workNote.edit")}</Button>
          </li>)}
        </ul>
        {draft ? <fieldset disabled={saving} className="space-y-3 min-w-0">
          <label className="block space-y-1 text-sm">
            <span>{t("workNote.project")}</span>
            <Input maxLength={120} value={draft.project} onChange={(event) => setDraft({ ...draft, project: event.target.value })} />
          </label>
          <label className="block space-y-1 text-sm">
            <span>{t("workNote.text")}</span>
            <Textarea autoFocus rows={5} maxLength={2000} className="border-border"
              value={draft.outcome} onChange={(event) => setDraft({ ...draft, outcome: event.target.value })} />
          </label>
          <div className="flex gap-2">
            <Button disabled={!draft.outcome.trim() || saving || loading} onClick={() => void save()}>{t(saving ? "workNote.saving" : "workNote.save")}</Button>
            <Button variant="outline" onClick={() => setOpen(false)}>{t("workNote.cancel")}</Button>
          </div>
        </fieldset> : <Button variant="outline" onClick={() => { setDraft(blank()); setSaved(false); }}>{t("resume.addNote")}</Button>}
        {error ? <p role="alert" className="text-sm text-destructive">{error}</p> : null}
        {saved ? <p role="status" className="text-sm">{t("workNote.saved")}</p> : null}
      </DialogContent>
    </Dialog>
  </>;
}
