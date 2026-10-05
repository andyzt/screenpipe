// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

/**
 * Contribution candidates: what the journal saw that might be worth a work
 * note. A candidate is a pointer to evidence, not a claim: the person still
 * writes what changed, names their role and approves it. Card text is shown
 * beside the editor as a hint and never copied into the note automatically.
 */

import type { ActivityCard, JournalDay, JournalRecap } from "./types";
import type { WorkNote } from "./work-log";
import { cardProject, isResumableCard } from "./resume";
import { shiftJournalDay } from "./format";

export type ContributionCandidate =
  | {
      kind: "card";
      key: string;
      date: string;
      title: string;
      /** Most recent card of the group, used to link the note's source. */
      card: ActivityCard;
      cardIds: number[];
      activeMinutes: number;
      project: string | null;
      /** A saved note already links one of these cards. */
      noted: boolean;
    }
  | {
      kind: "recap";
      key: string;
      date: string;
      /** One `done` bullet of the day's recap; model-written. */
      title: string;
      noted: false;
    };

/** Minimum active minutes for a card group to be worth suggesting. */
export const MIN_CANDIDATE_MINUTES = 10;

function normalize(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim()
    .replace(/\s+/g, " ");
}

/**
 * Same floor as the resume panel, plus `personal`: a work report must never be
 * seeded from a card the classifier already called personal, even if the
 * person could still write a Work note about it by hand.
 */
function isContributionCard(card: ActivityCard): boolean {
  return isResumableCard(card) && card.category.id !== "personal";
}

/**
 * Group each day's contribution cards by title, drop groups under the minute
 * floor, and mark groups a saved note already covers. Recap `done` bullets
 * are appended per day so work the cards missed can still be picked up.
 * Newest day first, longest group first within a day.
 */
export function contributionCandidates(
  days: JournalDay[],
  recaps: Map<string, JournalRecap>,
  notes: WorkNote[],
): ContributionCandidate[] {
  const notedIds = new Set(
    notes.map((note) => note.source_activity_id).filter((id): id is number => id !== null),
  );
  const out: ContributionCandidate[] = [];
  const ordered = [...days].sort((a, b) => b.date.localeCompare(a.date));
  for (const day of ordered) {
    const groups = new Map<string, Extract<ContributionCandidate, { kind: "card" }>>();
    for (const card of day.activities) {
      if (!isContributionCard(card)) continue;
      const key = `${day.date}|${normalize(card.title)}`;
      const existing = groups.get(key);
      if (!existing) {
        groups.set(key, {
          kind: "card",
          key,
          date: day.date,
          title: card.title,
          card,
          cardIds: [card.id],
          activeMinutes: card.active_minutes,
          project: cardProject(day, card),
          noted: notedIds.has(card.id),
        });
        continue;
      }
      existing.cardIds.push(card.id);
      existing.activeMinutes += card.active_minutes;
      existing.noted = existing.noted || notedIds.has(card.id);
      if (card.end_at > existing.card.end_at) {
        existing.card = card;
        existing.title = card.title;
      }
    }
    const dayCandidates = Array.from(groups.values())
      .filter((group) => group.activeMinutes >= MIN_CANDIDATE_MINUTES)
      .sort((a, b) => b.activeMinutes - a.activeMinutes);
    out.push(...dayCandidates);

    const recap = recaps.get(day.date);
    if (recap && (recap.status === "ready" || recap.status === "stale")) {
      recap.done.forEach((line, index) => {
        const text = line.trim();
        if (!text) return;
        // Skip a bullet that restates a card already listed for the day.
        const restated = dayCandidates.some(
          (candidate) => normalize(candidate.title) === normalize(text),
        );
        if (restated) return;
        out.push({
          kind: "recap",
          key: `${day.date}|recap|${index}`,
          date: day.date,
          title: text,
          noted: false,
        });
      });
    }
  }
  return out;
}

/** The last `count` dates ending on `today`, oldest first. */
export function recentDates(today: string, count: number): string[] {
  const dates: string[] = [];
  for (let offset = count - 1; offset >= 0; offset -= 1) {
    dates.push(shiftJournalDay(today, -offset));
  }
  return dates;
}
