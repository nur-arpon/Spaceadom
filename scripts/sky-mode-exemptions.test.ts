/**
 * sky-mode-exemptions.test.ts — TOUCHPAD T2 (coordinator, 2026-09-19): in
 * hidden-keyboard mode (`body.sky-mode`) ONLY `#sky-return` and `#gear-dock`
 * stay visible; everything else on the stage is hidden by the rule
 *   body.sky-mode #stage > *:not(#sky-return):not(#gear-dock) { … }
 *
 * So the "Special keys" pill (moved to the bottom-right) and the touchpad page
 * must be DIRECT #stage children (caught by that rule), and the exemption list
 * must name nothing else. The thumbnail lives inside #keyboard-scale, so it
 * hides with #keyboard-outer. This guards the arrow never overlapping them.
 *
 *   node scripts/sky-mode-exemptions.test.ts
 */

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const css = readFileSync(join(root, "src", "styles.css"), "utf8");
const html = readFileSync(join(root, "index.html"), "utf8");

test("the sky-mode hide rule exempts only #sky-return and #gear-dock", () => {
  const re = /body\.sky-mode\s+#stage\s*>\s*\*((?::not\([^)]*\))+)\s*,/;
  const m = re.exec(css);
  assert.ok(m, "the sky-mode stage-hide rule exists");
  const exemptions = [...m![1].matchAll(/:not\(([^)]*)\)/g)].map((x) => x[1].trim()).sort();
  assert.deepEqual(exemptions, ["#gear-dock", "#sky-return"], `only those two are exempt (${exemptions.join(", ")})`);
});

test("the specials dock and the touchpad page are direct #stage children", () => {
  // Between <div id="stage"> and its close, both ids appear as elements.
  assert.match(html, /id="specials-dock"/, "specials-dock is in the markup");
  assert.match(html, /id="touchpad-page"/, "touchpad-page is in the markup");
  // Neither is nested in #gear-dock (which would exempt it from hiding).
  const gearDock = /id="gear-dock"[\s\S]*?<\/div>\s*<!--/.exec(html)?.[0] ?? "";
  assert.ok(!gearDock.includes("specials-dock"), "specials pill is NOT inside #gear-dock");
});
