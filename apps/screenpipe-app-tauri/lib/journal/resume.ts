// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

/**
 * "Continue work": pure logic behind the resume panel.
 *
 * Everything here is derived from journal cards, their evidence and the day's
 * recap the engine already holds. Nothing is generated: a candidate task is a
 * group of cards that share a title, a source is a deduplicated evidence row,
 * and the only model-written text is the recap's `next` list, which the panel
 * labels as a suggestion. Observation proves a person saw something; it never
 * proves the task is done or what the next step is.
 */

import type {
  ActivityCard,
  ActivityEvidence,
  JournalDay,
  JournalRecap,
} from "./types";
import { journalDayToday, shiftJournalDay } from "./format";

export interface ResumeCandidate {
  /** Stable within one panel load: normalized title + project. */
  key: string;
  title: string;
  project: string | null;
  /** The most recent card of the group; its summary is "where you stopped". */
  lastCard: ActivityCard;
  /** Journal date (`YYYY-MM-DD`) of `lastCard`. */
  lastDate: string;
  lastSeenAt: string;
  cardIds: number[];
  /** Distinct journal dates the task appeared on, ascending. */
  dates: string[];
  activeMinutes: number;
}

export interface ResumeSource {
  /** Empty when the frame carried no app name; the UI names it as unknown. */
  app: string;
  windowTitle: string | null;
  browserUrl: string | null;
  occurredAt: string;
  frameId: number | null;
  /** How many evidence rows collapsed into this source. */
  hits: number;
}

/** The last `count` journal dates ending today, oldest first. */
export function resumeDates(now = new Date(), count = 3): string[] {
  const today = journalDayToday(now);
  const dates: string[] = [];
  for (let offset = count - 1; offset >= 0; offset -= 1) {
    dates.push(shiftJournalDay(today, -offset));
  }
  return dates;
}

function normalizeTitle(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim()
    .replace(/\s+/g, " ");
}

/**
 * Cards that can be resumed: real work, not idle, not the engine's own system
 * cards (the "could not summarize" placeholder), not a distraction, and not a
 * break or detour the classifier flagged. `unknown` and `other_work` stay in —
 * other work is still work someone may want to return to. The contribution
 * candidates reuse this and add their own `personal` exclusion.
 */
export function isResumableCard(card: ActivityCard): boolean {
  if (card.category.is_idle || card.category.is_system) return false;
  if (card.category.id === "distraction") return false;
  if (card.intention_relation === "break" || card.intention_relation === "possible_distraction") {
    return false;
  }
  return card.title.trim().length > 0;
}

/** The project of the intention this card was classified against, if any. */
export function cardProject(day: JournalDay, card: ActivityCard): string | null {
  if (!card.intention) return null;
  const intention = day.intentions.find((row) => row.id === card.intention?.id);
  return intention?.project ?? null;
}

/**
 * Group resumable cards across the loaded days by title (and intention
 * project when the card had one), most recently seen first.
 */
export function collectResumeCandidates(days: JournalDay[], limit = 8): ResumeCandidate[] {
  const groups = new Map<string, ResumeCandidate>();
  for (const day of days) {
    for (const card of day.activities) {
      if (!isResumableCard(card)) continue;
      const project = cardProject(day, card);
      const key = `${normalizeTitle(card.title)}|${(project ?? "").toLowerCase()}`;
      const existing = groups.get(key);
      if (!existing) {
        groups.set(key, {
          key,
          title: card.title,
          project,
          lastCard: card,
          lastDate: day.date,
          lastSeenAt: card.end_at,
          cardIds: [card.id],
          dates: [day.date],
          activeMinutes: card.active_minutes,
        });
        continue;
      }
      existing.cardIds.push(card.id);
      if (!existing.dates.includes(day.date)) existing.dates.push(day.date);
      existing.activeMinutes += card.active_minutes;
      if (card.end_at > existing.lastSeenAt) {
        existing.lastCard = card;
        existing.lastDate = day.date;
        existing.lastSeenAt = card.end_at;
        existing.title = card.title;
      }
    }
  }
  return Array.from(groups.values())
    .map((candidate) => ({ ...candidate, dates: [...candidate.dates].sort() }))
    .sort((a, b) => b.lastSeenAt.localeCompare(a.lastSeenAt))
    .slice(0, limit);
}

/**
 * Collapse sampled evidence into the distinct materials the task touched:
 * one row per (app, window title, URL), keeping the latest moment so "open in
 * timeline" lands where the person last was. Audio rows have no material to
 * reopen and are left out.
 */
export function dedupeSources(evidence: ActivityEvidence[], limit = 6): ResumeSource[] {
  const byKey = new Map<string, ResumeSource>();
  for (const row of evidence) {
    if (row.source_type !== "frame") continue;
    const app = row.app_name?.trim() || "";
    if (!app && !row.window_title && !row.browser_url) continue;
    const key = `${app.toLowerCase()}|${(row.window_title ?? "").toLowerCase()}|${(row.browser_url ?? "").toLowerCase()}`;
    const existing = byKey.get(key);
    if (!existing) {
      byKey.set(key, {
        app,
        windowTitle: row.window_title,
        browserUrl: row.browser_url,
        occurredAt: row.occurred_at,
        frameId: row.frame_id,
        hits: 1,
      });
      continue;
    }
    existing.hits += 1;
    if (row.occurred_at > existing.occurredAt) {
      existing.occurredAt = row.occurred_at;
      existing.frameId = row.frame_id;
    }
  }
  return Array.from(byKey.values())
    .sort((a, b) => b.hits - a.hits || b.occurredAt.localeCompare(a.occurredAt))
    .slice(0, limit);
}

/**
 * The recap's `next` bullets are the only "open question / next step" the
 * engine has. They come from a model, so the caller must label them as a
 * suggestion; a missing or stale recap yields nothing rather than a guess.
 */
export function suggestedNextSteps(recap: JournalRecap | null): string[] {
  if (!recap) return [];
  if (recap.status !== "ready" && recap.status !== "stale") return [];
  return recap.next.filter((line) => line.trim().length > 0).slice(0, 4);
}
