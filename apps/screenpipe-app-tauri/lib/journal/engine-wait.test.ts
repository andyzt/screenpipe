// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

import { describe, expect, it, vi } from "vitest";
import { JournalApiError } from "./api";
import { isEngineUnavailable, withEngineWait } from "./engine-wait";

describe("isEngineUnavailable", () => {
  it("treats transport failures and proxy 502/503 as the engine still starting", () => {
    expect(isEngineUnavailable(new TypeError("Load failed"))).toBe(true);
    expect(isEngineUnavailable(new Error("Failed to fetch"))).toBe(true);
    expect(isEngineUnavailable(new JournalApiError("bad gateway", 502))).toBe(true);
    expect(isEngineUnavailable(new JournalApiError("unavailable", 503))).toBe(true);
  });

  it("does not hide real answers or an abort", () => {
    expect(isEngineUnavailable(new JournalApiError("not found", 404))).toBe(false);
    expect(isEngineUnavailable(new JournalApiError("bad request", 400))).toBe(false);
    expect(isEngineUnavailable(new DOMException("aborted", "AbortError"))).toBe(false);
  });
});

describe("withEngineWait", () => {
  it("retries while the engine is unreachable and returns the first answer", async () => {
    const request = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(new TypeError("Load failed"))
      .mockRejectedValueOnce(new TypeError("Load failed"))
      .mockResolvedValue("day");
    const waiting = vi.fn();
    const result = await withEngineWait(request, new AbortController().signal, waiting, 5, 1);
    expect(result).toBe("day");
    expect(request).toHaveBeenCalledTimes(3);
    expect(waiting).toHaveBeenCalledTimes(2);
  });

  it("gives up after the attempt budget", async () => {
    const request = vi.fn<() => Promise<string>>().mockRejectedValue(new TypeError("Load failed"));
    await expect(withEngineWait(request, new AbortController().signal, undefined, 3, 1)).rejects.toThrow(
      "Load failed",
    );
    expect(request).toHaveBeenCalledTimes(3);
  });

  it("does not retry a real HTTP answer", async () => {
    const request = vi.fn<() => Promise<string>>().mockRejectedValue(new JournalApiError("not found", 404));
    await expect(withEngineWait(request, new AbortController().signal, undefined, 3, 1)).rejects.toThrow(
      "not found",
    );
    expect(request).toHaveBeenCalledTimes(1);
  });

  it("stops when the caller aborts during the wait", async () => {
    const controller = new AbortController();
    const request = vi.fn<() => Promise<string>>().mockRejectedValue(new TypeError("Load failed"));
    const pending = withEngineWait(request, controller.signal, undefined, 10, 10_000);
    controller.abort();
    await expect(pending).rejects.toThrow("Load failed");
    expect(request).toHaveBeenCalledTimes(1);
  });
});
