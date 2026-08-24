/**
 * dismissable.ts — ONE rule, applied once: anything that pops up closes when
 * you press somewhere else.
 *
 * WHY THIS EXISTS. The owner, 2026-08-24:
 *
 *   *"Could you make a rule of like when a bubble comes up after pressing
 *   something, if I press somewhere else it closes. For example I had to
 *   specifically tell you that when I press the settings then the settings
 *   panel arrives and I press somewhere else in the screen, it doesn't
 *   automatically close… Even now, when you introduced the conflicts popup on
 *   the dashboard, pressing somewhere else in the screen doesn't close it."*
 *
 * He is describing the same bug arriving twice, and he is right about the
 * cause. The dashboard's dismissal used to live in ONE function,
 * `closeAllPopovers()` in main.ts, which named each surface by id:
 *
 *     const pop = document.getElementById("profile-popover");   // profile
 *     const tray = document.getElementById("specials-tray");    // specials
 *     closeSettingsPanel();                                     // settings
 *
 * That works, and it is invisible to anyone adding a fifth surface in a
 * different file. `conflict-prompt.ts` even remembered the HALF of the
 * contract that is locally obvious — it stops its own clicks propagating so it
 * survives them — and could not have known about the half that lives in
 * another module. So it appears, and nothing ever closes it.
 *
 * **The failure mode is the design, not the author.** A rule that has to be
 * remembered in a second file will be missed exactly when someone is
 * concentrating on the first one. So dismissal is now a REGISTRY: a surface
 * registers itself at the moment it is shown, in its own file, on the line
 * that shows it. Forgetting is still possible, but it now looks like a missing
 * call beside the code you are writing rather than a missing line in a file
 * you have never opened.
 *
 * WHAT A CALLER GETS, in one call:
 *   - clicks INSIDE the surface no longer close it (the stopPropagation half),
 *   - a press anywhere else closes it,
 *   - Escape closes the most recently opened surface first,
 *   - and an unregister function, so a surface that closes itself does not
 *     leave a dead entry behind.
 *
 * WHAT IT DELIBERATELY DOES NOT COVER: persistent status banners
 * (`#conflict-banner`, the dead-hook warning). Those are not "bubbles" — they
 * report a condition that is still true, and dismissing them on an unrelated
 * click would hide a real fault. They carry their own explicit controls.
 */

interface Entry {
  el: HTMLElement;
  close: () => void;
  /** performance.now() at registration — see `armedAt` below. */
  armedAt: number;
  /** Higher = opened later. Escape closes the highest first. */
  seq: number;
}

const _open = new Set<Entry>();
let _seq = 0;
let _wired = false;

/**
 * A click that OPENS a surface is still propagating when the surface
 * registers, so without this it would reach the document listener and close
 * the thing it just opened.
 *
 * `Event.timeStamp` and `performance.now()` share a time origin, so an event
 * created BEFORE this entry existed has `timeStamp <= armedAt` and is ignored
 * for that entry — and only for that entry. No timers, no one-frame delay, and
 * no reliance on every opener remembering to call `stopPropagation()`.
 */
function wire(): void {
  if (_wired) return;
  _wired = true;

  document.addEventListener("click", (ev) => {
    if (_open.size === 0) return;
    for (const entry of [..._open]) {
      if (ev.timeStamp <= entry.armedAt) continue;   // the click that opened it
      close(entry);
    }
  });

  // Capture phase: sky mode, the special cards and the conflict prompt all
  // listen for Escape too, and the topmost surface must win. Closing exactly
  // ONE per press is what makes a stack of surfaces feel right — Escape should
  // peel, not clear.
  document.addEventListener(
    "keydown",
    (ev) => {
      if (ev.key !== "Escape" || _open.size === 0) return;
      let top: Entry | null = null;
      for (const entry of _open) if (!top || entry.seq > top.seq) top = entry;
      if (!top) return;
      ev.stopPropagation();
      close(top);
    },
    true,
  );
}

function close(entry: Entry): void {
  if (!_open.delete(entry)) return;   // already gone; never close twice
  try {
    entry.close();
  } catch (e) {
    // A throwing close() must not strand every OTHER open surface.
    console.error("dismissable: a close handler threw", e);
  }
}

/**
 * Make `el` close on an outside press and on Escape.
 *
 * Call it where the surface is SHOWN, and call the returned function where it
 * is hidden. Registering twice for the same element is harmless — the second
 * registration supersedes the first.
 */
export function registerDismissable(el: HTMLElement, onClose: () => void): () => void {
  wire();

  // Supersede any previous registration of the same element, so a surface that
  // is re-opened without being unregistered cannot accumulate entries.
  for (const e of [..._open]) if (e.el === el) _open.delete(e);

  // The local half of the contract, applied for the caller so it cannot be the
  // thing that is forgotten.
  el.addEventListener("click", (e) => e.stopPropagation());

  const entry: Entry = { el, close: onClose, armedAt: performance.now(), seq: ++_seq };
  _open.add(entry);
  return () => { _open.delete(entry); };
}

/** Close every registered surface. Used by main.ts's `closeAllPopovers`. */
export function dismissAll(): void {
  for (const entry of [..._open]) close(entry);
}

/** How many registered surfaces are open. For tests and for Escape ordering. */
export function openDismissableCount(): number {
  return _open.size;
}
