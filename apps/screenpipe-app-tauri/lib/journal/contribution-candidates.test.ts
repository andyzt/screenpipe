// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
import { describe, expect, it } from "vitest";
import { contributionCandidates, recentDates } from "./contribution-candidates";
import { makeActivityCard, makeCategory, makeIntention, makeJournalDay, makeRecap } from "./fixtures";
import type { WorkNote } from "./work-log";

const note = (source_activity_id: number | null): WorkNote => ({
  id: "n1", revision: 1, date: "2026-09-22", context: "work", project: "", outcome: "x", status: "completed",
  role: "owner", approved: true, source_activity_id, source_activity_key: null, source_start_at: null,
  source_end_at: null, updated_at: "2026-09-22T10:00:00Z",
});

describe("recentDates", () => {
  it("counts back calendar dates across a month boundary", () => {
    expect(recentDates("2026-10-02", 4)).toEqual(["2026-09-29", "2026-09-30", "2026-10-01", "2026-10-02"]);
  });
});

describe("contributionCandidates", () => {
  it("groups work cards per day above the minute floor, marks noted groups, appends recap bullets", () => {
    const day1 = makeJournalDay({
      date: "2026-09-21",
      intentions: [makeIntention({ id: 8, project: "First" }), makeIntention({ id: 9, project: "Alpha" })],
      activities: [
        makeActivityCard({ id: 1, title: "Auth fix", active_minutes: 6, intention: { id: 9, title: "Ship" }, end_at: "2026-09-21T09:00:00Z" }),
        makeActivityCard({ id: 2, title: "Auth fix", active_minutes: 6, intention: { id: 9, title: "Ship" }, end_at: "2026-09-21T10:00:00Z" }),
        makeActivityCard({ id: 3, title: "Tiny thing", active_minutes: 4 }),
        makeActivityCard({ id: 4, title: "Scrolling", active_minutes: 30, category: makeCategory({ id: "distraction" }) }),
        makeActivityCard({ id: 5, title: "Idle", active_minutes: 30, category: makeCategory({ id: "idle", is_idle: true }) }),
        makeActivityCard({ id: 7, title: "Banking", active_minutes: 25, category: makeCategory({ id: "personal", name: "Personal" }) }),
        makeActivityCard({ id: 8, title: "Could not summarize this period", active_minutes: 0, category: makeCategory({ id: "system", is_system: true }) }),
      ],
      recap: { status: "ready", generated_at: "2026-09-21T18:00:00Z" },
    });
    const day2 = makeJournalDay({
      date: "2026-09-22",
      activities: [makeActivityCard({ id: 6, title: "Billing review", active_minutes: 45 })],
    });
    const recaps = new Map([[
      "2026-09-21",
      makeRecap({ date: "2026-09-21", done: ["Auth fix", "Helped QA reproduce the crash", " "] }),
    ]]);
    const out = contributionCandidates([day1, day2], recaps, [note(2)]);
    expect(out.map((c) => [c.kind, c.date, c.title])).toEqual([
      ["card", "2026-09-22", "Billing review"],
      ["card", "2026-09-21", "Auth fix"],
      ["recap", "2026-09-21", "Helped QA reproduce the crash"],
    ]);
    const auth = out[1];
    expect(auth.kind).toBe("card");
    if (auth.kind === "card") {
      expect(auth.cardIds).toEqual([1, 2]);
      expect(auth.activeMinutes).toBe(12);
      expect(auth.noted).toBe(true);
      expect(auth.project).toBe("Alpha");
      expect(auth.card.id).toBe(2);
    }
    expect(out[0].noted).toBe(false);
  });

  it("counts back across a month boundary using the shared day helper", () => {
    expect(recentDates("2026-03-02", 3)).toEqual(["2026-02-28", "2026-03-01", "2026-03-02"]);
  });

  it("ignores recaps that are not ready or stale", () => {
    const day = makeJournalDay({ date: "2026-09-22", activities: [] });
    const recaps = new Map([["2026-09-22", makeRecap({ status: "failed", done: ["ghost"] })]]);
    expect(contributionCandidates([day], recaps, [])).toEqual([]);
  });
});
