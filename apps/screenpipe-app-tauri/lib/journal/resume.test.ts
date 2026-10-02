// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
import { describe, expect, it } from "vitest";
import {
  collectResumeCandidates,
  dedupeSources,
  isResumableCard,
  resumeDates,
  suggestedNextSteps,
} from "./resume";
import {
  makeActivityCard,
  makeCategory,
  makeIntention,
  makeJournalDay,
  makeRecap,
} from "./fixtures";
import type { ActivityEvidence } from "./types";

describe("resumeDates", () => {
  it("returns the last three journal dates ending today, oldest first", () => {
    // 02:00 local is still the previous journal day (04:00 boundary).
    const now = new Date(2026, 8, 23, 2, 0, 0);
    expect(resumeDates(now)).toEqual(["2026-09-20", "2026-09-21", "2026-09-22"]);
    expect(resumeDates(new Date(2026, 8, 23, 9, 0, 0))).toEqual([
      "2026-09-21",
      "2026-09-22",
      "2026-09-23",
    ]);
  });
});

describe("isResumableCard", () => {
  it("keeps work and other work, drops idle, system, distraction, breaks and detours", () => {
    expect(isResumableCard(makeActivityCard())).toBe(true);
    expect(isResumableCard(makeActivityCard({ intention_relation: "other_work" }))).toBe(true);
    expect(isResumableCard(makeActivityCard({ category: makeCategory({ id: "personal" }) }))).toBe(true);
    expect(isResumableCard(makeActivityCard({ category: makeCategory({ id: "idle", is_idle: true }) }))).toBe(false);
    expect(isResumableCard(makeActivityCard({ category: makeCategory({ id: "system", is_system: true }), title: "Could not summarize this period", active_minutes: 0 }))).toBe(false);
    expect(isResumableCard(makeActivityCard({ category: makeCategory({ id: "distraction" }) }))).toBe(false);
    expect(isResumableCard(makeActivityCard({ intention_relation: "break" }))).toBe(false);
    expect(isResumableCard(makeActivityCard({ intention_relation: "possible_distraction" }))).toBe(false);
    expect(isResumableCard(makeActivityCard({ title: "   " }))).toBe(false);
  });
});

describe("collectResumeCandidates", () => {
  it("groups cards by title across days and orders by last seen", () => {
    const monday = makeJournalDay({
      date: "2026-09-21",
      activities: [
        makeActivityCard({ id: 1, title: "Auth fix", start_at: "2026-09-21T08:00:00Z", end_at: "2026-09-21T09:00:00Z", active_minutes: 50 }),
        makeActivityCard({ id: 2, title: "Billing review", start_at: "2026-09-21T10:00:00Z", end_at: "2026-09-21T11:00:00Z", active_minutes: 40 }),
      ],
    });
    const tuesday = makeJournalDay({
      date: "2026-09-22",
      activities: [
        makeActivityCard({ id: 3, title: "auth  fix!", summary: "Latest", start_at: "2026-09-22T08:00:00Z", end_at: "2026-09-22T08:30:00Z", active_minutes: 25 }),
        makeActivityCard({ id: 4, title: "Lunch", intention_relation: "break", start_at: "2026-09-22T12:00:00Z", end_at: "2026-09-22T13:00:00Z" }),
      ],
    });
    const candidates = collectResumeCandidates([monday, tuesday]);
    expect(candidates.map((c) => c.title)).toEqual(["auth  fix!", "Billing review"]);
    const auth = candidates[0];
    expect(auth.cardIds).toEqual([1, 3]);
    expect(auth.dates).toEqual(["2026-09-21", "2026-09-22"]);
    expect(auth.activeMinutes).toBe(75);
    expect(auth.lastCard.summary).toBe("Latest");
    expect(auth.lastDate).toBe("2026-09-22");
  });

  it("takes the project from the card's own intention, not the day's first", () => {
    const day = makeJournalDay({
      date: "2026-09-22",
      intentions: [makeIntention({ id: 7, project: "Alpha" }), makeIntention({ id: 8, project: "Beta" })],
      activities: [makeActivityCard({ id: 1, title: "Write spec", intention: { id: 8, title: "Later" } })],
    });
    expect(collectResumeCandidates([day])[0].project).toBe("Beta");
    const unknownIntention = makeJournalDay({
      date: "2026-09-22",
      intentions: [makeIntention({ id: 7, project: "Alpha" })],
      activities: [makeActivityCard({ id: 1, title: "Write spec", intention: { id: 99, title: "Gone" } })],
    });
    expect(collectResumeCandidates([unknownIntention])[0].project).toBeNull();
  });

  it("splits the same title under different intention projects and caps the list", () => {
    const withProject = makeJournalDay({
      date: "2026-09-22",
      intentions: [makeIntention({ id: 7, project: "Alpha" })],
      activities: [
        makeActivityCard({ id: 1, title: "Write spec", intention: { id: 7, title: "Ship" }, end_at: "2026-09-22T09:00:00Z" }),
        makeActivityCard({ id: 2, title: "Write spec", intention: null, end_at: "2026-09-22T10:00:00Z" }),
        ...Array.from({ length: 10 }, (_, i) =>
          makeActivityCard({ id: 100 + i, title: `Task ${i}`, end_at: `2026-09-22T1${i}:00:00Z`.replace("T110", "T11") }),
        ),
      ],
    });
    const candidates = collectResumeCandidates([withProject], 5);
    expect(candidates).toHaveLength(5);
    const all = collectResumeCandidates([withProject], 50);
    const specs = all.filter((c) => c.title === "Write spec");
    expect(specs).toHaveLength(2);
    expect(specs.map((c) => c.project).sort()).toEqual([null, "Alpha"].sort());
  });
});

describe("dedupeSources", () => {
  const row = (over: Partial<ActivityEvidence>): ActivityEvidence => ({
    source_type: "frame",
    source_id: 1,
    occurred_at: "2026-09-22T08:00:00Z",
    frame_id: 1,
    app_name: "Cursor",
    window_title: "auth.rs",
    browser_url: null,
    ...over,
  });

  it("collapses repeated materials, keeps the latest frame and drops audio", () => {
    const sources = dedupeSources([
      row({ source_id: 1, frame_id: 1, occurred_at: "2026-09-22T08:00:00Z" }),
      row({ source_id: 2, frame_id: 2, occurred_at: "2026-09-22T08:30:00Z" }),
      row({ source_id: 3, frame_id: 3, app_name: "Google Chrome", window_title: "PR #12", browser_url: "https://github.com/x/pull/12" }),
      row({ source_id: 4, frame_id: null, source_type: "audio", app_name: null, window_title: null }),
      row({ source_id: 5, frame_id: 5, app_name: null, window_title: null, browser_url: null }),
      row({ source_id: 6, frame_id: 6, app_name: null, window_title: "Untitled window" }),
    ]);
    expect(sources).toHaveLength(3);
    expect(sources[2]).toMatchObject({ app: "", windowTitle: "Untitled window" });
    expect(sources[0]).toMatchObject({ app: "Cursor", windowTitle: "auth.rs", frameId: 2, hits: 2 });
    expect(sources[1]).toMatchObject({ app: "Google Chrome", browserUrl: "https://github.com/x/pull/12", hits: 1 });
  });

  it("caps the list", () => {
    const many = Array.from({ length: 10 }, (_, i) => row({ source_id: i, frame_id: i, window_title: `file ${i}` }));
    expect(dedupeSources(many, 3)).toHaveLength(3);
  });
});

describe("suggestedNextSteps", () => {
  it("uses only a ready or stale recap and drops blank lines", () => {
    expect(suggestedNextSteps(null)).toEqual([]);
    expect(suggestedNextSteps(makeRecap({ status: "none", next: ["x"] }))).toEqual([]);
    expect(suggestedNextSteps(makeRecap({ status: "failed", next: ["x"] }))).toEqual([]);
    expect(suggestedNextSteps(makeRecap({ status: "stale", next: [" a ", "", "b"] }))).toEqual([" a ", "b"]);
    expect(suggestedNextSteps(makeRecap({ next: ["1", "2", "3", "4", "5"] }))).toHaveLength(4);
  });
});
