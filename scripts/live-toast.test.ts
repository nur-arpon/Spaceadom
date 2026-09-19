/**
 * live-toast.test.ts — 1.0.123 (owner, 2026-09-19): the LIVE toast is ONE
 * keyed pill updated in place, never a stack. `toast.ts` has no DOM harness
 * (it imports the Tauri API and touches AudioContext at module scope), so
 * this pins the SOURCE the way touchpad-page.test.ts does: the reuse path
 * exists and comes first, the update touches only the text nodes, the end
 * path arms the ordinary leave/retire clock, `showToast` is untouched, and
 * the Rust side rate-limits with the pure limiter.
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
const lib = readFileSync(join(root, "src-tauri", "src", "lib.rs"), "utf8");

function body(name: string): string {
  const start = toast.indexOf(`export function ${name}(`);
  assert.ok(start >= 0, `${name} is exported from toast.ts`);
  // A top-level function ends at the first "}" in column 0 after its head.
  const end = toast.indexOf("\n}\n", start);
  return toast.slice(start, end < 0 ? undefined : end + 2);
}

test("a second showLiveToast with the same key reuses the element and adds nothing", () => {
  const b = body("showLiveToast");
  const reuse = b.indexOf("_live.get(key)");
  const create = b.indexOf("document.createElement");
  assert.ok(reuse >= 0 && create >= 0, "both the reuse and the create path exist");
  assert.ok(reuse < create, "the reuse path is checked BEFORE anything is created");
  // The reuse branch returns before reaching createElement/appendChild.
  const branch = b.slice(reuse, create);
  assert.ok(/return;\s*\}/.test(branch), "the reuse branch returns without creating");
  assert.ok(branch.includes('querySelector(".msg")') && branch.includes("textContent"), "it swaps the text node");
  assert.ok(!branch.includes("appendChild"), "it appends nothing");
  assert.ok(!branch.includes("relayout(") && !branch.includes("requestFit(") && !branch.includes("beep("),
    "an update restarts nothing: no relayout, no fit, no beep");
  assert.ok(b.includes("_live.set(key, entry)"), "the created pill is registered under its key");
  assert.ok(b.includes("live: true"), "the entry is marked live");
});

test("endLiveToast lingers then arms the ordinary leave/retire clock", () => {
  const b = body("endLiveToast");
  assert.ok(b.includes("_live.delete(key)"), "the key is released");
  assert.ok(b.includes("armEntry(cur, leaveIn, leaveIn + LEAVE_MS)"), "the normal clock (leave, then retire)");
  assert.ok(/const LIVE_LINGER_MS = 600;/.test(toast), "~600 ms after Exit");
});

test("showToast itself is untouched by the live path", () => {
  const b = body("showToast");
  assert.ok(!b.includes("_live") && !b.includes("live:"), "showToast never mentions the live map");
  assert.ok(b.includes("while (_toasts.length > 3)"), "its eviction rule is the one it always had");
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
