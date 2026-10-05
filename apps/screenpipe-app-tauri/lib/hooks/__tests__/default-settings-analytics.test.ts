// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

import { describe, expect, it } from "vitest";
import { createDefaultSettingsObject } from "@/lib/hooks/use-settings";

// docs/UPSTREAM_SYNC.md (Telemetry): the PostHog key and host in this tree are
// upstream's, so a new install of the fork must not report anywhere until the
// user opts in. Existing installs keep whatever value they already stored.
describe("default settings: analytics", () => {
  it("defaults analyticsEnabled to false for new installs", () => {
    const settings = createDefaultSettingsObject();
    expect(settings.analyticsEnabled).toBe(false);
  });
});
