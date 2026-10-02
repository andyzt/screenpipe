// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

/**
 * Accessibility gate for jsdom component tests.
 *
 * Runs axe-core over a rendered container and returns the violations a
 * reviewer would block on (impact serious or critical). Rules that need a
 * layout engine or a whole document are switched off: jsdom cannot compute
 * colour contrast (that is covered by the token values in globals.css and
 * journal-theme.css), and a component fragment has no page landmarks.
 */
import axe from "axe-core";

const FRAGMENT_RULES: Record<string, { enabled: boolean }> = {
  "color-contrast": { enabled: false },
  region: { enabled: false },
  "landmark-one-main": { enabled: false },
  "page-has-heading-one": { enabled: false },
  bypass: { enabled: false },
};

export async function seriousAxeViolations(
  container: Element,
): Promise<{ id: string; impact: string | null | undefined; nodes: string[] }[]> {
  const results = await axe.run(container, { rules: FRAGMENT_RULES });
  return results.violations
    .filter((v) => v.impact === "serious" || v.impact === "critical")
    .map((v) => ({
      id: v.id,
      impact: v.impact,
      nodes: v.nodes.slice(0, 3).map((n) => n.target.join(" ")),
    }));
}
