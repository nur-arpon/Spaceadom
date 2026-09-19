/**
 * touchpad-tokens.test.ts — TOUCHPAD T2 (1.0.120, TOUCHPAD-BRIEF-T2 §Tests):
 * the two look token sheets must define the SAME set of `--sp-*` names.
 *
 *   node scripts/touchpad-tokens.test.ts
 *
 * `data-look="chocolate"` (the design's dark surfaces) and `data-look="app"`
 * (mapped onto the app's --st-* variables) are the two sheets. If one defines
 * a token the other forgets, the "Matches the app" look would inherit a stale
 * chocolate value (or vice-versa) — a silent visual bug. Same no-framework,
 * regex-as-parser arrangement as setting-subs.test.ts.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const css = readFileSync(join(here, "..", "src", "styles", "touchpad.css"), "utf8");

/** The `--sp-*` names DEFINED inside the block for `#touchpad-page[data-look="<look>"]`. */
function tokensOf(look: string): Set<string> {
  const re = new RegExp(`#touchpad-page\\[data-look="${look}"\\]\\s*\\{([\\s\\S]*?)\\}`);
  const m = re.exec(css);
  assert.ok(m, `the ${look} look block exists`);
  const names = [...m![1].matchAll(/(--sp-[a-z0-9-]+)\s*:/g)].map((x) => x[1]);
  assert.ok(names.length >= 10, `${look} defines a full surface set (${names.length})`);
  return new Set(names);
}

test("the chocolate and app look sheets define the same --sp-* token names", () => {
  const choc = tokensOf("chocolate");
  const app = tokensOf("app");
  const onlyChoc = [...choc].filter((n) => !app.has(n));
  const onlyApp = [...app].filter((n) => !choc.has(n));
  assert.deepEqual(onlyChoc, [], `tokens only in chocolate: ${onlyChoc.join(", ")}`);
  assert.deepEqual(onlyApp, [], `tokens only in app: ${onlyApp.join(", ")}`);
});
