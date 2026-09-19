/**
 * touchpad-page.test.ts — 1.0.122 (owner decision 2026-09-19 17:30): the
 * touchpad page has NO reserved edge and NO restricted action list. The
 * bottom edge is a normal edge, every edge offers Brightness · Volume ·
 * Video scrub · Any shortcut · Nothing, and "Any shortcut" is live with the
 * key editor's chord recorder. This pins the page's source so a "Later" /
 * "Reserved" state can never quietly come back.
 *
 *   node scripts/touchpad-page.test.ts
 *
 * Same no-framework, regex-as-parser arrangement as touchpad-tokens.test.ts.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const page = readFileSync(join(root, "src", "components", "touchpad-page.ts"), "utf8");
const css = readFileSync(join(root, "src", "styles", "touchpad.css"), "utf8");
const themes = readFileSync(join(root, "src", "styles", "themes.css"), "utf8");

test('no "Later" / "Reserved" state remains on the touchpad page', () => {
  for (const word of [/\bLater\b/, /\bReserved\b/, /\breserved\b/, /is-reserved/, /Coming later/]) {
    assert.ok(!word.test(page), `touchpad-page.ts still says ${word}`);
  }
  assert.ok(!/is-reserved/.test(css), "touchpad.css no longer styles a reserved band");
});

test("the bottom edge is an ordinary edge row and every edge offers every action", () => {
  assert.match(page, /\$\{row\("bottom"\)\}/, "the Edges panel lists the bottom edge like the others");
  assert.match(page, /of 4 on/, "the count is out of four edges");
  for (const kind of ["brightness", "volume", "seek", "scrub", "chords", "none"]) {
    assert.ok(page.includes(`actionRow("${kind}"`), `the Does-what list offers ${kind}`);
  }
  assert.ok(!/actionRow\([^)]*,\s*true\)/.test(page), "no action row is disabled");
});

// 1.0.130 (owner decisions 2026-09-20 00:50): the "Does what" order is
// Brightness · Volume · Video seek · Video scrub · Tabs · Zoom · Undo / Redo ·
// Copy / Paste · Track · Any shortcut · Nothing; Seek is the top band's default;
// Seek and Scrub are two SEPARATE choices (scrub is only the automatic
// fallback inside a seek gesture) and the Scrub row says what it is.
test('the "Does what" list is in the owner\'s order, with the five presets between scrub and Any shortcut', () => {
  const order = ["brightness", "volume", "seek", "scrub", "PRESETS.map", "chords", "none"];
  const idx = order.map((k) => (k === "PRESETS.map" ? page.indexOf("PRESETS.map((p) => actionRow(") : page.indexOf(`actionRow("${k}"`)));
  for (let i = 0; i < idx.length; i++) {
    assert.ok(idx[i] >= 0, `${order[i]} is in the list`);
    if (i) assert.ok(idx[i] > idx[i - 1], `${order[i]} comes after ${order[i - 1]}`);
  }
  const presets = [...page.matchAll(/\{ id: "([a-z_]+)", name: "([^"]+)"/g)].map((m) => [m[1], m[2]]);
  assert.deepEqual(presets, [
    ["tabs", "Tabs"],
    ["zoom", "Zoom"],
    ["undo_redo", "Undo / Redo"],
    ["copy_paste", "Copy / Paste"],
    ["track", "Track"],
  ]);
  assert.match(page, /once: true \},\s*\{ id: "track"/, "Copy / Paste fires once per slide");
  assert.match(page, /id: "track"[^}]*once: true/, "Track fires once per slide");
  assert.match(page, /scrub: "5-second hops, works in every player"/, "the Scrub row's description");
  assert.match(page, /edge === "top" \? "seek"/, "the top band's default is Video seek");
  assert.match(page, /Turn on the top edge and a video follows your finger\./, "the first-run copy");
  // Picking "Any shortcut" over a preset starts from the preset's pair.
  assert.match(page, /presetSpec\(p\)\.forward/, "a preset's pair seeds Any shortcut");
});

test('"Any shortcut" uses the key editor\'s recorder and two axis-aware fields', () => {
  assert.match(page, /data-chord-rec="\$\{dir\}"/, "a record button per direction");
  assert.match(page, /data-chord-clear="\$\{dir\}"/, "a clear button per direction");
  assert.match(page, /Slide \$\{dirWord\(edge, dir\)\} sends/, "the labels say up/down or right/left");
  assert.match(page, /recorder\?: ChordRecorderHost/, "the host supplies the recorder");
  const main = readFileSync(join(root, "src", "main.ts"), "utf8");
  for (const cmd of ["chord_record_start", "chord_record_poll", "chord_record_stop"]) {
    assert.ok(main.includes(cmd), `main.ts wires ${cmd} into the touchpad page`);
  }
});

test("the touchpad page stays clickable in the Navy (starry) theme", () => {
  // The fun-mode pointer-events opt-in list must keep #touchpad-page.
  const re = /body\[data-theme="starry"\]\[data-fun="on"\] #touchpad-page/;
  assert.match(themes, re, "themes.css opts #touchpad-page into pointer events under starry");
});
