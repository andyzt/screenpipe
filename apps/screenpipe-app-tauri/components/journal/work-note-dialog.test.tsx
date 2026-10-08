// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { makeActivityCard } from "@/lib/journal/fixtures";
import { mockLocalApiResponse } from "@/lib/dev/browser-engine-mock";

const api = vi.hoisted(() => ({ fetch: vi.fn() }));
vi.mock("@/lib/api", () => ({ localFetch: api.fetch }));
import { WorkNoteDialog } from "./work-note-dialog";

beforeEach(() => {
  api.fetch.mockReset();
  api.fetch.mockImplementation((path: string, init?: RequestInit) => Promise.resolve(
    mockLocalApiResponse(new URL(`http://localhost:3030${path}`), init, "ready"),
  ));
});

describe("work note button", () => {
  it("opens, saves a linked note, and reads it back when reopened", async () => {
    const date = new Date();
    date.setHours(10, 0, 0, 0);
    const card = makeActivityCard({ id: 4101, start_at: date.toISOString() });
    render(<WorkNoteDialog card={card} />);
    fireEvent.click(screen.getByRole("button", { name: "Add work note" }));
    await screen.findByRole("dialog");
    await waitFor(() => expect(screen.queryByText("Loading notes…")).toBeNull());
    fireEvent.change(screen.getByLabelText("Work note"), { target: { value: "Fixed the missing note editor" } });
    fireEvent.click(screen.getByRole("button", { name: "Save note" }));
    await screen.findByText("Note saved.");
    const call = api.fetch.mock.calls.find(([, init]) => init?.method === "PUT");
    expect(JSON.parse(call![1].body)).toMatchObject({
      source_activity_id: 4101, outcome: "Fixed the missing note editor", approve: false, expected_revision: null,
    });
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    fireEvent.click(screen.getByRole("button", { name: "Add work note" }));
    await screen.findByText("Fixed the missing note editor");
    fireEvent.click(screen.getByRole("button", { name: "Edit note" }));
    expect(screen.getByLabelText("Work note")).toHaveValue("Fixed the missing note editor");
    fireEvent.change(screen.getByLabelText("Work note"), { target: { value: "Updated contribution" } });
    fireEvent.click(screen.getByRole("button", { name: "Save note" }));
    await screen.findByText("Updated contribution");
    const writes = api.fetch.mock.calls.filter(([, init]) => init?.method === "PUT");
    expect(JSON.parse(writes[1][1].body)).toMatchObject({ expected_revision: 1, approve: false });
  });

  it("keeps the draft after a failed save, so the user can retry", async () => {
    render(<WorkNoteDialog card={makeActivityCard()} />);
    fireEvent.click(screen.getByRole("button", { name: "Add work note" }));
    await waitFor(() => expect(screen.queryByText("Loading notes…")).toBeNull());
    fireEvent.change(screen.getByLabelText("Work note"), { target: { value: "Unsaved result" } });
    api.fetch.mockResolvedValueOnce(Response.json({ error: "failure" }, { status: 500 }));
    fireEvent.click(screen.getByRole("button", { name: "Save note" }));
    await screen.findByRole("alert");
    expect(screen.getByLabelText("Work note")).toHaveValue("Unsaved result");
    expect(screen.getByRole("button", { name: "Save note" })).toBeEnabled();
    expect(screen.queryByText("Note saved.")).toBeNull();
  });
});
