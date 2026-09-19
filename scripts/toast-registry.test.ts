/**
 * toast-registry.test.ts — 1.0.131 (owner, 2026-09-20 01:15): TOASTS NEVER
 * STACK FOR THE SAME THING. Drives the leaf module directly (no DOM, no
 * Tauri): a live pill with the same key is reused in every phase — updated
 * while up, REVIVED while fading — and an ordinary toast with identical text
 * restarts in place; different text still stacks.
 *
 *   node scripts/toast-registry.test.ts
 */

import test from "node:test";
import assert from "node:assert/strict";

import { planLive, findIdentical, type RegistryEntry } from "../src/components/toast-registry.ts";

type E = RegistryEntry & { id: number };
const e = (id: number, text: string, phase: RegistryEntry["phase"], key?: string): E =>
  key ? { id, text, phase, live: true, key } : { id, text, phase };

test("planLive: no pill with the key → create", () => {
  assert.deepEqual(planLive([], "touchpad"), { kind: "create" });
  assert.deepEqual(planLive([e(1, "Chrome", "open")], "touchpad"), { kind: "create" }, "an ordinary pill is not a live one");
  assert.deepEqual(planLive([e(1, "Volume 40%", "open", "app-volume")], "touchpad"), { kind: "create" }, "another key is another pill");
});

test("planLive: a pill with the key that is up (dot or open) → update in place", () => {
  const dot = e(1, "Volume 40%", "dot", "touchpad");
  assert.deepEqual(planLive([dot], "touchpad"), { kind: "update", entry: dot }, "still entering: update (its open timer is kept)");
  const open = e(2, "Volume 41%", "open", "touchpad");
  assert.deepEqual(planLive([e(0, "Chrome", "open"), open], "touchpad"), { kind: "update", entry: open });
});

test("planLive: a pill with the key that is FADING after endLiveToast → revive, same entry", () => {
  const fading = e(3, "Volume 55%", "leave", "touchpad");
  const plan = planLive([e(0, "Chrome", "open"), fading, e(4, "Zoom", "open")], "touchpad");
  assert.equal(plan.kind, "revive");
  assert.equal(plan.kind === "revive" && plan.entry, fading, "the SAME entry (same element, same slot), never a new one");
});

test("planLive: the newest pill with the key wins if two ever exist", () => {
  const older = e(1, "a", "leave", "k");
  const newer = e(2, "b", "open", "k");
  assert.deepEqual(planLive([older, newer], "k"), { kind: "update", entry: newer });
});

test("findIdentical: identical text that is open or fading restarts in place", () => {
  const open = e(1, "Chrome", "open");
  assert.equal(findIdentical([open], "Chrome"), open);
  const fading = e(2, "Chrome", "leave");
  assert.equal(findIdentical([fading], "Chrome"), fading, "a fading pill is revived, not duplicated");
  assert.equal(findIdentical([open, fading], "Chrome"), fading, "the newest identical pill");
});

test("findIdentical: different text still stacks; a live pill and an entering pill are never matched", () => {
  assert.equal(findIdentical([e(1, "Chrome", "open")], "Edge"), null, "different text → a new pill, as today");
  assert.equal(findIdentical([e(1, "Chrome", "open")], "chrome"), null, "IDENTICAL means identical (case too)");
  assert.equal(findIdentical([e(1, "Chrome", "open", "touchpad")], "Chrome"), null, "a live pill is not an ordinary toast");
  assert.equal(findIdentical([e(1, "Chrome", "dot")], "Chrome"), null, "still entering: its clock is fresh, leave it");
  assert.equal(findIdentical([], "Chrome"), null);
});
