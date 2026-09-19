/**
 * setting-subs.test.ts — 1.0.119 (brief 4 §5): the per-option subtitle map is
 * COMPLETE against the option tables the pills are built from.
 *
 *   node scripts/setting-subs.test.ts
 *
 * Same arrangement as own-window-keys.test.ts: Node 22+ strips the types, no
 * framework, and it lives in `scripts/` because `tsconfig.json` includes
 * `src` and the project has no `@types/node`.
 *
 * The option tables are read from `controls.ts` AS TEXT rather than
 * imported: that module imports `./report-dialog` extensionless (Vite
 * resolves it, Node's ESM loader does not), and the tables are plain
 * literals, so the regex below is the whole parser.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import { SUB_LINES, subLineFor, subLineHtml } from "../src/components/setting-subs.ts";

const here = dirname(fileURLToPath(import.meta.url));
const controls = readFileSync(join(here, "..", "src", "components", "controls.ts"), "utf8");

/** `export const NAME_OPTS: … = [ ["value", "Label"], … ];` → the values. */
function optionsOf(name: string): string[] {
  const m = new RegExp(`export const ${name}[^=]*=\\s*\\[([\\s\\S]*?)\\];`).exec(controls);
  assert.ok(m, `${name} is declared in controls.ts`);
  const values = [...m![1].matchAll(/\[\s*"([^"]+)"\s*,\s*"[^"]*"\s*\]/g)].map((x) => x[1]);
  assert.ok(values.length >= 2, `${name} has options: ${values.join(",")}`);
  return values;
}

const PILLS: ReadonlyArray<readonly [string, string]> = [
  ["middlestyle", "MIDDLE_STYLE_OPTS"],
  ["middlescope", "MIDDLE_SCOPE_OPTS"],
  ["alllayout", "ALL_LAYOUT_OPTS"],
  ["hudring", "RING_OPTS"],
  ["touchpadlook", "TOUCHPAD_LOOK_OPTS"],
];

/** 1.0.123 — TOGGLES that carry a sub line have exactly the options "on" and
 *  "off". Listed here, not parsed: a switch has no option table. */
const TOGGLES: ReadonlyArray<string> = ["tpslidetoast"];
const TOGGLE_OPTS = ["on", "off"] as const;

/** Every control with a map is either a pill or a listed toggle — a map for
 *  a control nothing renders would be an orphan. */
const ALL_CONTROLS: ReadonlyArray<readonly [string, () => string[]]> = [
  ...PILLS.map(([control, table]) => [control, () => optionsOf(table)] as const),
  ...TOGGLES.map((control) => [control, () => [...TOGGLE_OPTS]] as const),
];

test("every option of every pill has its own one-line subtitle", () => {
  for (const [control, opts_] of ALL_CONTROLS) {
    const opts = opts_();
    assert.ok(SUB_LINES[control], `${control} has a map`);
    for (const value of opts) {
      const line = subLineFor(control, value);
      assert.ok(line.length > 0, `${control}.${value} has a line`);
      assert.ok(line.length <= 60, `${control}.${value} is one short clause: "${line}"`);
      assert.ok(/[.!]$/.test(line), `${control}.${value} ends like a sentence`);
    }
    // And nothing in the map that is not an option (a renamed value would
    // otherwise keep an orphan line forever).
    for (const key of Object.keys(SUB_LINES[control]!)) {
      assert.ok(opts.includes(key), `${control}.${key} is a real option`);
    }
  }
});

test("the lines differ per option, so the text visibly follows the choice", () => {
  for (const [control, opts_] of ALL_CONTROLS) {
    const lines = opts_().map((v) => subLineFor(control, v));
    assert.equal(new Set(lines).size, lines.length, `${control}: no two options share a line`);
  }
});

test("no map is an orphan: every SUB_LINES key is a pill or a listed toggle", () => {
  const known = new Set(ALL_CONTROLS.map(([c]) => c));
  for (const key of Object.keys(SUB_LINES)) {
    assert.ok(known.has(key), `${key} is rendered by settings-panel.ts (add it to PILLS or TOGGLES)`);
  }
  // And the toggle really is wired: the panel renders its line and its switch.
  const panel = readFileSync(join(here, "..", "src", "components", "settings-panel.ts"), "utf8");
  for (const control of TOGGLES) {
    assert.ok(panel.includes(`subLineHtml("${control}"`), `${control}'s sub line is rendered`);
    assert.ok(panel.includes(`wireToggle("${control}"`), `${control}'s switch is wired`);
  }
});

test("the markup carries the id paintSubLine swaps by", () => {
  assert.equal(
    subLineHtml("alllayout", "spiral"),
    `<div class="set-sub" id="set-alllayout-sub">Packed like a sunflower’s seeds.</div>`,
  );
  assert.equal(subLineFor("nosuch", "x"), "");
});
