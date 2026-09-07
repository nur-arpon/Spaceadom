/**
 * tour.ts — the first-run "Guided first bind" walkthrough.
 *
 * Four beats and one conditional detour, and nothing else:
 *
 *   entry   the two-line promise ("Hold Space, tap any app's initial letter…")
 *   step 1  pick a letter          — advances when the key editor opens
 *   step 2  bind it                — advances on a real save
 *   step 2b pick a profile         — ONLY when the save opened the browser-
 *                                    profile picker; advances when it closes
 *   step 3  use it                 — advances on a REAL launch from the engine
 *   done    "That's it — you're set."  → config.tour_done = true
 *
 * STEP 2b EXISTS BECAUSE THE TOUR OVERTOOK THE APP (PROBLEM 242 follow-up,
 * 2026-09-06). The owner bound K to Brave in the walkthrough; Brave has two
 * profiles, so `key-detail-panel.ts::openProfilePage` put "Which Brave
 * profile?" on screen — and the tour was already sitting UNDERNEATH it saying
 * "Now hold Space and tap Space+K — try it." In his words: *"The walkthrough
 * wasn't intelligent enough to understand."* His original spec for the tour
 * had asked for a "browser-profile step if browser"; this is it.
 *
 * The ordering that caused it is worth stating, because it is the reason 2b
 * accepts an entry from step 3 and not only from step 2: `commit()` reports
 * the save to `tourBindingSaved` BEFORE it runs `opts.onSaved`, and
 * `onSaved` is what opens the picker. So the tour is ALREADY in step 3 by the
 * time the picker exists. Rather than reorder a save path that seven call
 * sites depend on, 2b simply takes the tour back — in the same tick, before a
 * frame is painted, so step 3 is never seen.
 *
 * THREE RULES THIS MODULE EXISTS TO KEEP, all of them learned the hard way
 * elsewhere in this app:
 *
 * 1. **It never listens to the keyboard.** The Win32 hook owns Space
 *    system-wide and swallows it, so a page-level keydown listener could not
 *    see the combo even if it were allowed to try. Step 3 therefore advances
 *    on the `st-launched` event Rust emits after a cascade actually succeeds
 *    (`engine/mod.rs::handle_alpha`) — the only thing in the process that
 *    knows a launch really happened.
 *
 * 2. **It never touches the DOM it points at.** Highlights are free-floating
 *    ring elements inside this module's own layer, positioned from the
 *    targets' `getBoundingClientRect()` on a rAF loop. The key editor rebuilds
 *    its whole `innerHTML` on every `renderPanel`, and `updateMatrix` re-skins
 *    every key cell — a class parked on either would be silently swept away.
 *    Measuring instead of marking is what makes the tour survive a re-render.
 *
 * 3. **It is a LEAF module** (PROBLEM 148): it imports nothing from `main.ts`,
 *    so `preview.ts` can render it. Everything it needs from the config
 *    arrives through the `TourHost` it is initialised with.
 *
 * The layer sits at z-index 8800 — above every dashboard surface (the highest
 * is 90) and deliberately BELOW `.confirm-back`'s 9000, so a destructive
 * confirmation is never buried under a walkthrough card. It is a DASHBOARD
 * layer only; the overlay window has its own page and never loads this.
 */
import { listen } from "@tauri-apps/api/event";

// ---------------------------------------------------------------------------
// Copy — verbatim, including the spacing and the " !".
// `.tour-line` is `white-space: pre-wrap`, so the double space in ENTRY_A
// survives; in plain HTML it would collapse to one.
// ---------------------------------------------------------------------------

const ENTRY_A = "Hold Space, tap any app's initial  letter — boom ! it opens.";
const ENTRY_B = "Hold Space, tap the app's initial again — boom ! it's gone.";
const STEP1 = "Click any letter to give it a job.";
const STEP1_BOUND = "This one's already set — pick an empty letter, or change this one.";
const STEP1_PAUSED = "No rush — click the letter again when ready.";
const STEP2 = "Paste a link, choose a file, or pick an app — that's what your letter will open.";
/**
 * Step 2b, the browser-profile beat. Two shapes, because the picker has two:
 * ONE named browser (the app-grid flow — "Which Brave profile?"), or the whole
 * list (a URL that has not chosen a browser yet — "Open … in…").
 *
 * The named copy is the owner's, verbatim, and it deliberately mirrors the
 * picker's own footer hint ("Skip this and Space + K opens Brave the way it
 * always has") so the card and the thing it is pointing at say the same words.
 */
const step2bNamed = (browser: string): string =>
  `${browser} has more than one profile. Pick the one this key should open — ` +
  `or skip, and it opens ${browser} the way it always has.`;
const STEP2B_ANY =
  "Pick which browser — and which profile — this key should open. " +
  "Or skip, and it opens in your default browser.";
const STEP3_HEAD = "Now hold Space and tap ";
const STEP3_TAIL = " — try it.";
const DONE = "That's it — you're set.";

// ---------------------------------------------------------------------------
// Host + state
// ---------------------------------------------------------------------------

/**
 * The two things the tour needs from whoever owns the config. Kept this small
 * on purpose: it is the whole reason the module can be a leaf, and the reason
 * `preview.ts` can drive the tour against a stub config with two lambdas.
 */
export interface TourHost {
  /** Has the tour already been completed or skipped? (`config.tour_done`) */
  isDone(): boolean;
  /** Write `tour_done = true` and persist. Called once, on finish or skip. */
  setDone(): void;
}

type Phase = "off" | "entry" | "step1" | "step2" | "step2b" | "step3" | "done";

let _host: TourHost | null = null;
let _phase: Phase = "off";
/** Set when step 2 was reached by clicking a letter that was already bound. */
let _openedBound = false;
/** Set when the editor was closed without saving — step 1 with the softer copy. */
let _paused = false;
/** The letter the user actually bound, for step 3. */
let _key = "";
/** What they bound to it — shown on the step 3 chip. */
let _label = "";
/**
 * The account label on the binding that was last saved, or "" for none.
 *
 * READ OFF THE SAVED BINDING (`commit()` passes `full.browser_profile_name`),
 * never remembered from the tile the user pressed. That is what stops it going
 * stale: re-bind the same key to something with no profile during step 3 and
 * the next save reports null, so the step-3 note stops naming a profile the
 * key no longer opens. A value latched at pick-time would have survived that.
 */
let _profile = "";
/**
 * Was the user actually ASKED which profile to use — i.e. has step 2b been on
 * screen since the editor was last opened?
 *
 * `_profile` alone cannot answer it, because the app writes a profile the user
 * never chose: a browser with exactly ONE profile has its directory and label
 * stored on the binding so the launch is explicit rather than relying on
 * Chromium's last-used. Step 3 naming "Chrome (Person 1)" after a bind that
 * asked nothing would be reporting a decision back to someone who never made
 * one, so the parenthesis needs both halves.
 */
let _askedProfile = false;
/**
 * The browser named on the picker's header while step 2b is up, or "" when the
 * picker is listing every browser. Only the copy reads it.
 */
let _pickerBrowser = "";
/**
 * Has a binding genuinely landed during THIS run of the tour?
 *
 * It exists for one question the state machine could not otherwise answer: the
 * editor has closed while we are in step 2 — is that someone walking away
 * (→ step 1, softer copy) or someone who has already bound the key and simply
 * backed out of the profile picker with ← (→ step 3)? Before step 2b there was
 * no way to be in step 2 after a save, so "close in step 2 = abandonment" was
 * exact. The ← path makes it inexact, and this is what restores it.
 */
let _bound = false;
let _listenerWired = false;

// ---- the layer ----
let _layer: HTMLElement | null = null;
let _ringHost: HTMLElement | null = null;
let _card: HTMLElement | null = null;
let _skipPill: HTMLButtonElement | null = null;

/** One entry per highlight the current step wants. Re-resolved every frame. */
interface RingTarget {
  /** CSS selector, re-queried each frame — the target may be re-rendered. */
  sel: string;
  /** Corner radius in px, matched to whatever it is ringing. */
  radius: number;
  /** How far outside the target's box the ring sits. */
  pad: number;
  /** Breathe (motion allowed) or sit still (`:root.reduced-motion`). */
  pulse: boolean;
}
let _targets: RingTarget[] = [];
let _raf = 0;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/** Wire the tour once, at bootstrap. Safe to call twice; the second is a no-op. */
export function initTour(host: TourHost): void {
  _host = host;
  if (_listenerWired) return;
  _listenerWired = true;

  // The ONLY input the tour ever takes from the engine. `st-launched` is
  // emitted by Rust after `smart_cascade` reports anything other than
  // `Failed`, so a key that did not actually open anything never advances
  // step 3 — which is exactly the "wrong key / Space alone: nothing happens,
  // no error, it waits" behaviour the step is specified to have.
  void listen<{ key: string; label: string }>("st-launched", (e) => {
    if (_phase !== "step3") return;
    const k = (e.payload?.key ?? "").toLowerCase();
    if (!k || k !== _key) return;   // wrong key: wait, silently
    finish();
  }).catch(() => { /* no backend (preview without the bus) — the tour just waits */ });
}

/** First-run entry point: shows the card unless the user is already done. */
export function maybeStartTour(): void {
  if (!_host || _host.isDone()) return;
  if (_phase !== "off") return;
  go("entry");
}

/**
 * Re-entry from Settings → "Show me the walkthrough". Restarts at STEP 1
 * regardless of `tour_done`: someone who asks for the walkthrough wants the
 * doing part, not the pitch they have already read.
 */
export function startTour(): void {
  // REVIEW FIX 2026-09-04 — a teardown may still be pending. `exit()` sets
  // `_phase = "off"` immediately but removes the layer 210 ms later; opening the
  // walkthrough again inside that window let `ensureLayer()` reuse the DYING
  // layer, and the old timer then ripped the freshly-started tour out of the
  // DOM. Finish the pending teardown NOW so `ensureLayer()` builds a clean one.
  cancelPendingExit();
  _paused = false;
  _openedBound = false;
  _key = "";
  _label = "";
  _profile = "";
  _pickerBrowser = "";
  _askedProfile = false;
  _bound = false;
  go("step1");
}

export function isTourActive(): boolean {
  return _phase !== "off";
}

/**
 * The key editor opened. Called from `key-detail-panel.ts::openPanel`, which
 * is the one door both the real dashboard and the preview harness go through.
 */
export function tourEditorOpened(key: string, alreadyBound: boolean): void {
  // BEFORE the phase guard, deliberately. Opening the editor is a fresh bind
  // gesture whatever step we are on, and nothing has been asked yet in it — so
  // a step-3 re-bind to a one-profile browser cannot inherit "they were asked"
  // from the picker that ran the last time round.
  _askedProfile = false;
  if (_phase !== "step1") return;
  _key = key.toLowerCase();
  _openedBound = alreadyBound;
  _paused = false;
  // A fresh trip through the editor describes a fresh binding. Carrying the
  // previous one's label or profile forward would let step 3 name a browser
  // profile that belongs to a key the user has since moved on from.
  _label = "";
  _profile = "";
  go("step2");
}

/**
 * A binding was genuinely committed. Called from `key-detail-panel.ts::commit`
 * immediately after `_onSave`, i.e. past both of that function's early exits —
 * so this cannot fire for a save that did not happen.
 */
export function tourBindingSaved(key: string, label: string, profile: string | null): void {
  const k = key.toLowerCase();

  // A LATER save to the SAME key, while step 3 is already up or the picker is
  // still open. This is the profile pick finishing (`commitProfile` re-commits
  // the whole binding with the pin attached) and it is also a re-bind of the
  // key mid-walkthrough. Neither changes the step; both change what the step
  // is describing, and step 3's note has to follow or it names the previous
  // target forever.
  if ((_phase === "step3" || _phase === "step2b") && k === _key) {
    _label = label;
    _profile = profile ?? "";
    if (_phase === "step3") render();
    return;
  }

  if (_phase !== "step2") return;
  _key = k;
  _label = label;
  _profile = profile ?? "";
  _bound = true;
  go("step3");
}

/**
 * The browser-profile picker just opened over the editor (PROBLEM 242
 * follow-up). Called from `key-detail-panel.ts::openProfilePage`, which is the
 * ONE place page 2 is ever built — the app-grid tile, the chip, the 4b disc and
 * the replace-confirm branch all funnel through it, so a hook there covers
 * every door including the ones added later.
 *
 * `browser` is the browser being asked about, or null when the picker is
 * listing every detected browser (a URL that has not chosen one yet).
 *
 * ACCEPTED FROM STEP 3, NOT ONLY STEP 2, and that is not defensive coding — it
 * is the normal path. See the ordering note in the file header: `commit()`
 * reports the save before it opens the picker, so step 3 is where we always
 * are when this arrives from the app-grid flow.
 */
export function tourProfilePickerOpened(key: string, browser: string | null): void {
  if (_phase !== "step2" && _phase !== "step2b" && _phase !== "step3") return;
  // A picker for some OTHER key is not part of this walkthrough's story, and
  // 2b's copy ("…this key should open") would be a lie about it.
  if (key.toLowerCase() !== _key) return;
  _pickerBrowser = browser ?? "";
  _askedProfile = true;
  go("step2b");
}

/**
 * The picker closed — by a pick, by the skip/reset row, by Done, by ✕, or by
 * ← back to the app grid.
 *
 * IT TAKES NO PROFILE NAME, ON PURPOSE. Every exit that CHOOSES something goes
 * on to re-commit the binding, so `tourBindingSaved` above hears the answer
 * from `commit()` — the binding that was actually written — and the exits that
 * choose nothing leave the previous value standing, which is the truth for
 * them. Passing the pressed tile's label in here as well would be a second
 * source for one fact, and the second source is the one that goes stale.
 *
 * `backToEditor` is the ← case ONLY: the picker went away but the editor did
 * not, so the honest place to stand is step 2, in front of the editor the user
 * is now looking at — not step 3, telling them to press a key while a dialog
 * is still open. That is the same mistake in a smaller box.
 */
export function tourProfilePickerClosed(backToEditor = false): void {
  if (_phase !== "step2b") return;
  _pickerBrowser = "";
  // `_bound` guards the one path that reaches the picker WITHOUT a save: the
  // 4b disc pressed on an unedited pill value commits nothing on purpose
  // ("it is ALREADY committed"), so there is no new binding for step 3 to
  // point at and the tour belongs back in the editor.
  if (backToEditor || !_bound) { go("step2"); return; }
  go("step3");
}

/**
 * The editor closed. Fires for BOTH "closed after saving" and "closed without
 * saving" — `closePanel` cannot tell them apart and should not try. The
 * distinction is made here instead: a save has already moved us to step 3, so
 * only a close that arrives while we are still in step 2 is an abandonment.
 *
 * Two amendments since step 2b exists:
 *
 * · From 2b this is a picker dismissal the picker itself never reported —
 *   Esc, or a click on the backdrop, both of which close the whole panel.
 *   Route it through the same door as ✕ so those two paths cannot drift.
 * · In step 2, "closed" no longer implies "never bound": ← from the picker
 *   lands back here with the binding already written. `_bound` is what tells
 *   the two apart, and getting it wrong would nag a user who has finished.
 */
export function tourEditorClosed(): void {
  if (_phase === "step2b") { tourProfilePickerClosed(); return; }
  if (_phase !== "step2") return;
  if (_bound) { go("step3"); return; }
  _paused = true;
  go("step1");
}

// ---------------------------------------------------------------------------
// The state machine
// ---------------------------------------------------------------------------

function go(next: Phase): void {
  _phase = next;
  if (next === "off") { teardown(); return; }
  ensureLayer();
  render();
}

/** Skip: stop now, and never ask again. */
function skip(): void {
  _host?.setDone();
  exit();
}

/** Step 3 satisfied — say so, then stand down. */
function finish(): void {
  _host?.setDone();
  go("done");
  // Long enough to read six words, short enough not to sit there. The Done
  // button dismisses it sooner.
  window.setTimeout(() => { if (_phase === "done") exit(); }, 3200);
}

/**
 * The pending 210 ms teardown from `exit()`, if one is in flight. `_phase` is
 * already `"off"` by then, so this handle is the only thing that knows the tour
 * is still EXITING rather than gone — which is what `startTour()` has to ask.
 */
let _exitTimer = 0;

/**
 * Finish an in-flight exit immediately. Safe to call when there is none.
 * Doing the removal here rather than merely cancelling the timer matters:
 * the layer would otherwise stay in the DOM with `tour-leaving` on it, and
 * `ensureLayer()` (which only checks `isConnected`) would adopt it.
 */
function cancelPendingExit(): void {
  if (!_exitTimer) return;
  window.clearTimeout(_exitTimer);
  _exitTimer = 0;
  teardown();
}

/** Play the exit, then remove everything. ~65% of the entrance (320 → 200ms). */
function exit(): void {
  _phase = "off";
  _targets = [];
  document.body.classList.remove("tour-on", "tour-dim-board", "tour-dim-editor");
  if (!_layer) return;
  _layer.classList.add("tour-leaving");
  const dying = _layer;
  if (_exitTimer) window.clearTimeout(_exitTimer);
  _exitTimer = window.setTimeout(() => {
    _exitTimer = 0;
    if (dying.parentNode) dying.parentNode.removeChild(dying);
    if (_layer === dying) teardown();
  }, 210);
}

function teardown(): void {
  if (_raf) { cancelAnimationFrame(_raf); _raf = 0; }
  if (_layer?.parentNode) _layer.parentNode.removeChild(_layer);
  _layer = null;
  _ringHost = null;
  _card = null;
  _skipPill = null;
  _targets = [];
  document.body.classList.remove("tour-on", "tour-dim-board", "tour-dim-editor");
}

// ---------------------------------------------------------------------------
// The layer
// ---------------------------------------------------------------------------

function ensureLayer(): void {
  if (_layer?.isConnected) return;

  _layer = document.createElement("div");
  _layer.id = "tour-layer";
  // Not `role="dialog"`: this never traps focus and never blocks the app. The
  // card announces itself politely and the page underneath stays usable.
  _layer.setAttribute("aria-live", "polite");

  _ringHost = document.createElement("div");
  _ringHost.className = "tour-rings";
  _layer.appendChild(_ringHost);

  _skipPill = document.createElement("button");
  _skipPill.type = "button";
  _skipPill.id = "tour-skip";
  _skipPill.className = "tour-skip";
  _skipPill.textContent = "Skip";
  _skipPill.addEventListener("click", skip);
  _layer.appendChild(_skipPill);

  _card = document.createElement("div");
  _card.className = "tour-card";
  _layer.appendChild(_card);

  // Body-level, NOT inside #stage: the stage is a positioned, z-indexed
  // container, and anything parked inside it inherits the dimming this tour
  // applies to the stage's own children.
  document.body.appendChild(_layer);
  document.body.classList.add("tour-on");
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function render(): void {
  if (!_card || !_layer) return;

  _card.className = "tour-card";
  _card.replaceChildren();
  const dim = document.body.classList;
  dim.remove("tour-dim-board", "tour-dim-editor");
  if (_skipPill) _skipPill.hidden = _phase === "entry" || _phase === "done";

  switch (_phase) {
    case "entry":
      _card.classList.add("tour-hero");
      _card.appendChild(line(ENTRY_A));
      _card.appendChild(line(ENTRY_B));
      _card.appendChild(actions([
        { text: "Show me", primary: true, run: () => { _paused = false; go("step1"); } },
        { text: "Skip", primary: false, run: skip },
      ]));
      setTargets([]);
      break;

    case "step1":
      dim.add("tour-dim-board");
      _card.appendChild(line(_paused ? STEP1_PAUSED : STEP1));
      // 2–3 empty letters, so "any letter" has somewhere obvious to land.
      setTargets(pickFreeLetters(3).map((k) => ({
        sel: `#keyboard-matrix .key[data-key="${cssKey(k)}"]`,
        radius: 16, pad: 3, pulse: true,
      })));
      break;

    case "step2": {
      dim.add("tour-dim-editor");
      if (_openedBound) _card.appendChild(note(STEP1_BOUND));
      _card.appendChild(line(STEP2));
      // The editor's three ways in, in the order the copy names them.
      setTargets([
        { sel: "#key-detail-panel .ed-path-wrap", radius: 999, pad: 3, pulse: false },
        { sel: "#key-detail-panel #ed-browse", radius: 999, pad: 3, pulse: false },
        { sel: "#key-detail-panel #ed-grid-scroll", radius: 16, pad: 4, pulse: false },
      ]);
      break;
    }

    case "step2b": {
      // The SAME dim as step 2, deliberately, and it needs no variant of its
      // own: `.bp-page` is `position:absolute; inset:0` on an opaque
      // background inside `#key-detail-panel`, so "keep the panel lit" and
      // "keep the picker lit" are the same rule while page 2 is up. A second
      // class with an identical selector would be two things to keep in step.
      dim.add("tour-dim-editor");
      _card.appendChild(line(
        _pickerBrowser ? step2bNamed(_pickerBrowser) : STEP2B_ANY,
      ));
      // The tiles only. NOT the Done button, which is the other half of the
      // copy: the panel is centred and the card sits at `bottom: 64px`, so on
      // a short viewport the card covers the picker's footer — a ring under
      // an opaque card reads as a rendering fault. The picker's own footer
      // hint already says what Done does, in the same words.
      setTargets([
        { sel: "#key-detail-panel .bp-scroll", radius: 16, pad: 4, pulse: false },
      ]);
      break;
    }

    case "step3": {
      // OWNER DECISION: the dashboard un-dims here so the Guide HUD's ring can
      // appear naturally when they hold Space. That ring IS the discovery
      // moment — do not suppress it, and do not dim over it.
      const p = document.createElement("p");
      p.className = "tour-line";
      p.appendChild(document.createTextNode(STEP3_HEAD));
      p.appendChild(combo(_key));
      p.appendChild(document.createTextNode(STEP3_TAIL));
      _card.appendChild(p);
      // The note reflects the choice that was actually just made. A user who
      // picked a profile in step 2b gets it named back to them — "Space+K
      // opens Brave (Arpon)." — and one who skipped gets the plain sentence,
      // which is the truth for them: it opens Brave the way it always has.
      //
      // BOTH halves are required. `_profile` alone is not enough (a
      // one-profile browser stores a name nobody chose) and `_askedProfile`
      // alone is not enough (they were asked and said no). Together they mean
      // "there is a choice, and the user made it".
      if (_label) {
        const target = _askedProfile && _profile ? `${_label} (${_profile})` : _label;
        _card.appendChild(note(`Space+${_key.toUpperCase()} opens ${target}.`));
      }
      setTargets([]);
      // A tour button holding focus is a button that Space would press. The
      // hook swallows Space long before the page sees it, so this is belt and
      // braces — but step 3 is the one step where the user is about to hold
      // Space, and this costs nothing.
      const active = document.activeElement as HTMLElement | null;
      if (active && _layer.contains(active)) active.blur();
      break;
    }

    case "done":
      _card.classList.add("tour-hero", "tour-done");
      _card.appendChild(line(DONE));
      _card.appendChild(actions([
        { text: "Done", primary: true, run: exit },
      ]));
      setTargets([]);
      break;
  }
}

function line(text: string): HTMLElement {
  const el = document.createElement("p");
  el.className = "tour-line";
  el.textContent = text;   // textContent + pre-wrap = the copy, exactly as written
  return el;
}

function note(text: string): HTMLElement {
  const el = document.createElement("p");
  el.className = "tour-note";
  el.textContent = text;
  return el;
}

/** The small Space+X glyph step 3 renders in place of the letter. */
function combo(key: string): HTMLElement {
  const wrap = document.createElement("span");
  wrap.className = "tour-combo";
  const a = document.createElement("span");
  a.className = "tour-cap";
  a.textContent = "Space";
  const plus = document.createElement("span");
  plus.className = "tour-plus";
  plus.textContent = "+";
  const b = document.createElement("span");
  b.className = "tour-cap";
  b.textContent = key.toUpperCase();
  wrap.append(a, plus, b);
  return wrap;
}

interface Action { text: string; primary: boolean; run: () => void }

function actions(list: Action[]): HTMLElement {
  const row = document.createElement("div");
  row.className = "tour-actions";
  for (const a of list) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "tour-btn" + (a.primary ? " tour-btn-primary" : "");
    b.textContent = a.text;
    b.addEventListener("click", a.run);
    row.appendChild(b);
  }
  return row;
}

// ---------------------------------------------------------------------------
// Rings
// ---------------------------------------------------------------------------

function setTargets(list: RingTarget[]): void {
  _targets = list;
  if (!_ringHost) return;

  // Pool the elements rather than rebuilding them: a ring that survives from
  // one step to the next keeps its opacity transition instead of flashing.
  while (_ringHost.children.length > list.length) {
    _ringHost.removeChild(_ringHost.lastChild!);
  }
  while (_ringHost.children.length < list.length) {
    const r = document.createElement("div");
    r.className = "tour-ring";
    _ringHost.appendChild(r);
  }
  list.forEach((t, i) => {
    const el = _ringHost!.children[i] as HTMLElement;
    el.className = "tour-ring" + (t.pulse ? " pulse" : "");
    el.style.borderRadius = `${t.radius}px`;
  });

  // Place them NOW, synchronously, before the loop is even scheduled: a ring
  // that waits for the first animation frame is a ring drawn at 0,0 for one
  // paint, and rAF is not guaranteed to be prompt — a throttled or occluded
  // webview can withhold frames indefinitely while still compositing what it
  // already has. Measured 2026-09-04 in the Vite harness, where rAF fired
  // ZERO times until a screenshot forced a paint.
  //
  // It is NOT a replacement for the loop, and the same measurement is why:
  // when the tour starts before the board has been laid out, `wireKeyboardFit`
  // has not run yet, `#keyboard-scale` still carries a near-zero scale, and a
  // key cell measures 1.78px — under the guard in `place()`, so the ring stays
  // off. One frame later the fit lands and the loop puts it right. Sync-place
  // gets it right when the layout already IS settled; the loop is what covers
  // when it is not.
  place();

  if (list.length && !_raf) _raf = requestAnimationFrame(tick);
  if (!list.length && _raf) { cancelAnimationFrame(_raf); _raf = 0; }
}

/**
 * Re-measure every frame. Cheap (at most three `getBoundingClientRect` calls)
 * and it is what makes the rings immune to a re-render, a window resize, the
 * board's fit-to-window rescale, and the editor's bloom animation all at once
 * — none of which fire an event this module could otherwise hook.
 */
function tick(): void {
  _raf = 0;
  if (!_ringHost || !_targets.length) return;
  place();
  _raf = requestAnimationFrame(tick);
}

/** One measure-and-move pass. A ring whose target is gone simply fades out. */
function place(): void {
  if (!_ringHost) return;
  _targets.forEach((t, i) => {
    const el = _ringHost!.children[i] as HTMLElement | undefined;
    if (!el) return;
    const target = document.querySelector(t.sel);
    const r = target?.getBoundingClientRect();
    if (!r || r.width < 2 || r.height < 2) { el.classList.remove("on"); return; }
    el.style.left = `${r.left - t.pad}px`;
    el.style.top = `${r.top - t.pad}px`;
    el.style.width = `${r.width + t.pad * 2}px`;
    el.style.height = `${r.height + t.pad * 2}px`;
    el.classList.add("on");
  });
}

/**
 * Up to `n` letters with nothing bound to them, read from the BOARD rather
 * than the config — `.key.bindable:not(.bound)` is the same predicate
 * `applyKeyState` paints with, so the highlight can never disagree with what
 * the user is looking at. The home-row preference is cosmetic: three lit keys
 * spread across the middle of the board read as "any of these", where the
 * first three of the alphabet read as an instruction.
 */
function pickFreeLetters(n: number): string[] {
  const free = Array.from(
    document.querySelectorAll<HTMLElement>("#keyboard-matrix .key.bindable:not(.bound)"),
  ).map((el) => el.dataset.key ?? "").filter(Boolean);
  if (!free.length) return [];

  const preferred = ["s", "d", "f", "j", "k", "l", "g", "h", "e", "r"];
  const picked: string[] = [];
  for (const k of preferred) {
    if (picked.length >= n) break;
    if (free.includes(k)) picked.push(k);
  }
  for (const k of free) {
    if (picked.length >= n) break;
    if (!picked.includes(k)) picked.push(k);
  }
  return picked;
}

/** Letters only reach this, but a selector built from data must still be safe. */
function cssKey(k: string): string {
  return k.replace(/[^a-z0-9_-]/gi, "");
}
