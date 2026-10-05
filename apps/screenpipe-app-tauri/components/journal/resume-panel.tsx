// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
"use client";

/**
 * "Continue work": the card that brings a person back to a task.
 *
 * Reads the last three journal days, groups their cards into tasks, and for
 * the chosen task shows where the person stopped (the last card's summary),
 * the materials it touched (deduplicated evidence) and the recap's suggested
 * next steps. Titles, summaries and next steps are model-written from screen
 * observation and are labelled as such. Actions reuse what exists: "Continue
 * this task" starts an intention with a title the person can edit first (and
 * says which intention it replaces), "Add work note" opens the work log editor
 * linked to the last card, every material opens the timeline at that moment,
 * and "Open that day" lands on the day with the card selected. Nothing here
 * is generated on demand, so opening the panel costs no model call.
 */

import React, { useCallback, useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { emit } from "@tauri-apps/api/event";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { useTimelineStore } from "@/lib/hooks/use-timeline-store";
import { trackTraction } from "@/lib/analytics/traction";
import {
  createIntention,
  fetchActiveIntention,
  fetchActivityDetail,
  fetchJournalDay,
  fetchRecap,
} from "@/lib/journal/api";
import {
  collectResumeCandidates,
  dedupeSources,
  resumeDates,
  suggestedNextSteps,
  type ResumeCandidate,
  type ResumeSource,
} from "@/lib/journal/resume";
import { formatClock, formatJournalDayLabel, formatMinutes } from "@/lib/journal/format";
import { useLocale, useT } from "@/lib/i18n";
import type { Intention, JournalDay, JournalRecap } from "@/lib/journal/types";

type LoadState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; days: JournalDay[]; candidates: ResumeCandidate[] };

/** How long the closing dialog is given before the work-note dialog opens. */
const HANDOFF_DELAY_MS = 200;

export function ResumePanel({
  onOpenDay,
  onIntentionCreated,
}: {
  /** Navigate the journal to a date with a card selected; the panel closes first. */
  onOpenDay?: (date: string, cardId: number) => void;
  /** An intention was started from here; the host refreshes its strips. */
  onIntentionCreated?: (intention: Intention) => void;
}) {
  const t = useT();
  const locale = useLocale();
  const router = useRouter();
  const setPendingNavigation = useTimelineStore((s) => s.setPendingNavigation);
  const [open, setOpen] = useState(false);
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [sources, setSources] = useState<ResumeSource[] | null>(null);
  const [sourcesError, setSourcesError] = useState<string | null>(null);
  const [recap, setRecap] = useState<JournalRecap | null>(null);
  const [activeIntention, setActiveIntention] = useState<Intention | null>(null);
  const [draftTitle, setDraftTitle] = useState("");
  const [draftProject, setDraftProject] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    const controller = new AbortController();
    setState({ status: "loading" });
    setSelectedKey(null);
    setMessage(null);
    setActiveIntention(null);
    const dates = resumeDates();
    Promise.all(dates.map((date) => fetchJournalDay(date, controller.signal)))
      .then((days) => {
        if (controller.signal.aborted) return;
        const candidates = collectResumeCandidates(days);
        setState({ status: "ready", days, candidates });
        setSelectedKey(candidates[0]?.key ?? null);
      })
      .catch((reason: unknown) => {
        if (controller.signal.aborted) return;
        setState({
          status: "error",
          message: reason instanceof Error ? reason.message : String(reason),
        });
      });
    fetchActiveIntention(controller.signal)
      .then((next) => {
        if (!controller.signal.aborted) setActiveIntention(next);
      })
      .catch(() => {
        // Without the answer the panel simply cannot say what it replaces.
      });
    return () => controller.abort();
  }, [open]);

  const selected = useMemo(
    () =>
      state.status === "ready"
        ? (state.candidates.find((candidate) => candidate.key === selectedKey) ?? null)
        : null,
    [state, selectedKey],
  );

  useEffect(() => {
    setMessage(null);
    if (!selected) {
      setSources(null);
      setRecap(null);
      setDraftTitle("");
      setDraftProject("");
      return;
    }
    setDraftTitle(selected.title);
    setDraftProject(selected.project ?? "");
    const controller = new AbortController();
    setSources(null);
    setSourcesError(null);
    setRecap(null);
    fetchActivityDetail(selected.lastCard.id, controller.signal)
      .then((detail) => {
        if (controller.signal.aborted) return;
        setSources(dedupeSources(detail.evidence));
      })
      .catch((reason: unknown) => {
        if (controller.signal.aborted) return;
        setSources([]);
        setSourcesError(reason instanceof Error ? reason.message : String(reason));
      });
    // The day stub already says whether a recap exists; only read one that does.
    const dayStub =
      state.status === "ready"
        ? state.days.find((day) => day.date === selected.lastDate)
        : undefined;
    const recapStatus = dayStub?.recap.status;
    if (recapStatus === "ready" || recapStatus === "stale") {
      fetchRecap(selected.lastDate, controller.signal)
        .then((next) => {
          if (!controller.signal.aborted) setRecap(next);
        })
        .catch(() => {
          // A missing recap only removes the suggestion block.
        });
    }
    return () => controller.abort();
  }, [selected, state]);

  const openInTimeline = useCallback(
    (source: ResumeSource) => {
      const frame = source.frameId ? String(source.frameId) : undefined;
      setPendingNavigation({ timestamp: source.occurredAt, frameId: frame });
      setOpen(false);
      router.push("/home?section=timeline");
      trackTraction("evidence_opened", { surface: "timeline" });
      window.setTimeout(() => {
        if (frame) void emit("navigate-to-frame", frame);
        else void emit("navigate-to-timestamp", source.occurredAt);
      }, 250);
    },
    [router, setPendingNavigation],
  );

  const continueTask = async () => {
    const title = draftTitle.trim();
    if (!selected || !title) return;
    setBusy(true);
    setMessage(null);
    try {
      const created = await createIntention({
        title,
        project: draftProject.trim() || undefined,
      });
      setActiveIntention(created);
      onIntentionCreated?.(created);
      setMessage(t("resume.continued", { title }));
    } catch (reason) {
      setMessage(
        t("resume.continueFailed", {
          error: reason instanceof Error ? reason.message : String(reason),
        }),
      );
    } finally {
      setBusy(false);
    }
  };

  const addWorkNote = () => {
    if (!selected) return;
    const detail = { id: selected.lastCard.id, start_at: selected.lastCard.start_at };
    setOpen(false);
    // Let this dialog finish closing before the work-note dialog takes focus.
    window.setTimeout(() => {
      window.dispatchEvent(new CustomEvent("journal-add-work-note", { detail }));
    }, HANDOFF_DELAY_MS);
  };

  const nextSteps = suggestedNextSteps(recap);
  const sameAsActive =
    !!activeIntention &&
    activeIntention.title.trim() === draftTitle.trim() &&
    (activeIntention.project ?? "").trim() === draftProject.trim();
  const replaces = activeIntention && selected && !sameAsActive ? activeIntention : null;

  return (
    <Dialog open={open} onOpenChange={(value) => { if (!busy) setOpen(value); }}>
      <DialogTrigger asChild>
        <Button size="sm" variant="outline" data-testid="journal-resume-open">
          {t("resume.button")}
        </Button>
      </DialogTrigger>
      <DialogContent className="max-w-4xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("resume.title")}</DialogTitle>
          <DialogDescription>{t("resume.description")}</DialogDescription>
        </DialogHeader>

        {state.status === "loading" ? (
          <p role="status" className="text-sm text-muted-foreground">{t("resume.loading")}</p>
        ) : null}
        {state.status === "error" ? (
          <p role="alert" className="text-sm text-destructive">
            {t("resume.error", { error: state.message })}
          </p>
        ) : null}
        {state.status === "ready" && state.candidates.length === 0 ? (
          <p className="text-sm text-muted-foreground" data-testid="journal-resume-empty">
            {t("resume.empty")}
          </p>
        ) : null}

        {state.status === "ready" && state.candidates.length > 0 ? (
          <div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
            <ul className="flex flex-col gap-1" aria-label={t("resume.candidatesAria")}>
              {state.candidates.map((candidate) => (
                <li key={candidate.key}>
                  <button
                    type="button"
                    data-testid="journal-resume-candidate"
                    aria-pressed={candidate.key === selectedKey}
                    onClick={() => setSelectedKey(candidate.key)}
                    className={
                      "w-full rounded-md border px-3 py-2 text-left " +
                      (candidate.key === selectedKey
                        ? "border-foreground bg-muted"
                        : "border-border hover:bg-muted/50")
                    }
                  >
                    <span className="block truncate text-sm font-medium">{candidate.title}</span>
                    <span className="block text-xs text-muted-foreground">
                      {formatJournalDayLabel(candidate.lastDate, new Date(), locale)} · {formatClock(candidate.lastSeenAt, locale)}
                      {" · "}
                      {t("resume.activeEstimate", { minutes: formatMinutes(candidate.activeMinutes, locale) })}
                      {candidate.dates.length > 1
                        ? ` · ${t("resume.days", { count: candidate.dates.length })}`
                        : ""}
                    </span>
                  </button>
                </li>
              ))}
            </ul>

            {selected ? (
              <section className="flex flex-col gap-3" aria-label={t("resume.detailAria")} data-testid="journal-resume-detail">
                <div>
                  <h3 className="text-base font-semibold">{selected.title}</h3>
                  {selected.project ? (
                    <p className="text-xs text-muted-foreground">{t("resume.project", { project: selected.project })}</p>
                  ) : null}
                  <p className="text-xs text-muted-foreground" data-testid="journal-resume-model-caveat">
                    {t("resume.modelCaveat")}
                  </p>
                </div>

                <div>
                  <h4 className="text-xs uppercase tracking-wide text-muted-foreground">{t("resume.lastSeen")}</h4>
                  <p className="text-sm">
                    {formatJournalDayLabel(selected.lastDate, new Date(), locale)}, {formatClock(selected.lastCard.start_at, locale)}–{formatClock(selected.lastCard.end_at, locale)}
                    {selected.lastCard.state === "provisional" ? ` · ${t("resume.provisional")}` : ""}
                  </p>
                  <p className="text-sm text-muted-foreground">{selected.lastCard.summary}</p>
                  {selected.lastCard.detailed_summary ? (
                    <p className="mt-1 text-sm text-muted-foreground">{selected.lastCard.detailed_summary}</p>
                  ) : null}
                </div>

                <div>
                  <h4 className="text-xs uppercase tracking-wide text-muted-foreground">{t("resume.materials")}</h4>
                  {sources === null && !sourcesError ? (
                    <p className="text-sm text-muted-foreground">{t("resume.materialsLoading")}</p>
                  ) : null}
                  {sourcesError ? (
                    <p className="text-sm text-destructive" role="alert">{t("resume.materialsError", { error: sourcesError })}</p>
                  ) : null}
                  {sources && sources.length === 0 && !sourcesError ? (
                    <p className="text-sm text-muted-foreground">{t("resume.materialsEmpty")}</p>
                  ) : null}
                  {sources && sources.length > 0 ? (
                    <ul className="flex flex-col divide-y divide-border">
                      {sources.map((source) => (
                        <li key={`${source.app}|${source.windowTitle}|${source.browserUrl}`} className="flex items-start gap-2 py-1.5" data-testid="journal-resume-source">
                          <div className="min-w-0 flex-1">
                            <span className="block truncate text-sm font-medium">{source.app || t("resume.unknownApp")}</span>
                            {source.windowTitle ? <span className="block truncate text-sm text-muted-foreground">{source.windowTitle}</span> : null}
                            {source.browserUrl ? <span className="block truncate text-xs text-muted-foreground">{source.browserUrl}</span> : null}
                          </div>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-7 shrink-0 px-2"
                            disabled={!source.frameId}
                            data-testid="journal-resume-open-timeline"
                            onClick={() => openInTimeline(source)}
                          >
                            {t("evidence.openInTimeline")}
                          </Button>
                        </li>
                      ))}
                    </ul>
                  ) : null}
                </div>

                <div>
                  <h4 className="text-xs uppercase tracking-wide text-muted-foreground">{t("resume.nextSteps")}</h4>
                  {nextSteps.length > 0 ? (
                    <>
                      <ul className="list-disc pl-5 text-sm" data-testid="journal-resume-next">
                        {nextSteps.map((line) => <li key={line}>{line}</li>)}
                      </ul>
                      <p className="text-xs text-muted-foreground">{t("resume.nextStepsCaveat")}</p>
                    </>
                  ) : (
                    <p className="text-sm text-muted-foreground">{t("resume.nextStepsEmpty")}</p>
                  )}
                </div>

                <div className="flex flex-col gap-2 rounded-md border border-border p-3">
                  <h4 className="text-xs uppercase tracking-wide text-muted-foreground">{t("resume.continueHeading")}</h4>
                  <div className="grid gap-2 sm:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
                    <label className="text-xs text-muted-foreground">
                      {t("resume.editTitle")}
                      <Input
                        aria-label={t("resume.editTitle")}
                        data-testid="journal-resume-title"
                        maxLength={200}
                        value={draftTitle}
                        onChange={(event) => setDraftTitle(event.target.value)}
                      />
                    </label>
                    <label className="text-xs text-muted-foreground">
                      {t("resume.editProject")}
                      <Input
                        aria-label={t("resume.editProject")}
                        data-testid="journal-resume-project"
                        maxLength={120}
                        value={draftProject}
                        onChange={(event) => setDraftProject(event.target.value)}
                      />
                    </label>
                  </div>
                  {sameAsActive ? (
                    <p className="text-xs text-muted-foreground" data-testid="journal-resume-already-active">
                      {t("resume.alreadyActive")}
                    </p>
                  ) : null}
                  {replaces ? (
                    <p className="text-xs text-foreground" data-testid="journal-resume-replaces">
                      {t("resume.replaces", { title: replaces.title })}
                    </p>
                  ) : null}
                  <div className="flex flex-wrap gap-2">
                    <Button size="sm" disabled={busy || !draftTitle.trim() || sameAsActive} data-testid="journal-resume-continue" onClick={() => void continueTask()}>
                      {t("resume.continue")}
                    </Button>
                    <Button size="sm" variant="outline" disabled={busy} data-testid="journal-resume-add-note" onClick={addWorkNote}>
                      {t("resume.addNote")}
                    </Button>
                    {onOpenDay ? (
                      <Button size="sm" variant="ghost" disabled={busy} data-testid="journal-resume-open-day" onClick={() => { setOpen(false); onOpenDay(selected.lastDate, selected.lastCard.id); }}>
                        {t("resume.openDay")}
                      </Button>
                    ) : null}
                  </div>
                  {message ? <p role="status" className="text-sm">{message}</p> : null}
                </div>
              </section>
            ) : null}
          </div>
        ) : null}
        <p className="text-xs text-muted-foreground">{t("resume.footnote")}</p>
      </DialogContent>
    </Dialog>
  );
}
