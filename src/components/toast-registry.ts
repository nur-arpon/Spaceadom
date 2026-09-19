/**
 * toast-registry.ts — the PURE half of "toasts never stack for the same
 * thing" (owner, 2026-09-20 01:15, 1.0.131). A LEAF module: it imports
 * nothing, touches no DOM and no timers, so `scripts/toast-registry.test.ts`
 * can drive it in plain Node (`toast.ts` itself cannot be loaded there — it
 * imports the Tauri API and touches AudioContext at module scope).
 *
 * `toast.ts` keeps one array of entries, newest last. Every entry carries
 * its text, its phase ("dot" = entering, "open", "leave" = fading out) and,
 * for a LIVE pill, its key. Two questions are answered here:
 *
 *   planLive(entries, key)   — a `showLiveToast(key, …)` should CREATE a
 *                              pill (none with that key), UPDATE the one that
 *                              is up (dot/open), or REVIVE the one that is
 *                              fading after `endLiveToast` (leave).
 *   findIdentical(entries, text) — a `showToast(text)` should restart THIS
 *                              ordinary pill's clock in place instead of
 *                              adding another: identical text only, still
 *                              open or fading, never a live pill and never
 *                              one still entering (its clock is fresh).
 */

export type ToastPhase = "dot" | "open" | "leave";

export interface RegistryEntry {
  /** The message text as shown (after the leading-glyph split). */
  text: string;
  phase: ToastPhase;
  /** A keyed live pill (updated in place); absent/false for an ordinary toast. */
  live?: boolean;
  /** The live pill's key. */
  key?: string;
}

export type LivePlan<T> =
  | { kind: "create" }
  | { kind: "update"; entry: T }
  | { kind: "revive"; entry: T };

/** What `showLiveToast(key)` should do, given the pills currently up. A pill
 *  with that key in ANY phase is reused — "leave" (fading after
 *  `endLiveToast`) is revived, "dot"/"open" is updated in place; only when
 *  none exists is a new element created. */
export function planLive<T extends RegistryEntry>(entries: readonly T[], key: string): LivePlan<T> {
  // Newest last: if two ever existed (they should not), the newest wins.
  for (let i = entries.length - 1; i >= 0; i--) {
    const e = entries[i];
    if (!e.live || e.key !== key) continue;
    return e.phase === "leave" ? { kind: "revive", entry: e } : { kind: "update", entry: e };
  }
  return { kind: "create" };
}

/** The ordinary pill whose clock `showToast(text)` should restart instead of
 *  stacking: the newest non-live entry with IDENTICAL text that is open or
 *  fading. `null` when none — different text still stacks as before. */
export function findIdentical<T extends RegistryEntry>(entries: readonly T[], text: string): T | null {
  for (let i = entries.length - 1; i >= 0; i--) {
    const e = entries[i];
    if (e.live) continue;
    if (e.text !== text) continue;
    if (e.phase !== "open" && e.phase !== "leave") continue;
    return e;
  }
  return null;
}
