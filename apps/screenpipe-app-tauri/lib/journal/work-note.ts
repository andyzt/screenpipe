// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

import { localFetch } from "@/lib/api";

export interface WorkNote {
  id: string;
  revision: number;
  date: string;
  context: string;
  project: string;
  outcome: string;
  status: string;
  role: string;
  approved: boolean;
  source_activity_id: number | null;
  source_activity_key: string | null;
  source_start_at: string | null;
  source_end_at: string | null;
  updated_at: string;
}

export interface WorkNoteInput {
  id: string;
  expected_revision: number | null;
  date: string;
  context: string;
  project: string;
  outcome: string;
  status: string;
  role: string;
  source_activity_id: number | null;
  approve: false;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await localFetch(path, init);
  if (!response.ok) throw new Error(String(response.status));
  return response.json();
}

export function fetchWorkNotes(date: string, signal?: AbortSignal) {
  const query = new URLSearchParams({ from: date, to: date });
  return request<{ entries: WorkNote[]; truncated: boolean }>(
    `/journal/work-log?${query}`, { signal, cache: "no-store" },
  );
}

export function saveWorkNote(note: WorkNoteInput) {
  const { id, ...input } = note;
  return request<{ entry: WorkNote }>(`/journal/work-log/${encodeURIComponent(id)}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
}
