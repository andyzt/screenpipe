// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

/**
 * Guards the journal-loop surfaces against new English literals in JSX.
 *
 * Every string a user can read in `components/journal`, `components/first-run`
 * and `components/onboarding` must come from `t()` / `translate()` so the
 * Russian build never shows half a screen in English. The check is an AST
 * walk with TypeScript's own parser rather than a regex, so it sees exactly
 * what React renders: JSX text children, string literals used as children
 * (directly, through `?:`, `&&`, `||`, `??`, and the literal parts of template
 * strings), and the three attributes that reach the screen or the screen
 * reader: `placeholder`, `aria-label`, `title`.
 *
 * A literal is a finding when it still contains a Latin letter after every
 * allowlisted token is removed. Anything without Latin letters — `{" · "}`,
 * `&nbsp;`, `—`, numbers, a lone `?` — is never a finding, and neither is a
 * `t("key")` call, because its argument is a call argument, not a child.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";

import ts from "typescript";
import { describe, expect, it } from "vitest";

const APP_ROOT = path.resolve(__dirname, "..", "..");

/** The directories under lint. Add a surface here once it is fully localized. */
const SURFACES = [
  "components/journal",
  "components/first-run",
  "components/onboarding",
] as const;

/**
 * Files under the linted surfaces that this build never shows. They are
 * upstream flows the fork hides or no longer mounts; the plan
 * (stats/QUALITY_DESIGN_RESEARCH.md, "do not do") keeps hidden upstream
 * surfaces untouched rather than translating them. Each entry says why. An
 * `unimported` file is re-checked below: the moment any non-test file imports
 * it again, the test fails and the exclusion has to be revisited.
 */
const NOT_SHOWN_IN_THIS_BUILD: Record<string, { reason: string; unimported?: true }> = {
  "components/onboarding/login-gate.tsx": {
    reason: "login slide removed: this build has no screenpipe account (app/onboarding/page.tsx)",
    unimported: true,
  },
  "components/first-run/next-steps.tsx": {
    reason: "upstream first-run next steps, no longer mounted",
    unimported: true,
  },
  "components/first-run/search-shortcut-practice.tsx": {
    reason: "upstream search-shortcut lesson, no longer mounted",
    unimported: true,
  },
  "components/onboarding/final-setup-step.tsx": {
    reason: "slide recommended-setup is in HIDDEN_ONBOARDING_SLIDES",
  },
  "components/onboarding/plan-selection-step.tsx": {
    reason: "upstream billing slide, shown only when shouldShowPlanSelection",
  },
  "components/first-run/trial-activation-paywall.tsx": {
    reason: "upstream Business-trial checkout behind the trial-activation experiment",
  },
  "components/onboarding/focused-spotlight.tsx": {
    reason: "used only by the Live View settings guide, a hidden upstream surface",
  },
};

/** JSX attributes whose string value reaches the user or the screen reader. */
const CHECKED_ATTRIBUTES = new Set(["placeholder", "aria-label", "title"]);

/**
 * Tokens that are the same in every locale: product and vendor names,
 * protocol and format names, units. A literal made only of these (plus
 * punctuation, digits and single characters) is not English copy.
 */
const ALLOWLIST = new Set([
  // Product and vendor names.
  "screenpipe",
  "Screenpipe",
  "Ollama",
  "DeepSeek",
  "VseLLM",
  "GitHub",
  "Telegram",
  "Slack",
  "Claude",
  "Cursor",
  "Codex",
  "Notion",
  "Obsidian",
  "macOS",
  "Windows",
  "Linux",
  // Protocols, formats, acronyms.
  "MCP",
  "Markdown",
  "HTML",
  "URL",
  "API",
  "OK",
  "AI",
  "OCR",
  "CPU",
  "GPU",
  "RAM",
  "AVX2",
  // Units.
  "GiB",
  "GB",
  "MB",
  "ms",
  "px",
  "kbps",
]);

interface Finding {
  file: string;
  line: number;
  literal: string;
}

function walkDir(dir: string, out: string[]): void {
  for (const entry of readdirSync(dir)) {
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) {
      walkDir(full, out);
      continue;
    }
    if (!entry.endsWith(".tsx")) continue;
    if (entry.endsWith(".test.tsx")) continue;
    if (/\.stories\./.test(entry)) continue;
    out.push(full);
  }
}

/** HTML entities are opaque to the parser; decode them so `&nbsp;` has no letters. */
function decodeEntities(text: string): string {
  return text
    .replace(/&nbsp;/g, " ")
    .replace(/&apos;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&#x[0-9a-fA-F]+;/g, " ")
    .replace(/&#\d+;/g, " ")
    .replace(/&[a-zA-Z]+;/g, " ");
}

/** True when Latin letters survive after allowlisted tokens are dropped. */
export function isEnglishLiteral(raw: string): boolean {
  const text = decodeEntities(raw).trim();
  if (!/[A-Za-z]/.test(text)) return false;
  const tokens = text.split(/[\s·,()/]+/).filter(Boolean);
  return tokens.some((token) => {
    const bare = token.replace(/^[^A-Za-z0-9]+|[^A-Za-z0-9]+$/g, "");
    if (bare.length <= 1) return false;
    if (ALLOWLIST.has(bare)) return false;
    return /[A-Za-z]/.test(bare);
  });
}

/**
 * The string literals an expression can render as-is. Calls (`t()`,
 * `formatX()`), identifiers and member reads are opaque: their value is
 * decided elsewhere, and `t("key")` is exactly what we want to see.
 */
function literalsIn(expr: ts.Expression | undefined, out: ts.Node[]): void {
  if (!expr) return;
  if (ts.isStringLiteral(expr) || ts.isNoSubstitutionTemplateLiteral(expr)) {
    out.push(expr);
    return;
  }
  if (ts.isTemplateExpression(expr)) {
    if (isEnglishLiteral(expr.head.text)) out.push(expr.head);
    for (const span of expr.templateSpans) {
      if (isEnglishLiteral(span.literal.text)) out.push(span.literal);
    }
    return;
  }
  if (ts.isParenthesizedExpression(expr)) {
    literalsIn(expr.expression, out);
    return;
  }
  if (ts.isConditionalExpression(expr)) {
    literalsIn(expr.whenTrue, out);
    literalsIn(expr.whenFalse, out);
    return;
  }
  if (ts.isBinaryExpression(expr)) {
    const op = expr.operatorToken.kind;
    if (
      op === ts.SyntaxKind.AmpersandAmpersandToken ||
      op === ts.SyntaxKind.BarBarToken ||
      op === ts.SyntaxKind.QuestionQuestionToken
    ) {
      literalsIn(expr.left, out);
      literalsIn(expr.right, out);
    }
  }
}

function literalText(node: ts.Node): string {
  if (ts.isJsxText(node)) return node.text;
  if (
    ts.isStringLiteral(node) ||
    ts.isNoSubstitutionTemplateLiteral(node) ||
    ts.isTemplateHead(node) ||
    ts.isTemplateMiddle(node) ||
    ts.isTemplateTail(node)
  ) {
    return node.text;
  }
  return node.getText();
}

export function lintSource(source: ts.SourceFile, file: string): Finding[] {
  const findings: Finding[] = [];
  const report = (node: ts.Node) => {
    const text = literalText(node);
    if (!isEnglishLiteral(text)) return;
    const { line } = source.getLineAndCharacterOfPosition(node.getStart(source));
    findings.push({ file, line: line + 1, literal: text.trim() });
  };

  const visit = (node: ts.Node): void => {
    if (ts.isJsxText(node)) {
      if (!node.containsOnlyTriviaWhiteSpaces) report(node);
    } else if (
      ts.isJsxExpression(node) &&
      node.parent &&
      (ts.isJsxElement(node.parent) || ts.isJsxFragment(node.parent))
    ) {
      const literals: ts.Node[] = [];
      literalsIn(node.expression, literals);
      literals.forEach(report);
    } else if (ts.isJsxAttribute(node)) {
      const name = node.name.getText(source);
      if (CHECKED_ATTRIBUTES.has(name) && node.initializer) {
        if (ts.isStringLiteral(node.initializer)) {
          report(node.initializer);
        } else if (ts.isJsxExpression(node.initializer)) {
          const literals: ts.Node[] = [];
          literalsIn(node.initializer.expression, literals);
          literals.forEach(report);
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return findings;
}

function parse(file: string, text: string): ts.SourceFile {
  return ts.createSourceFile(
    file,
    text,
    ts.ScriptTarget.Latest,
    /* setParentNodes */ true,
    ts.ScriptKind.TSX,
  );
}

export function lintSurfaces(): Finding[] {
  const files: string[] = [];
  for (const surface of SURFACES) walkDir(path.join(APP_ROOT, surface), files);
  files.sort();
  const findings: Finding[] = [];
  for (const file of files) {
    const rel = path.relative(APP_ROOT, file);
    if (NOT_SHOWN_IN_THIS_BUILD[rel]) continue;
    findings.push(...lintSource(parse(rel, readFileSync(file, "utf8")), rel));
  }
  return findings;
}

function describeFindings(findings: Finding[]): string {
  const lines = findings.map(
    (f) => `  ${f.file}:${f.line}  ${JSON.stringify(f.literal)}`,
  );
  return [
    `${findings.length} bare English literal(s) in JSX. Route each through t() (lib/i18n/en.json + ru.json)`,
    "or, for a proper noun / unit, add the token to ALLOWLIST in lib/i18n/literal-lint.test.ts:",
    ...lines,
  ].join("\n");
}

describe("literal lint", () => {
  it("flags JSX text, literal children and user-facing attributes", () => {
    const fixture = `
      export function Fixture({ t, on }: any) {
        return (
          <section aria-label="Saved notes" title={on ? "Recording" : "Paused"}>
            <h1>Reviewed work</h1>
            <p>{on ? "Approved" : t("x.notApproved")}</p>
            <input placeholder="Reviewed the migration" />
            <span>{\`≈ \${on} min\`}</span>
            <span>{on && "noted"}</span>
          </section>
        );
      }
    `;
    const found = lintSource(parse("fixture.tsx", fixture), "fixture.tsx").map(
      (f) => f.literal,
    );
    expect(found).toEqual([
      "Saved notes",
      "Recording",
      "Paused",
      "Reviewed work",
      "Approved",
      "Reviewed the migration",
      "min",
      "noted",
    ]);
  });

  it("ignores translated strings, separators, entities, units and proper nouns", () => {
    const fixture = `
      export function Fixture({ t, count }: any) {
        return (
          <section aria-label={t("a.label")} title={t("a.title")}>
            <h1>{t("a.heading")}</h1>
            <span>{" · "}</span>
            <span>&nbsp;—&nbsp;</span>
            <span>{count} GiB</span>
            <span>Ollama · DeepSeek</span>
            <span>screenpipe</span>
            <span>?</span>
            <span>{count > 1 ? " · " : ""}</span>
            <span>{formatDuration(count)}</span>
            <span>{\`\${count}%\`}</span>
            <img alt="decorative" data-testid="only-for-tests" className="x y" />
          </section>
        );
      }
    `;
    expect(lintSource(parse("fixture.tsx", fixture), "fixture.tsx")).toEqual([]);
  });

  it("keeps every not-shown exclusion honest", () => {
    const sources: string[] = [];
    for (const dir of ["app", "components", "lib"]) {
      const files: string[] = [];
      const walk = (d: string) => {
        for (const entry of readdirSync(d)) {
          const full = path.join(d, entry);
          if (statSync(full).isDirectory()) {
            if (entry !== "node_modules") walk(full);
          } else if (/\.(ts|tsx)$/.test(entry) && !/\.test\.(ts|tsx)$/.test(entry)) {
            files.push(full);
          }
        }
      };
      walk(path.join(APP_ROOT, dir));
      sources.push(...files);
    }
    for (const [rel, entry] of Object.entries(NOT_SHOWN_IN_THIS_BUILD)) {
      expect(statSync(path.join(APP_ROOT, rel)).isFile(), `${rel} no longer exists; drop it from NOT_SHOWN_IN_THIS_BUILD`).toBe(true);
      if (!entry.unimported) continue;
      const module = rel.replace(/\.tsx$/, "").split("/").pop()!;
      const importers = sources.filter((file) => {
        if (path.relative(APP_ROOT, file) === rel) return false;
        const text = readFileSync(file, "utf8");
        return new RegExp(`from ["'][^"']*/${module}["']`).test(text);
      });
      expect(
        importers.map((f) => path.relative(APP_ROOT, f)),
        `${rel} is imported again, so it may be shown: localize it and remove it from NOT_SHOWN_IN_THIS_BUILD`,
      ).toEqual([]);
    }
  });

  it("finds no bare English literal on the journal loop surfaces", () => {
    const findings = lintSurfaces();
    expect(findings, describeFindings(findings)).toEqual([]);
  });
});
