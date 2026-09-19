/**
 * live-toast.test.ts — 1.0.123 (owner, 2026-09-19): the LIVE toast is ONE
 * keyed pill updated in place, never a stack. 1.0.131 (owner, 2026-09-20
 * 01:15): TOASTS NEVER STACK FOR THE SAME THING — a live pill is found by key
 * in ANY phase and REVIVED while fading, an ordinary toast with IDENTICAL
 * text restarts in place, the live linger is 1500 ms, and the reader uses
 * ONE key for every edge. `toast.ts` has no DOM harness (it imports the
 * Tauri API and touches AudioContext at module scope), so this pins the
 * SOURCE the way touchpad-page.test.ts does; the pure decisions are driven
 * for real in scripts/toast-registry.test.ts.
 *
 *   node scripts/live-toast.test.ts
 */

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const toast = readFileSync(join(root, "src", "components", "toast.ts"), "utf8");
const reader = readFileSync(join(root, "src-tauri", "src", "touchpad", "mod.rs"), "utf8");
const engine = readFileSync(join(root, "src-tauri", "src", "engine", "mod.rs"), "utf8");
const lib = readFileSync(join(root, "src-tauri", "src", "lib.rs"), "utf8");

function body(name: string): string {
  const start = toast.indexOf(`export function ${name}(`);
  assert.ok(start >= 0, `${name} is exported from toast.ts`);
  // A top-level function ends at the first "}" in column 0 after its head.
  const end = toast.indexOf("\n}\n", start);
  return toast.slice(start, end < 0 ? undefined : end + 2);
}

function fn(name: string): string {
  const start = toast.indexOf(`function ${name}(`);
  assert.ok(start >= 0, `${name} exists in toast.ts`);
  const end = toast.indexOf("\n}\n", start);
  return toast.slice(start, end < 0 ? undefined : end + 2);
}

test("showLiveToast plans by key over the one stack: update / revive come BEFORE anything is created", () => {
  const b = body("showLiveToast");
  assert.ok(toast.includes('import { planLive, findIdentical } from "./toast-registry";'), "the pure decisions come from the leaf module");
  const plan = b.indexOf("planLive(_toasts, key)");
  const update = b.indexOf('plan.kind === "update"');
  const revive = b.indexOf('plan.kind === "revive"');
  const create = b.indexOf("document.createElement");
  assert.ok(plan >= 0 && update >= 0 && revive >= 0 && create >= 0, "plan, update, revive and create all exist");
  assert.ok(plan < update && update < revive && revive < create, "update and revive are checked BEFORE anything is created");
  const branches = b.slice(update, b.indexOf("const { accent"));
  assert.equal((branches.match(/return;/g) ?? []).length, 2, "both the update and the revive branch return without creating");
  assert.ok(!branches.includes("appendChild") && !branches.includes("createElement"), "neither appends anything");
  assert.ok(branches.includes("setLiveText(plan.entry, letter, text)"), "both swap the text node");
  assert.ok(branches.includes("reviveEntry(plan.entry)"), "the revive branch un-fades the same entry");
  assert.ok(!toast.includes("_live.get(") && !toast.includes("new Map<string, ToastEntry>"), "no separate live map to fall out of sync");
  assert.ok(b.includes("live: true") && b.includes("text, key,"), "the created pill is marked live and carries its key");
});

test("the update path touches only the text nodes; the revive path cancels the exit and un-fades", () => {
  const set = fn("setLiveText");
  assert.ok(set.includes('querySelector(".msg")') && set.includes("textContent"), "it swaps the text node");
  assert.ok(!set.includes("appendChild") && !set.includes("relayout(") && !set.includes("requestFit(") && !set.includes("beep("),
    "an update restarts nothing: no relayout, no fit, no beep");
  const rev = fn("reviveEntry");
  assert.ok(rev.includes('if (t.phase === "dot") return;'), "a pill still entering keeps its open timer");
  assert.ok(rev.includes("window.clearTimeout(h)"), "the pending leave/retire is cancelled");
  assert.ok(rev.includes('t.el.classList.remove("leave")') && rev.includes('t.el.classList.add("open")') && rev.includes('t.phase = "open"'),
    "the exit class comes off; same element, same slot");
});

test("endLiveToast lingers 1500 ms then arms the ordinary leave/retire clock, and leaves the pill findable", () => {
  const b = body("endLiveToast");
  assert.ok(b.includes("planLive(_toasts, key)"), "found by key");
  assert.ok(b.includes('if (plan.kind !== "update") return;'), "none, or already fading: a no-op");
  assert.ok(b.includes("armEntry(cur, leaveIn, leaveIn + LEAVE_MS)"), "the normal clock (leave, then retire)");
  assert.ok(!b.includes("_live.delete") && !b.includes("splice"), "the key is NOT released here — retire removes it, so a new showLiveToast in the linger revives it");
  assert.ok(/const LIVE_LINGER_MS = 1500;/.test(toast), "1500 ms after Exit (owner, 2026-09-20: was 600)");
  assert.ok(b.includes("LIVE_LINGER_MS + (wasArmed ? OPEN_AT : 0)"), "one constant, used by every key");
});

test("showToast restarts an IDENTICAL toast in place instead of stacking; different text still stacks", () => {
  const b = body("showToast");
  const dup = b.indexOf("findIdentical(_toasts, text)");
  const create = b.indexOf("document.createElement");
  assert.ok(dup >= 0 && create >= 0 && dup < create, "the identical check comes BEFORE anything is created");
  const branch = b.slice(dup, create);
  assert.ok(branch.includes("reviveEntry(dup)") && branch.includes("restartDrain(dup, duration)") && branch.includes("armEntry(dup, duration, duration + LEAVE_MS)"),
    "un-fade, re-run the drain ring, restart the clock in place");
  assert.ok(/return;\s*\}/.test(branch), "and return without creating");
  assert.ok(branch.includes("_flying === 0") && branch.includes("!_absorbed.includes(dup)"), "a pill mid-flight or wearing SPACE is left to the normal path");
  assert.ok(b.includes("while (_toasts.length > 3)"), "its eviction rule is the one it always had");
  assert.ok(b.includes("el, phase: \"dot\", text, duration"), "the entry carries its text for the next identical check");
});

test("relayout leaves a live pill full size and out of the depth count", () => {
  const start = toast.indexOf("function relayout(): void {");
  const b = toast.slice(start, toast.indexOf("\n}\n", start));
  assert.ok(b.includes('if (t.live) { t.el.dataset.depth = "0"; continue; }'));
  assert.ok(b.includes('x.phase === "open" && !x.live'));
});

test("the overlay listens for toast-live / toast-live-end and Rust emits them globally", () => {
  assert.ok(toast.includes('listen<{ key?: string; text?: string } | null>("toast-live"'));
  assert.ok(toast.includes('"toast-live-end"'));
  assert.ok(lib.includes('app_handle.emit("toast-live", LiveToastPayload { key, text })'));
  assert.ok(lib.includes('app_handle.emit("toast-live-end", LiveToastEndPayload { key })'));
});

test("the reader rate-limits with the pure limiter and reads the switch from its snapshot", () => {
  assert.ok(reader.includes("pub(crate) const LIVE_TOAST_MIN_MS: u64 = 125;"), "≤ 8 Hz");
  assert.ok(reader.includes("toast_limit.allow(t0.elapsed().as_millis() as u64)"), "Move goes through the limiter");
  assert.ok(reader.includes("if cfg.slide_toast {"), "gated on the snapshot's field, not the config lock");
  const cb = reader.slice(reader.indexOf("let on_report = Box::new"), reader.indexOf("raw::run_sink(&pad, on_report)"));
  assert.ok(!cb.includes("config()."), "no config-lock read on the raw-input callback");
});

test("Rust uses ONE stable key per thing: \"touchpad\" for every edge, \"app-volume\" for the specials", () => {
  assert.ok(reader.includes('const LIVE_TOAST_KEY: &str = "touchpad";'), "one key for all four edges — back-to-back slides on different edges reuse the pill");
  const cb = reader.slice(reader.indexOf("let on_report = Box::new"), reader.indexOf("raw::run_sink(&pad, on_report)"));
  assert.ok(!/show_live_toast\(&app, "/.test(cb) && !/end_live_toast\(&app, "/.test(cb), "the reader never passes a literal key — only LIVE_TOAST_KEY");
  assert.equal((engine.match(/"app-volume"/g) ?? []).length, 3, "app-volume: one show + two end paths, the same literal");
});
