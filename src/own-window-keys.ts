/**
 * own-window-keys.ts — the OWN-WINDOW FALLBACK (PROBLEM 259).
 *
 * WHY THIS FILE EXISTS
 * --------------------
 * PROBLEM 257, measured live on 2026-09-06/07: while Spaceadom's own WebView2
 * window holds the foreground, NEITHER `WH_KEYBOARD_LL` hook in the process is
 * called. In one 60-second sample with the dashboard focused the whole time:
 * `primary_real:0 primary_injected:0 reference:0 mouse:2705`. One thread, one
 * minute, the mouse hook on that same thread firing 2,705 times. Re-hooking
 * does not recover it; the cause is upstream of anything this process owns.
 *
 * The owner's requirement does not care: *with the Spaceadom dashboard
 * focused, holding Space must show the ring and Space+letter must launch —
 * exactly as elsewhere.*
 *
 * The dashboard PAGE does still receive ordinary `keydown` / `keyup` for those
 * keys — it is the focused window, and the WebView2 input path is not what was
 * lost. So the page becomes the witness the hook cannot be. This module runs
 * the same tap / hold / combo state machine the hook runs and feeds what it
 * sees to the engine through three commands (`own_window_space_down`,
 * `own_window_key`, `own_window_space_up`). From `engine::dispatch` onwards
 * there is exactly ONE code path: the ring, the cascade and the toast do not
 * know this happened.
 *
 * WHY THIS CANNOT DOUBLE-FIRE
 * ---------------------------
 * Four things have to fail at once before one press becomes two holds, and the
 * first is not code at all:
 *
 *  0. A healthy hook returns `LRESULT(1)` for Space-down and for every combo
 *     key, so those keystrokes never reach any window — INCLUDING OURS. The
 *     page receiving a Space `keydown` at all is therefore already evidence
 *     that the hook did not intercept it.
 *  1. Rust checks the foreground window is ours (`hook::own_window_*`).
 *  2. Rust drops the page's Space-down if the hook stamped one inside the last
 *     100 ms, or has a hold latched right now.
 *  3. Rust ignores a combo or a release unless the fallback itself accepted
 *     the matching Space-down. A hold belongs to one path for its whole life.
 *
 * THE TRADEOFF, STATED PLAINLY
 * ----------------------------
 * The hook SUPPRESSES Space-down and types a space itself on release; that is
 * what makes "tap Space always types a space" exact and repeat-free. A page
 * cannot suppress a key it has already been handed without deciding, at
 * keydown time, that this press is a hold — which is the one thing nobody can
 * know yet. So:
 *
 *  · In a text field the Space keydown is NOT prevented. The browser inserts
 *    the space immediately, and a TAP therefore behaves perfectly — the space
 *    is the browser's own, at the browser's own caret, with the browser's own
 *    undo entry.
 *  · If the press turns into a hold (past the ring's delay) or a combo, the
 *    inserted space (and any auto-repeat spaces behind it) is removed and
 *    further repeats are suppressed.
 *  · KNOWN DIVERGENCE FROM THE HOOK: the hook types a space when a long
 *    no-combo hold is released; this path does not, because it removed the one
 *    the browser had already put there. Holding Space inside a dashboard text
 *    field and letting go leaves no space. Deliberate — the alternative is
 *    re-inserting a character at a caret the user may have moved, which is a
 *    worse failure than a missing space.
 *  · Outside a text field the Space keydown IS prevented, because there the
 *    browser's "space" means *click the focused button* — holding Space with
 *    the gear focused would otherwise open Settings on the way to the ring.
 *
 * SHAPE
 * -----
 * The decision half is a PURE state machine (`stepOwnWindow`) with no DOM and
 * no IPC in it, because that is the half worth testing — see
 * `own-window-keys.test.ts`. The DOM half below it only translates events in
 * and effects out.
 */

import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// The pure state machine
// ---------------------------------------------------------------------------

/** Timings, mirrored from the same config values the hook and engine read. */
export interface OwnWindowTiming {
  /**
   * `config.rollover_ms` — the hook's typing-vs-command window. A letter that
   * lands within this long of Space going down is PROSE, not a shortcut
   * (PROBLEM 95/218). Matching it here is what stops a fast typist's "…a s…"
   * from launching Slack inside the dashboard.
   */
  rolloverMs: number;
  /**
   * `config.guide_hud_delay_ms` — when the ring appears, and therefore the
   * moment this press stops being a tap. The same number the engine uses to
   * schedule the HUD, so the space disappears exactly as the ring arrives.
   */
  holdThresholdMs: number;
}

export type OwnWindowPhase = "idle" | "holding" | "aborted";

export interface OwnWindowState {
  phase: OwnWindowPhase;
  /** Timestamp of the Space-down that opened this hold. */
  spaceDownAt: number;
  /** Spaces the browser has inserted for this hold and we have not removed. */
  insertedSpaces: number;
  /** Has this hold dispatched a combo? Reported on release. */
  comboFired: boolean;
  /**
   * Have we already taken the inserted spaces back? Once true, every further
   * Space auto-repeat is suppressed so no new ones appear.
   */
  spacesTakenBack: boolean;
}

export type OwnWindowInput =
  | {
      kind: "space-down";
      at: number;
      /** OS auto-repeat of a Space already held. */
      repeat: boolean;
      /** Ctrl / Alt / Win physically held — keyboard-hook law 4. */
      chord: boolean;
      /** An IME composition is in progress. */
      composing: boolean;
      /** The focused element takes typed text (input / textarea / editable). */
      inTextField: boolean;
    }
  | {
      kind: "key-down";
      at: number;
      /** Windows virtual-key code, or `null` for a key we never intercept. */
      vk: number | null;
      repeat: boolean;
      chord: boolean;
      composing: boolean;
    }
  | { kind: "space-up"; at: number }
  | { kind: "hold-threshold" }
  | { kind: "window-blur" };

export type OwnWindowEffect =
  /** Remember the focused element and caret, so the space can be found again. */
  | { kind: "capture-caret" }
  /** Stop the browser acting on this key. */
  | { kind: "prevent-default" }
  /** `invoke("own_window_space_down")`. */
  | { kind: "space-down" }
  /** `invoke("own_window_key", { vk })`. */
  | { kind: "combo"; vk: number }
  /** `invoke("own_window_space_up", { hadCombo })`. */
  | { kind: "space-up"; hadCombo: boolean }
  /** Delete `count` spaces this hold caused, ending at the caret. */
  | { kind: "take-back-spaces"; count: number }
  | { kind: "start-hold-timer"; ms: number }
  | { kind: "clear-hold-timer" };

export function initialOwnWindowState(): OwnWindowState {
  return {
    phase: "idle",
    spaceDownAt: 0,
    insertedSpaces: 0,
    comboFired: false,
    spacesTakenBack: false,
  };
}

/**
 * One input in, the next state and the effects to perform out. No DOM, no IPC,
 * no clock — every time comes in on the input, so a test can drive a whole
 * hold in a loop.
 */
export function stepOwnWindow(
  state: OwnWindowState,
  input: OwnWindowInput,
  timing: OwnWindowTiming,
): { state: OwnWindowState; effects: OwnWindowEffect[] } {
  const effects: OwnWindowEffect[] = [];
  const next: OwnWindowState = { ...state };

  switch (input.kind) {
    // -----------------------------------------------------------------------
    case "space-down": {
      // An IME composition owns the keyboard; never reach into one.
      if (input.composing) break;

      // Keyboard-hook law 4: Ctrl+Space / Alt+Space / Win+Space are real OS and
      // app shortcuts (IME switch, autocomplete, window menu). Never swallow
      // them, and never start a hold on one.
      if (input.chord) break;

      if (state.phase !== "idle") {
        // An auto-repeat of a Space we are already holding. The hook counts
        // these and suppresses them all; here they matter because each one the
        // browser handles inserts ANOTHER space.
        if (state.phase === "holding") {
          if (state.spacesTakenBack) {
            effects.push({ kind: "prevent-default" });
          } else if (input.inTextField) {
            next.insertedSpaces = state.insertedSpaces + 1;
          }
        }
        break;
      }

      next.phase = "holding";
      next.spaceDownAt = input.at;
      next.comboFired = false;
      next.spacesTakenBack = false;

      if (input.inTextField) {
        // Let the browser type it. A tap must be indistinguishable from a
        // normal space, and this is the only way to get that for free.
        next.insertedSpaces = 1;
        effects.push({ kind: "capture-caret" });
      } else {
        // Nothing to type here, and plenty to break: Space on a focused button
        // is a click. Suppress it outright.
        next.insertedSpaces = 0;
        next.spacesTakenBack = true;
        effects.push({ kind: "prevent-default" });
      }

      effects.push({ kind: "space-down" });
      effects.push({ kind: "start-hold-timer", ms: timing.holdThresholdMs });
      break;
    }

    // -----------------------------------------------------------------------
    case "key-down": {
      if (state.phase !== "holding") break;
      if (input.composing) break;
      // PROBLEM 176 — a real OS chord that overlaps a held Space must WIN.
      // Win+Shift+S is the worked example: it arrives as `S` with Win down,
      // and a fallback that read it as Space+S would launch an app instead of
      // taking a screenshot. Pass-through, not abort: the Space is still
      // genuinely held.
      if (input.chord) break;
      // A key this fallback never takes from the page (Escape, Enter, Tab,
      // arrows, digits, F-keys…). See `vkForOwnWindow` for the full list and
      // the reason for each.
      if (input.vk === null) break;
      // Auto-repeat of a combo key. The hook dispatches on every repeat; here
      // that would fire the same launch again, so only the first counts.
      if (input.repeat) break;

      const held = input.at - state.spaceDownAt;
      if (timing.rolloverMs > 0 && held < timing.rolloverMs) {
        // TYPING ROLLOVER (PROBLEM 95/218). This is prose, not a command: the
        // thumb had not left the spacebar yet. Let both characters through —
        // the browser has already inserted the space and is about to insert
        // this letter, which is exactly what the hook's `inject_space_then_key`
        // does — and end the hold so the ring never appears.
        next.phase = "aborted";
        effects.push({ kind: "clear-hold-timer" });
        effects.push({ kind: "space-up", hadCombo: false });
        break;
      }

      next.comboFired = true;
      effects.push({ kind: "clear-hold-timer" });
      effects.push({ kind: "prevent-default" });
      if (!state.spacesTakenBack && state.insertedSpaces > 0) {
        effects.push({ kind: "take-back-spaces", count: state.insertedSpaces });
      }
      next.spacesTakenBack = true;
      next.insertedSpaces = 0;
      effects.push({ kind: "combo", vk: input.vk });
      break;
    }

    // -----------------------------------------------------------------------
    case "hold-threshold": {
      if (state.phase !== "holding") break;
      // The press is a HOLD: the ring is arriving now. Take the space back
      // before the user sees it, and suppress the repeats still to come.
      if (!state.spacesTakenBack && state.insertedSpaces > 0) {
        effects.push({ kind: "take-back-spaces", count: state.insertedSpaces });
      }
      next.spacesTakenBack = true;
      next.insertedSpaces = 0;
      break;
    }

    // -----------------------------------------------------------------------
    case "space-up": {
      if (state.phase === "idle") break;
      effects.push({ kind: "clear-hold-timer" });
      if (state.phase === "holding") {
        // A TAP (before the threshold, no combo) leaves the browser's own
        // space exactly where it is — that is the whole point of not
        // preventing it. Rust's `SpaceUp` cancels the pending ring.
        effects.push({ kind: "space-up", hadCombo: state.comboFired });
      }
      // "aborted" already sent its release at the rollover.
      Object.assign(next, initialOwnWindowState());
      break;
    }

    // -----------------------------------------------------------------------
    case "window-blur": {
      // THE LEAK THIS CLOSES: the page stops receiving key events the instant
      // it loses focus, so a hold interrupted by an Alt+Tab would never get
      // its `keyup` — and the ring would stay on screen with nothing left able
      // to take it down (the same failure PROBLEM 218's reaper exists for, on
      // a path the reaper cannot see).
      if (state.phase === "holding") {
        effects.push({ kind: "clear-hold-timer" });
        effects.push({ kind: "space-up", hadCombo: state.comboFired });
      } else if (state.phase === "aborted") {
        effects.push({ kind: "clear-hold-timer" });
      }
      Object.assign(next, initialOwnWindowState());
      break;
    }
  }

  return { state: next, effects };
}

/**
 * `KeyboardEvent` → the Windows virtual-key code the hook would have seen, or
 * `null` for a key this fallback must never take away from the page.
 *
 * The map is the hook's, NARROWED, and every omission is a decision:
 *
 *  · Escape, Enter, Tab — how a user closes a popover, submits a profile name
 *    and moves between fields. A fallback that ate them would break the
 *    dashboard to add a shortcut.
 *  · Backspace, arrows, Right Alt — the same, for a caret. Space+⌫ is Force
 *    Close and Space+↑/↓ scroll; neither is worth swallowing an editing key.
 *  · F1–F12 — gated hook-side on `BOUND_SPECIALS`, which the page cannot read.
 *  · Digits — the hook's `vk_to_char` covers A–Z only, so Space+7 produces no
 *    event there either. Intercepting one would cost a keystroke and buy
 *    nothing.
 *
 * `key` is preferred over `code` on purpose: `key` is what the layout produced,
 * which is what the OS virtual-key code reflects, so a non-QWERTY keyboard maps
 * the same way here as it does in the hook. `code` is the fallback for the case
 * `key` cannot answer (a dead key, a composed letter).
 */
export function vkForOwnWindow(ev: {
  key: string;
  code: string;
}): number | null {
  const k = ev.key;
  if (k.length === 1) {
    const upper = k.toUpperCase();
    if (upper >= "A" && upper <= "Z") return upper.charCodeAt(0);
    if (k === "`" || k === "~") return 0xc0; // VK_OEM_3
    if (k === ",") return 0xbc; // VK_OEM_COMMA
    if (k === ".") return 0xbe; // VK_OEM_PERIOD
    return null;
  }
  // `key` gave a name, not a character (Dead, Unidentified, Process…). Fall
  // back to the physical position for the letters only.
  const c = ev.code;
  if (c.length === 4 && c.startsWith("Key")) {
    const ch = c.charAt(3);
    if (ch >= "A" && ch <= "Z") return ch.charCodeAt(0);
  }
  return null;
}

/** Does this element take typed text? */
export function isTextEntry(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "TEXTAREA") return true;
  if (tag === "INPUT") {
    const type = (el as HTMLInputElement).type.toLowerCase();
    // The types where a space is a character. `checkbox`/`radio`/`button`
    // treat Space as ACTIVATE, which is the button case, not the text case.
    return (
      type === "text" ||
      type === "search" ||
      type === "url" ||
      type === "email" ||
      type === "tel" ||
      type === "password"
    );
  }
  return (el as HTMLElement).isContentEditable === true;
}

// ---------------------------------------------------------------------------
// The DOM half — translation only, no decisions
// ---------------------------------------------------------------------------

let timing: OwnWindowTiming = { rolloverMs: 50, holdThresholdMs: 300 };
let machine: OwnWindowState = initialOwnWindowState();
let holdTimer: number | null = null;
let listening = false;

/** Where the browser's spaces went, captured at Space-down. */
let caretHost: HTMLElement | null = null;

/**
 * Update the timings from config. Called at boot and again whenever
 * `config-updated` lands, so a changed ring delay or typing speed takes effect
 * without a restart — the same as the hook, which is re-armed by Rust.
 */
export function setOwnWindowKeyTiming(next: Partial<OwnWindowTiming>): void {
  if (typeof next.rolloverMs === "number" && next.rolloverMs >= 0) {
    timing.rolloverMs = next.rolloverMs;
  }
  if (typeof next.holdThresholdMs === "number" && next.holdThresholdMs > 0) {
    timing.holdThresholdMs = next.holdThresholdMs;
  }
}

function clearHoldTimer(): void {
  if (holdTimer !== null) {
    window.clearTimeout(holdTimer);
    holdTimer = null;
  }
}

function takeBackSpaces(count: number): void {
  const el = caretHost;
  if (!el || !el.isConnected || count <= 0) return;
  const wanted = " ".repeat(count);

  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    const end = el.selectionStart;
    if (end === null || end < count) return;
    const start = end - count;
    if (el.value.slice(start, end) !== wanted) return;
    // `setRangeText` keeps the browser's own undo stack coherent, which a
    // wholesale `value =` assignment does not.
    el.setRangeText("", start, end, "end");
    // The dashboard's inputs are wired to `input`, not to `keyup` — without
    // this the field's own listeners never learn the value changed.
    el.dispatchEvent(new Event("input", { bubbles: true }));
    return;
  }

  // contenteditable. Nothing in the dashboard is one today, so this branch is
  // written to be safe rather than clever: it only ever deletes characters it
  // has confirmed are the spaces it put there.
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0 || !sel.isCollapsed) return;
  const node = sel.anchorNode;
  if (!node || node.nodeType !== Node.TEXT_NODE) return;
  const text = node as Text;
  const offset = sel.anchorOffset;
  if (offset < count) return;
  if (text.data.slice(offset - count, offset) !== wanted) return;
  text.deleteData(offset - count, count);
  const range = document.createRange();
  range.setStart(text, offset - count);
  range.collapse(true);
  sel.removeAllRanges();
  sel.addRange(range);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

function run(effects: OwnWindowEffect[], ev: KeyboardEvent | null): void {
  for (const eff of effects) {
    switch (eff.kind) {
      case "prevent-default":
        ev?.preventDefault();
        break;
      case "capture-caret": {
        const active = document.activeElement;
        caretHost = active instanceof HTMLElement ? active : null;
        break;
      }
      case "take-back-spaces":
        takeBackSpaces(eff.count);
        break;
      case "start-hold-timer":
        clearHoldTimer();
        holdTimer = window.setTimeout(() => {
          holdTimer = null;
          const out = stepOwnWindow(machine, { kind: "hold-threshold" }, timing);
          machine = out.state;
          run(out.effects, null);
        }, eff.ms);
        break;
      case "clear-hold-timer":
        clearHoldTimer();
        break;
      case "space-down":
        void invoke<boolean>("own_window_space_down").catch(() => false);
        break;
      case "combo":
        void invoke<boolean>("own_window_key", { vk: eff.vk }).catch(() => false);
        break;
      case "space-up":
        void invoke<boolean>("own_window_space_up", {
          hadCombo: eff.hadCombo,
        }).catch(() => false);
        break;
    }
  }
}

function feed(input: OwnWindowInput, ev: KeyboardEvent | null): void {
  const out = stepOwnWindow(machine, input, timing);
  machine = out.state;
  run(out.effects, ev);
}

function onKeyDown(ev: KeyboardEvent): void {
  // `isComposing` is the standard flag; keyCode 229 is what WebView2 reports
  // for a key consumed by the IME on older paths. Both, because a fallback
  // that reaches into a composition is worse than a fallback that misses one.
  const composing = ev.isComposing === true || ev.keyCode === 229;
  const chord = ev.ctrlKey || ev.altKey || ev.metaKey;

  if (ev.key === " " || ev.code === "Space") {
    feed(
      {
        kind: "space-down",
        at: ev.timeStamp,
        repeat: ev.repeat,
        chord,
        composing,
        inTextField: isTextEntry(document.activeElement),
      },
      ev,
    );
    return;
  }

  feed(
    {
      kind: "key-down",
      at: ev.timeStamp,
      vk: vkForOwnWindow(ev),
      repeat: ev.repeat,
      chord,
      composing,
    },
    ev,
  );
}

function onKeyUp(ev: KeyboardEvent): void {
  if (ev.key !== " " && ev.code !== "Space") return;
  feed({ kind: "space-up", at: ev.timeStamp }, ev);
}

function attach(): void {
  if (listening) return;
  listening = true;
  // CAPTURE. The dashboard's own components listen on the bubble phase (the
  // settings panel's keyboard operability, PROBLEM 255), and a combo must be
  // decided before any of them sees it.
  document.addEventListener("keydown", onKeyDown, true);
  document.addEventListener("keyup", onKeyUp, true);
}

function detach(): void {
  if (!listening) return;
  listening = false;
  document.removeEventListener("keydown", onKeyDown, true);
  document.removeEventListener("keyup", onKeyUp, true);
  // Release any hold this window was holding — see the `window-blur` arm.
  feed({ kind: "window-blur" }, null);
  caretHost = null;
}

/**
 * Wire the fallback. Idempotent, and safe on any build: if the backend does not
 * have the three commands (an older Rust half), every `invoke` rejects and the
 * `.catch` swallows it — the page keeps behaving exactly as it did before.
 */
export function initOwnWindowKeys(t?: Partial<OwnWindowTiming>): void {
  if (t) setOwnWindowKeyTiming(t);
  window.addEventListener("focus", attach);
  window.addEventListener("blur", detach);
  if (document.hasFocus()) attach();
}

/** Test seam: reset module state between harness runs. */
export function __resetOwnWindowKeysForTest(): void {
  clearHoldTimer();
  machine = initialOwnWindowState();
  caretHost = null;
  timing = { rolloverMs: 50, holdThresholdMs: 300 };
}
