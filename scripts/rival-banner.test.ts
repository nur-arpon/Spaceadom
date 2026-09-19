/**
 * rival-banner.test.ts — PROBLEM 272: the second-install banner's words and
 * button, per backend verdict, pinned.
 *
 *   node scripts/rival-banner.test.ts
 *
 * (Node 22+ strips the types; no framework. Lives in `scripts/` because
 * `tsconfig.json` includes `src` and has no `@types/node` — same reasoning as
 * own-window-keys.test.ts.)
 *
 * The two that matter most are the NEGATIVE ones: the packaged side must
 * never be offered a removal (it cannot perform one), and no arm may
 * interpolate the `store_copy` sentence as if it were a path.
 */

import test from "node:test";
import assert from "node:assert/strict";

import {
  rivalBannerCopy,
  STORE_COPY_DIRECTIONS,
  type RivalBannerInputs,
} from "../src/components/rival-banner.ts";

const STORE_SENTENCE =
  "a Microsoft Store copy is also installed (LOCALTEST.Spaceadom_1.0.100.0_x64__nj4cr7rfsqc4c) — keep one";

function inputs(over: Partial<RivalBannerInputs>): RivalBannerInputs {
  return {
    kind: "second_copy",
    portable: false,
    version: "1.0.94",
    path: "C:\\Program Files\\Spaceadom\\spaceadom.exe",
    ...over,
  };
}

test("store_copy: the unpackaged side gets ONE button that removes the Store copy, after a confirm", () => {
  const c = rivalBannerCopy(inputs({ kind: "store_copy", path: STORE_SENTENCE, version: "1.0.100" }));
  assert.ok(c);
  assert.equal(c.button?.action, "remove_store");
  assert.equal(c.button?.label, "Remove the Store copy");
  assert.ok(c.confirm, "the removal is confirmed once");
  assert.equal(c.confirm?.confirmLabel, "Remove the Store copy");
  assert.match(c.text, /A Microsoft Store copy of Spaceadom is also installed — remove it\?/);
  // No UAC is involved; the text must say so and must not promise one.
  assert.doesNotMatch(c.text, /ask for permission/i);
  assert.match(c.text, /no permission prompt/);
});

test("store_copy: neither the sentence nor the version is interpolated into the text", () => {
  const c = rivalBannerCopy(inputs({ kind: "store_copy", path: STORE_SENTENCE, version: "1.0.100" }));
  assert.ok(c);
  assert.ok(!c.text.includes(STORE_SENTENCE), "the sentence is not a path and must not be rendered as one");
  assert.ok(!c.text.includes("LOCALTEST"), "the package full name is log material, not banner material");
  assert.ok(!c.text.includes("1.0.100"));
});

test("store_copy: the failure toast carries the directions the old banner showed", () => {
  const c = rivalBannerCopy(inputs({ kind: "store_copy", path: STORE_SENTENCE }));
  assert.ok(c);
  assert.ok(c.failToast.includes(STORE_COPY_DIRECTIONS));
  assert.match(STORE_COPY_DIRECTIONS, /Settings > Apps > Installed apps/);
  assert.match(STORE_COPY_DIRECTIONS, /keep it and uninstall this copy instead/);
});

test("packaged_host: the Store copy is NEVER offered a removal — directions and a door only", () => {
  const c = rivalBannerCopy(inputs({ kind: "packaged_host", path: "C:\\Users\\x\\AppData\\Local\\Spaceadom\\spaceadom.exe" }));
  assert.ok(c);
  assert.equal(c.button?.action, "open_installed_apps");
  assert.equal(c.button?.label, "Open Installed apps");
  assert.equal(c.confirm, null);
  assert.match(c.text, /cannot remove the other one for you/);
  assert.match(c.text, /remove this copy from Installed apps, or keep it and uninstall the other/);
  assert.doesNotMatch(c.button!.label, /Remove/);
  // It names the other copy — that is what the person has to find in the list.
  assert.ok(c.text.includes("v1.0.94"));
  assert.ok(c.text.includes("AppData\\Local\\Spaceadom"));
});

test("orphaned_entry: registry-only wording, the leftover-entry button, no confirm", () => {
  const c = rivalBannerCopy(inputs({ kind: "orphaned_entry", path: "a leftover installer entry — no separate copy of Spaceadom is actually running" }));
  assert.ok(c);
  assert.equal(c.button?.action, "repair");
  assert.equal(c.button?.label, "Remove the leftover entry");
  assert.equal(c.confirm, null);
  assert.match(c.text, /Nothing is running twice/);
  assert.match(c.text, /only removes the leftover entry/);
  assert.match(c.text, /not touched/);
});

test("second_copy: the original PROBLEM 129/141 banner, verbatim", () => {
  const c = rivalBannerCopy(inputs({}));
  assert.ok(c);
  assert.equal(c.button?.action, "repair");
  assert.equal(c.button?.label, "Remove the old copy");
  assert.equal(c.confirm, null);
  assert.equal(
    c.text,
    "Another copy of Spaceadom (v1.0.94) is installed at C:\\Program Files\\Spaceadom\\spaceadom.exe. " +
      "Both start with Windows and fight over the spacebar. " +
      "One click removes the old one (Windows will ask for permission once).",
  );
});

test("second_copy + portable: PROBLEM 254 wording, same button", () => {
  const c = rivalBannerCopy(inputs({ portable: true }));
  assert.ok(c);
  assert.match(c.text, /PORTABLE copy/);
  assert.match(c.text, /Closing this portable copy/);
  assert.equal(c.button?.label, "Remove the old copy");
  assert.equal(c.button?.action, "repair");
});

test("portable never changes the wording of the store, packaged or orphan arms", () => {
  for (const kind of ["store_copy", "packaged_host", "orphaned_entry"]) {
    const a = rivalBannerCopy(inputs({ kind, portable: false }));
    const b = rivalBannerCopy(inputs({ kind, portable: true }));
    assert.deepEqual(a, b, kind);
  }
});

test("an unknown kind renders nothing", () => {
  assert.equal(rivalBannerCopy(inputs({ kind: "something_new" })), null);
});

test("only store_copy asks for a confirm, and only remove_store is behind one", () => {
  for (const kind of ["second_copy", "orphaned_entry", "packaged_host", "store_copy"]) {
    const c = rivalBannerCopy(inputs({ kind }))!;
    assert.equal(c.confirm !== null, c.button?.action === "remove_store", kind);
  }
});
