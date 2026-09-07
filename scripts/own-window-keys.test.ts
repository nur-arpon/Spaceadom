/**
 * own-window-keys.test.ts — the PROBLEM 259 fallback's state machine, exercised.
 *
 *   node scripts/own-window-keys.test.ts
 *
 * (Node 22+ strips the types itself; there is no build step and no test
 * framework in this repo. It lives in `scripts/` rather than `src/` for one
 * concrete reason: `tsconfig.json` includes `src` and the project has no
 * `@types/node`, so a `node:test` import in there would fail `npx tsc
 * --noEmit`, which is a shipping gate.)
 *
 * WHAT IS AND IS NOT COVERED. Everything here is the PURE half — `stepOwnWindow`
 * and `vkForOwnWindow`. The DOM half (listener attach/detach, deleting the
 * inserted space out of a real `<input>`) needs a document and is verified in
 * `preview.html`; see the PROBLEM 259 entry in V14_FIXES_AND_CODE.md for that
 * recipe. The split is deliberate: the decisions are what regress, and they are
 * the half that can be tested without a browser.
 */

import test from "node:test";
import assert from "node:assert/strict";

import {
  initialOwnWindowState,
  stepOwnWindow,
  vkForOwnWindow,
  isTextEntry,
  type OwnWindowEffect,
  type OwnWindowInput,
  type OwnWindowState,
  type OwnWindowTiming,
} from "../src/own-window-keys.ts";

const TIMING: OwnWindowTiming = { rolloverMs: 50, holdThresholdMs: 300 };

/** Drive a whole hold and collect every effect, in order. */
function drive(
  inputs: OwnWindowInput[],
  timing: OwnWindowTiming = TIMING,
): { state: OwnWindowState; effects: OwnWindowEffect[] } {
  let state = initialOwnWindowState();
  const effects: OwnWindowEffect[] = [];
  for (const input of inputs) {
    const out = stepOwnWindow(state, input, timing);
    state = out.state;
    effects.push(...out.effects);
  }
  return { state, effects };
}

const kinds = (effects: OwnWindowEffect[]): string[] => effects.map((e) => e.kind);
const count = (effects: OwnWindowEffect[], kind: string): number =>
  effects.filter((e) => e.kind === kind).length;

const spaceDown = (over: Partial<Extract<OwnWindowInput, { kind: "space-down" }>> = {}) =>
  ({
    kind: "space-down",
    at: 0,
    repeat: false,
    chord: false,
    composing: false,
    inTextField: false,
    ...over,
  }) as OwnWindowInput;

const keyDown = (over: Partial<Extract<OwnWindowInput, { kind: "key-down" }>> = {}) =>
  ({
    kind: "key-down",
    at: 0,
    vk: 0x4b, // K
    repeat: false,
    chord: false,
    composing: false,
    ...over,
  }) as OwnWindowInput;

// ---------------------------------------------------------------------------
// The owner's requirement, in three tests
// ---------------------------------------------------------------------------

test("holding Space opens exactly one hold and arms the ring timer", () => {
  const { effects } = drive([spaceDown({ at: 100 })]);
  assert.deepEqual(kinds(effects), ["prevent-default", "space-down", "start-hold-timer"]);
  const timer = effects.find((e) => e.kind === "start-hold-timer");
  assert.equal(timer && "ms" in timer ? timer.ms : null, TIMING.holdThresholdMs);
});

test("Space auto-repeat never opens a second hold", () => {
  const { effects } = drive([
    spaceDown({ at: 100 }),
    spaceDown({ at: 400, repeat: true }),
    spaceDown({ at: 450, repeat: true }),
  ]);
  assert.equal(count(effects, "space-down"), 1);
  assert.equal(count(effects, "start-hold-timer"), 1);
});

test("Space+K past the rollover launches, and the K never reaches the page", () => {
  const { effects } = drive([spaceDown({ at: 100 }), keyDown({ at: 400, vk: 0x4b })]);
  assert.deepEqual(kinds(effects), [
    "prevent-default", // the Space (not a text field)
    "space-down",
    "start-hold-timer",
    "clear-hold-timer",
    "prevent-default", // the K
    "combo",
  ]);
  const combo = effects.find((e) => e.kind === "combo");
  assert.equal(combo && "vk" in combo ? combo.vk : null, 0x4b);
});

// ---------------------------------------------------------------------------
// TAP SEMANTICS — the space the user actually typed
// ---------------------------------------------------------------------------

test("a tap in a text field is never prevented and never invokes a combo", () => {
  const { effects } = drive([
    spaceDown({ at: 100, inTextField: true }),
    { kind: "space-up", at: 180 },
  ]);
  assert.deepEqual(kinds(effects), [
    "capture-caret",
    "space-down",
    "start-hold-timer",
    "clear-hold-timer",
    "space-up",
  ]);
  assert.equal(count(effects, "prevent-default"), 0, "the browser must type the space itself");
  assert.equal(count(effects, "take-back-spaces"), 0, "a tap's space stays");
  assert.equal(count(effects, "combo"), 0);
});

test("a hold in a text field takes its space back at the threshold, once", () => {
  const { effects } = drive([
    spaceDown({ at: 100, inTextField: true }),
    { kind: "hold-threshold" },
    { kind: "hold-threshold" }, // a stray second tick may not double-delete
    { kind: "space-up", at: 2_000 },
  ]);
  assert.equal(count(effects, "take-back-spaces"), 1);
  const back = effects.find((e) => e.kind === "take-back-spaces");
  assert.equal(back && "count" in back ? back.count : null, 1);
});

test("every auto-repeat space the browser inserted is taken back together", () => {
  const { effects } = drive([
    spaceDown({ at: 100, inTextField: true }),
    spaceDown({ at: 350, repeat: true, inTextField: true }),
    spaceDown({ at: 380, repeat: true, inTextField: true }),
    { kind: "hold-threshold" },
  ]);
  const back = effects.find((e) => e.kind === "take-back-spaces");
  assert.equal(back && "count" in back ? back.count : null, 3);
});

test("after the take-back, further repeats are suppressed so no space comes back", () => {
  const { effects } = drive([
    spaceDown({ at: 100, inTextField: true }),
    { kind: "hold-threshold" },
    spaceDown({ at: 500, repeat: true, inTextField: true }),
    spaceDown({ at: 530, repeat: true, inTextField: true }),
  ]);
  // One take-back, then a prevent-default for each repeat that followed it.
  assert.equal(count(effects, "take-back-spaces"), 1);
  assert.equal(count(effects, "prevent-default"), 2);
});

test("a combo in a text field takes the space back even before the threshold", () => {
  const { effects } = drive([
    spaceDown({ at: 100, inTextField: true }),
    keyDown({ at: 200, vk: 0x4b }),
  ]);
  assert.deepEqual(kinds(effects), [
    "capture-caret",
    "space-down",
    "start-hold-timer",
    "clear-hold-timer",
    "prevent-default",
    "take-back-spaces",
    "combo",
  ]);
});

test("outside a text field the Space is prevented — a focused button must not click", () => {
  const { effects } = drive([spaceDown({ at: 100, inTextField: false })]);
  assert.equal(effects[0]?.kind, "prevent-default");
  assert.equal(count(effects, "capture-caret"), 0);
});

// ---------------------------------------------------------------------------
// ROLLOVER — the hook's typing-vs-command rule, PROBLEM 95/218
// ---------------------------------------------------------------------------

test("a letter inside the rollover window is prose: no combo, and the hold ends", () => {
  const { state, effects } = drive([
    spaceDown({ at: 100, inTextField: true }),
    keyDown({ at: 130, vk: 0x53 }), // 30ms < 50ms
  ]);
  assert.equal(count(effects, "combo"), 0);
  assert.equal(count(effects, "prevent-default"), 0, "both characters must be typed");
  assert.equal(count(effects, "take-back-spaces"), 0, "the space is part of the prose");
  assert.equal(count(effects, "space-up"), 1, "the pending ring has to be cancelled");
  assert.equal(state.phase, "aborted");
});

test("an aborted hold does not send a second release when Space finally comes up", () => {
  const { effects } = drive([
    spaceDown({ at: 100, inTextField: true }),
    keyDown({ at: 130, vk: 0x53 }),
    { kind: "space-up", at: 400 },
  ]);
  assert.equal(count(effects, "space-up"), 1);
});

test("rollover_ms of 0 switches the rule off, exactly as it does in the hook", () => {
  const { effects } = drive(
    [spaceDown({ at: 100 }), keyDown({ at: 100, vk: 0x53 })],
    { rolloverMs: 0, holdThresholdMs: 300 },
  );
  assert.equal(count(effects, "combo"), 1);
});

// ---------------------------------------------------------------------------
// THE KEYS THIS FALLBACK MAY NOT TAKE
// ---------------------------------------------------------------------------

test("Ctrl/Alt/Win chords are never intercepted — hook law 4 and PROBLEM 176", () => {
  // Ctrl+Space is an IME switch / autocomplete, not a hold.
  assert.deepEqual(kinds(drive([spaceDown({ at: 100, chord: true })]).effects), []);
  // Win+Shift+S during a hold is a screenshot, not Space+S.
  const { effects } = drive([spaceDown({ at: 100 }), keyDown({ at: 400, chord: true })]);
  assert.equal(count(effects, "combo"), 0);
  assert.equal(count(effects, "prevent-default"), 1, "only the Space's own");
});

test("an IME composition owns the keyboard", () => {
  assert.deepEqual(kinds(drive([spaceDown({ at: 100, composing: true })]).effects), []);
  const { effects } = drive([spaceDown({ at: 100 }), keyDown({ at: 400, composing: true })]);
  assert.equal(count(effects, "combo"), 0);
});

test("a key the map declines is left with the page", () => {
  const { effects } = drive([spaceDown({ at: 100 }), keyDown({ at: 400, vk: null })]);
  assert.equal(count(effects, "combo"), 0);
  assert.equal(count(effects, "prevent-default"), 1);
});

test("a repeating combo key fires once, not once per repeat", () => {
  const { effects } = drive([
    spaceDown({ at: 100 }),
    keyDown({ at: 400, vk: 0x4b }),
    keyDown({ at: 440, vk: 0x4b, repeat: true }),
    keyDown({ at: 480, vk: 0x4b, repeat: true }),
  ]);
  assert.equal(count(effects, "combo"), 1);
});

test("Escape, Enter, Tab, digits and the F-keys are not ours to take", () => {
  for (const ev of [
    { key: "Escape", code: "Escape" },
    { key: "Enter", code: "Enter" },
    { key: "Tab", code: "Tab" },
    { key: "Backspace", code: "Backspace" },
    { key: "ArrowUp", code: "ArrowUp" },
    { key: "F5", code: "F5" },
    { key: "7", code: "Digit7" },
    { key: ";", code: "Semicolon" },
  ]) {
    assert.equal(vkForOwnWindow(ev), null, `${ev.key} must stay with the page`);
  }
});

test("every letter maps to the same virtual key the hook would have seen", () => {
  for (let c = 65; c <= 90; c++) {
    const lower = String.fromCharCode(c + 32);
    const upper = String.fromCharCode(c);
    assert.equal(vkForOwnWindow({ key: lower, code: `Key${upper}` }), c);
    assert.equal(vkForOwnWindow({ key: upper, code: `Key${upper}` }), c, "Shift+letter too");
  }
  // A dead/composed key gives a NAME in `key`; the physical position answers.
  assert.equal(vkForOwnWindow({ key: "Unidentified", code: "KeyF" }), 0x46);
});

test("the three punctuation combos the ring offers", () => {
  assert.equal(vkForOwnWindow({ key: "`", code: "Backquote" }), 0xc0);
  assert.equal(vkForOwnWindow({ key: "~", code: "Backquote" }), 0xc0);
  assert.equal(vkForOwnWindow({ key: ",", code: "Comma" }), 0xbc);
  assert.equal(vkForOwnWindow({ key: ".", code: "Period" }), 0xbe);
});

// ---------------------------------------------------------------------------
// THE LEAK: a hold that loses focus
// ---------------------------------------------------------------------------

test("losing focus mid-hold releases it, so the ring can never be stranded", () => {
  const { state, effects } = drive([spaceDown({ at: 100 }), { kind: "window-blur" }]);
  assert.equal(count(effects, "space-up"), 1);
  assert.equal(count(effects, "clear-hold-timer"), 1);
  assert.equal(state.phase, "idle");
});

test("a release reports whether the hold fired, so the engine sees the hook's shape", () => {
  const held = drive([spaceDown({ at: 100 }), keyDown({ at: 400 }), { kind: "space-up", at: 600 }]);
  const up = held.effects.find((e) => e.kind === "space-up");
  assert.equal(up && "hadCombo" in up ? up.hadCombo : null, true);

  const tapped = drive([spaceDown({ at: 100 }), { kind: "space-up", at: 150 }]);
  const up2 = tapped.effects.find((e) => e.kind === "space-up");
  assert.equal(up2 && "hadCombo" in up2 ? up2.hadCombo : null, false);
});

test("a stray release with no hold behind it does nothing at all", () => {
  assert.deepEqual(kinds(drive([{ kind: "space-up", at: 10 }]).effects), []);
  assert.deepEqual(kinds(drive([{ kind: "window-blur" }]).effects), []);
  assert.deepEqual(kinds(drive([keyDown({ at: 10 })]).effects), []);
});

// ---------------------------------------------------------------------------
// WHERE A SPACE IS A CHARACTER, AND WHERE IT IS A CLICK
// ---------------------------------------------------------------------------

test("isTextEntry separates a field from a button without a DOM", () => {
  const el = (tagName: string, extra: Record<string, unknown> = {}) =>
    ({ tagName, isContentEditable: false, ...extra }) as unknown as Element;

  assert.equal(isTextEntry(null), false);
  assert.equal(isTextEntry(el("TEXTAREA")), true);
  assert.equal(isTextEntry(el("INPUT", { type: "text" })), true);
  assert.equal(isTextEntry(el("INPUT", { type: "Search" })), true, "type is case-insensitive");
  assert.equal(isTextEntry(el("INPUT", { type: "checkbox" })), false, "Space activates it");
  assert.equal(isTextEntry(el("INPUT", { type: "range" })), false);
  assert.equal(isTextEntry(el("BUTTON")), false);
  assert.equal(isTextEntry(el("DIV")), false);
  assert.equal(isTextEntry(el("DIV", { isContentEditable: true })), true);
});
