// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { makeActivityCard, makeCategory, makeJournalDay, makeRecap } from "@/lib/journal/fixtures";

const api = vi.hoisted(() => ({
  fetchJournalDay: vi.fn(),
  fetchActivityDetail: vi.fn(),
  fetchRecap: vi.fn(),
  fetchActiveIntention: vi.fn(),
  createIntention: vi.fn(),
  push: vi.fn(),
  emit: vi.fn(),
  setPendingNavigation: vi.fn(),
}));
vi.mock("@/lib/journal/api", () => ({
  fetchJournalDay: api.fetchJournalDay,
  fetchActivityDetail: api.fetchActivityDetail,
  fetchRecap: api.fetchRecap,
  fetchActiveIntention: api.fetchActiveIntention,
  createIntention: api.createIntention,
}));
vi.mock("next/navigation", () => ({ useRouter: () => ({ push: api.push }) }));
vi.mock("@tauri-apps/api/event", () => ({ emit: api.emit }));
vi.mock("@/lib/hooks/use-timeline-store", () => ({
  useTimelineStore: (selector: (state: unknown) => unknown) =>
    selector({ setPendingNavigation: api.setPendingNavigation }),
}));
vi.mock("@/lib/analytics/traction", () => ({ trackTraction: vi.fn() }));

import { ResumePanel } from "./resume-panel";

const detail = {
  ...makeActivityCard({ id: 3 }),
  interval_keys: [],
  evidence: [
    { source_type: "frame", source_id: 1, occurred_at: "2026-09-22T08:10:00Z", frame_id: 11, app_name: "Cursor", window_title: "auth.rs", browser_url: null },
    { source_type: "frame", source_id: 2, occurred_at: "2026-09-22T08:20:00Z", frame_id: 12, app_name: "Cursor", window_title: "auth.rs", browser_url: null },
    { source_type: "frame", source_id: 3, occurred_at: "2026-09-22T08:25:00Z", frame_id: 13, app_name: "Google Chrome", window_title: "PR #12", browser_url: "https://github.com/x/pull/12" },
  ],
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers({ shouldAdvanceTime: true });
  api.fetchJournalDay.mockImplementation(async (date: string) =>
    makeJournalDay({
      date,
      // Only the newest day has a recap; the panel must not read one elsewhere.
      recap: date === "2026-09-22" ? { status: "ready", generated_at: "2026-09-22T18:00:00Z" } : { status: "none", generated_at: null },
      activities:
        date === "2026-09-22"
          ? [
              makeActivityCard({ id: 3, title: "Auth fix", summary: "Stopped at the session store.", start_at: "2026-09-22T08:00:00Z", end_at: "2026-09-22T08:30:00Z" }),
              makeActivityCard({ id: 4, title: "Lunch", intention_relation: "break", end_at: "2026-09-22T12:00:00Z" }),
              makeActivityCard({ id: 5, title: "Could not summarize this period", active_minutes: 0, category: makeCategory({ id: "system", is_system: true }), end_at: "2026-09-22T13:00:00Z" }),
            ]
          : [makeActivityCard({ id: 1, title: "Billing review", end_at: `${date}T10:00:00Z` })],
    }),
  );
  api.fetchActivityDetail.mockResolvedValue(detail);
  api.fetchRecap.mockResolvedValue(makeRecap({ next: ["Re-run the flaky test"] }));
  api.fetchActiveIntention.mockResolvedValue(null);
  api.createIntention.mockResolvedValue({ id: 1, title: "Auth fix" });
  vi.setSystemTime(new Date(2026, 8, 22, 15, 0, 0));
});
afterEach(() => vi.useRealTimers());

async function openPanel(onOpenDay?: (d: string, cardId: number) => void) {
  render(<ResumePanel onOpenDay={onOpenDay} />);
  fireEvent.click(screen.getByTestId("journal-resume-open"));
  await screen.findByTestId("journal-resume-detail");
}

describe("resume panel", () => {
  it("lists resumable tasks from the last three days, newest first, without breaks", async () => {
    await openPanel();
    expect(api.fetchJournalDay.mock.calls.map((c) => c[0])).toEqual(["2026-09-20", "2026-09-21", "2026-09-22"]);
    const candidates = screen.getAllByTestId("journal-resume-candidate");
    expect(candidates).toHaveLength(2);
    expect(candidates[0]).toHaveTextContent("Auth fix");
    expect(candidates[1]).toHaveTextContent("Billing review");
    expect(screen.queryByText("Lunch")).not.toBeInTheDocument();
    expect(screen.queryByText(/Could not summarize/)).not.toBeInTheDocument();
    expect(screen.getByText("Stopped at the session store.")).toBeInTheDocument();
    expect(screen.getByTestId("journal-resume-model-caveat")).toHaveTextContent(/model-written/);
    const sources = await screen.findAllByTestId("journal-resume-source");
    expect(sources).toHaveLength(2);
    expect(sources[0]).toHaveTextContent("auth.rs");
    expect(screen.getByTestId("journal-resume-next")).toHaveTextContent("Re-run the flaky test");
    expect(screen.getByText(/written by the model/i)).toBeInTheDocument();
  });

  it("opens the timeline at the latest frame of a material", async () => {
    await openPanel();
    const buttons = await screen.findAllByTestId("journal-resume-open-timeline");
    fireEvent.click(buttons[0]);
    expect(api.setPendingNavigation).toHaveBeenCalledWith({ timestamp: "2026-09-22T08:20:00Z", frameId: "12" });
    expect(api.push).toHaveBeenCalledWith("/home?section=timeline");
    await vi.advanceTimersByTimeAsync(300);
    expect(api.emit).toHaveBeenCalledWith("navigate-to-frame", "12");
  });

  it("reads the recap only for a day whose stub has one", async () => {
    await openPanel();
    await screen.findByTestId("journal-resume-next");
    expect(api.fetchRecap).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getAllByTestId("journal-resume-candidate")[1]);
    await screen.findAllByTestId("journal-resume-source");
    expect(api.fetchRecap).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("journal-resume-next")).not.toBeInTheDocument();
  });

  it("continues a task with an editable title, tells the host, and hands off to the work note editor", async () => {
    const listener = vi.fn();
    const onIntentionCreated = vi.fn();
    window.addEventListener("journal-add-work-note", listener);
    render(<ResumePanel onIntentionCreated={onIntentionCreated} />);
    fireEvent.click(screen.getByTestId("journal-resume-open"));
    await screen.findByTestId("journal-resume-detail");
    expect(screen.queryByTestId("journal-resume-replaces")).not.toBeInTheDocument();
    fireEvent.change(screen.getByTestId("journal-resume-title"), { target: { value: "Auth fix — rollback path" } });
    fireEvent.change(screen.getByTestId("journal-resume-project"), { target: { value: "Alpha" } });
    fireEvent.click(screen.getByTestId("journal-resume-continue"));
    await waitFor(() => expect(api.createIntention).toHaveBeenCalledWith({ title: "Auth fix — rollback path", project: "Alpha" }));
    await screen.findByText(/Intention set: Auth fix — rollback path/);
    expect(onIntentionCreated).toHaveBeenCalledWith({ id: 1, title: "Auth fix" });
    fireEvent.click(screen.getByTestId("journal-resume-add-note"));
    expect(listener).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(250);
    expect((listener.mock.calls[0][0] as CustomEvent).detail).toEqual({ id: 3, start_at: "2026-09-22T08:00:00Z" });
    window.removeEventListener("journal-add-work-note", listener);
  });

  it("says which intention continuing would end and refuses an empty title", async () => {
    api.fetchActiveIntention.mockResolvedValue({ id: 5, title: "Ship the release", project: null });
    await openPanel();
    await screen.findByTestId("journal-resume-replaces");
    expect(screen.getByTestId("journal-resume-replaces")).toHaveTextContent("Ship the release");
    fireEvent.change(screen.getByTestId("journal-resume-title"), { target: { value: "  " } });
    expect(screen.getByTestId("journal-resume-continue")).toBeDisabled();
    fireEvent.change(screen.getByTestId("journal-resume-title"), { target: { value: "Ship the release" } });
    fireEvent.change(screen.getByTestId("journal-resume-project"), { target: { value: "" } });
    expect(screen.queryByTestId("journal-resume-replaces")).not.toBeInTheDocument();
    // Continuing the intention that is already running would end and recreate it.
    expect(screen.getByTestId("journal-resume-already-active")).toBeInTheDocument();
    expect(screen.getByTestId("journal-resume-continue")).toBeDisabled();
  });

  it("opens the day of the task with its last card selected", async () => {
    const onOpenDay = vi.fn();
    await openPanel(onOpenDay);
    fireEvent.click(screen.getAllByTestId("journal-resume-candidate")[1]);
    fireEvent.click(screen.getByTestId("journal-resume-open-day"));
    expect(onOpenDay).toHaveBeenCalledWith("2026-09-21", 1);
  });

  it("explains an empty window and a failed read", async () => {
    api.fetchJournalDay.mockResolvedValue(makeJournalDay({ activities: [] }));
    render(<ResumePanel />);
    fireEvent.click(screen.getByTestId("journal-resume-open"));
    await screen.findByTestId("journal-resume-empty");
    api.fetchJournalDay.mockRejectedValue(new Error("engine down"));
    fireEvent.keyDown(document.body, { key: "Escape" });
    fireEvent.click(screen.getByTestId("journal-resume-open"));
    await screen.findByText(/engine down/);
  });
});
