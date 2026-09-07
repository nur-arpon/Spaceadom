/**
 * profile-editor.ts — profiles, behind the top-right pill popover.
 *
 * V14: the sidebar and the New Profile modal are gone; the design puts
 * profiles in a popover with pill rows and an inline "＋ New profile" field
 * (Dashboard Earthy v2.dc.html).
 *
 * set_active_profile SAVES on the Rust side — never persistConfig() after it
 * (that was the double config-save bug).
 *
 * ---------------------------------------------------------------------------
 * 1.0.96 — EDIT MODE, from the owner's artboards
 * ---------------------------------------------------------------------------
 *
 * The popover now has two states, and the whole feature turns on keeping them
 * apart:
 *
 *   NORMAL (unchanged, deliberately)   a row is a BUTTON. One click switches
 *                                      profile, double-click renames, the ✕
 *                                      deletes with the same two-step arming
 *                                      PROBLEM 105/108 built. Nothing about
 *                                      this path was touched.
 *
 *   EDIT (new)                         a row is an OBJECT. One click does NOT
 *                                      switch — it renames. The drag handle
 *                                      reorders, the copy icon duplicates, the
 *                                      ⤒ exports, the emoji disc opens
 *                                      Windows' emoji panel. "Done" is the
 *                                      only way out.
 *
 * **Why single-click must stop switching in edit mode**: every other control
 * on the row is small, and a mis-aimed click at a 22px icon lands on the row.
 * In normal mode that switches profile, which is harmless. In edit mode the
 * user is rearranging things they intend to keep, and silently switching the
 * ACTIVE profile mid-edit is a side effect they never asked for — the same
 * class of bug as PROBLEM 178's trap, where a control did something other than
 * what the user was reaching for.
 *
 * **THE ROW ORDER IS FUNCTIONAL.** `config.profiles` is the order RAlt cycles
 * in, so a drag changes behaviour, not decoration. That is why the handle only
 * exists in edit mode and why `reorder_profiles` validates the whole set in
 * Rust rather than trusting the list this file builds from the DOM.
 *
 * **DEVIATION FROM THE ARTBOARD, stated so nobody has to re-derive it.** The
 * artboard enumerates the row as: drag handle · emoji disc · name + subtitle ·
 * duplicate · ✕. Export is listed separately as an "edit-mode affordance"
 * without a home. It is on the ROW (a third icon, ⤒, before duplicate) because
 * export needs a target profile, and the row is the only place where "which
 * one?" has an answer that costs the user nothing. The alternatives were a
 * footer button that exports the active profile (surprising — in edit mode a
 * click no longer changes which one that is) or a second picking mode
 * (a mode inside a mode). Import has no target, so it IS in the footer.
 */
import { invoke } from "@tauri-apps/api/core";
import { showToast } from "./toast";
import { askConfirm } from "./confirm-dialog";
import type { AppConfig, Profile } from "../types.ts";

/**
 * The undo banner lives in main.ts, and this import is DYNAMIC on purpose.
 *
 * PROBLEM 148's rule: the preview harness can only render a component that
 * imports nothing from main.ts, because main.ts's module body registers a
 * DOMContentLoaded bootstrap that would tear the harness down and rebuild the
 * real dashboard against a backend that is not there. A static
 * `import { offerUndo } from "../main"` is what kept this file out of
 * preview.html for three versions — so the profile popover, the one surface
 * with a drag gesture and a rename flow in it, was the one that could never be
 * driven in a browser.
 *
 * In the shipping app this costs nothing: main.ts is the entry, it has already
 * imported THIS file, so the module is resolved and evaluated long before any
 * delete happens and the promise settles on the existing instance.
 */
async function offerUndoBanner(): Promise<void> {
  try {
    const m = await import("../main");
    m.offerUndo();
  } catch (e) {
    // The harness has no undo banner and no main.ts worth loading. The undo
    // itself is still in Rust either way — this is only the offer.
    console.info("profile-editor: no undo banner in this context", e);
  }
}

/// PROBLEM 197 — this used to be `/^[a-zA-Z0-9_]{1,24}$/`, and nothing in the
/// app ever needed it that strict. The owner, 2026-08-26: *"why is new
/// profile name restricted to only letters, numbers or underscore? People
/// might want to name with space or dash or anything."*
///
/// Traced every use of a profile name before answering: it's a plain JSON
/// string, compared with `==`, and the one place the frontend touches it
/// structurally is `row.dataset.profileName = profile.name` — an HTML
/// `data-*` attribute, which accepts any string, no escaping required. It is
/// never a filename, a CSS selector, or anything with real syntax rules — the
/// restriction had no technical basis, just an unexamined "identifier-safe"
/// default. Mirror of `regex_lite` in src-tauri/src/commands.rs — the two
/// MUST stay in sync, since the frontend check exists only to fail fast
/// before the round trip; Rust's is the one that is actually enforced.
///
/// (One name IS a filename now — `config::sanitise_for_filename` builds the
/// pre-delete backup's filename from it. That constraint lives entirely in
/// Rust, on the filename, and never on what the user may type.)
///
/// Length 1–24 and no control characters — a stray tab/newline could still
/// break the single-line pill this renders into. The `u` flag makes `{1,24}`
/// count by Unicode CODE POINT rather than UTF-16 code unit, so one emoji
/// outside the BMP costs 1 toward the limit, not 2 — matching how Rust's
/// `chars().count()` in `regex_lite` counts the same string. Spaces, dashes,
/// punctuation, accented letters and emoji are all fine now.
const PROFILE_NAME_RE = /^[^\x00-\x1f\x7f]{1,24}$/u;

let _config: AppConfig | null = null;
let _onProfileSwitch: ((name: string) => void) | null = null;

/** Edit mode. The Done button is its only exit; see the file header. */
let _editing = false;
/** Which row's emoji slot is open, if any. One at a time. */
let _emojiFor: string | null = null;
/** A pending delete's undo offer, rendered where the row was. */
let _pendingUndo: {
  name: string;
  index: number;
  timer: number;
  /** Ticks `secondsLeft` down and repaints the button text — PROBLEM 256. */
  tick: number;
  secondsLeft: number;
} | null = null;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export function initProfileEditor(
  config: AppConfig,
  onProfileSwitch: (name: string) => void,
): void {
  _config = config;
  _onProfileSwitch = onProfileSwitch;
  wireNewProfile();
  ensureChrome();
  renderProfileList();
}

export function refreshProfileList(config: AppConfig): void {
  _config = config;
  renderProfileList();
  syncPill();
}

/**
 * Keep the top-right pill in step with the active profile.
 *
 * The disc shows the profile's EMOJI when it has one and its initial letter
 * when it does not — `Profile::emoji`'s rule, that a profile without one keeps
 * the look it has always had, applied here. `.textContent` either way: an
 * emoji is user data like the name is.
 */
export function syncPill(): void {
  if (!_config) return;
  const name = _config.active_profile;
  const nameEl = document.getElementById("profile-pill-name");
  const initEl = document.getElementById("profile-pill-initial");
  if (nameEl) nameEl.textContent = name;
  if (!initEl) return;
  const emoji = _config.profiles.find((p) => p.name === name)?.emoji ?? null;
  if (emoji) {
    initEl.textContent = emoji;
    initEl.classList.add("has-emoji");
  } else {
    initEl.textContent = (name[0] ?? "·").toUpperCase();
    initEl.classList.remove("has-emoji");
  }
}

/**
 * Leave edit mode and drop any half-finished emoji box.
 *
 * Called when the popover closes, for the same reason PROBLEM 178 resets the
 * "＋ New profile" row: a mode that outlives the surface it belongs to is a
 * mode the user cannot see and did not ask to still be in.
 */
export function exitEditMode(): void {
  if (!_editing && !_emojiFor) return;
  _editing = false;
  _emojiFor = null;
  renderProfileList();
}

// ---------------------------------------------------------------------------
// Popover chrome — built here, not in index.html
// ---------------------------------------------------------------------------

/**
 * The header ("Profiles" + Edit/Done) and the edit-mode footer (Import).
 *
 * Built in TypeScript rather than added to index.html deliberately: this file
 * already owns everything inside the popover, and markup that only one module
 * ever touches is easier to keep honest next to the code that drives it. It
 * also keeps the preview harness working from the same source — preview.html
 * has the same bare `#profile-popover` and gets the same chrome.
 */
function ensureChrome(): void {
  const pop = document.getElementById("profile-popover");
  const list = document.getElementById("profile-list");
  if (!pop || !list || document.getElementById("profile-editmode")) return;

  const head = document.createElement("div");
  head.id = "profile-pop-head";

  const title = document.createElement("span");
  title.id = "profile-pop-title";
  title.textContent = "Profiles";

  const edit = document.createElement("button");
  edit.type = "button";
  edit.id = "profile-editmode";
  edit.className = "profile-editmode";
  edit.textContent = "Edit";
  edit.title = "Rearrange, rename, duplicate and delete profiles";
  edit.addEventListener("click", (e) => {
    e.stopPropagation();
    _editing = !_editing;
    if (!_editing) _emojiFor = null;
    renderProfileList();
  });

  head.append(title, edit);
  pop.insertBefore(head, list);

  // Import sits in the FOOTER because it has no target profile — see the file
  // header for why Export does not.
  const imp = document.createElement("button");
  imp.type = "button";
  imp.id = "profile-import";
  imp.className = "dashed-btn profile-import";
  imp.textContent = "⤓ Import a profile…";
  imp.hidden = true;
  imp.addEventListener("click", (e) => {
    e.stopPropagation();
    void importProfile();
  });
  pop.appendChild(imp);
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/** The disc's glyph: the emoji if there is one, else the initial. */
function discGlyph(p: Profile): string {
  return p.emoji || (p.name[0] ?? "?").toUpperCase();
}

function iconButton(cls: string, glyph: string, label: string): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.className = cls;
  b.textContent = glyph;
  b.title = label;
  b.setAttribute("aria-label", label);
  return b;
}

function renderProfileList(): void {
  if (!_config) return;
  const list = document.getElementById("profile-list");
  if (!list) return;

  const pop = document.getElementById("profile-popover");
  pop?.classList.toggle("editing", _editing);
  const editBtn = document.getElementById("profile-editmode");
  if (editBtn) {
    editBtn.textContent = _editing ? "Done" : "Edit";
    editBtn.classList.toggle("done", _editing);
    editBtn.title = _editing
      ? "Finish editing — clicking a profile switches to it again"
      : "Rearrange, rename, duplicate and delete profiles";
  }
  const imp = document.getElementById("profile-import") as HTMLElement | null;
  if (imp) imp.hidden = !_editing;

  list.innerHTML = "";

  _config.profiles.forEach((profile, i) => {
    // The undo offer stands IN PLACE of the row that was deleted, so the row
    // comes back where it was — see offerDeleteUndo.
    if (_pendingUndo && _pendingUndo.index === i) list.appendChild(undoRow());

    const isActive = profile.name === _config!.active_profile;
    const count = Object.values(profile.bindings).filter(
      (b) => b.app || b.web_url,
    ).length;

    const row = document.createElement("div");
    row.className = "profile-row" + (isActive ? " active" : "");
    row.dataset.profileName = profile.name;
    row.setAttribute("role", "listitem");
    row.tabIndex = 0;
    row.setAttribute("aria-current", isActive ? "true" : "false");
    row.style.animationDelay = `${i * 55}ms`;

    // --- drag handle (edit mode only) --------------------------------
    if (_editing) {
      const grip = document.createElement("span");
      grip.className = "profile-row-grip";
      grip.textContent = "⠿";
      grip.title = `Drag to move ${profile.name} — this is the order RAlt cycles in`;
      grip.setAttribute("aria-hidden", "true");
      // HTML5 drag-and-drop, not pointer maths: the row is inside a popover
      // that closes on any outside click, and a hand-rolled drag would have to
      // fight that. `draggable` is set on the ROW but armed only from the
      // grip, so a click anywhere else still means what it means.
      grip.addEventListener("mousedown", () => { row.draggable = true; });
      grip.addEventListener("mouseup", () => { row.draggable = false; });
      row.append(grip);
      wireRowDrag(row, list);
    }

    // --- emoji / initial disc ----------------------------------------
    if (_editing && _emojiFor === profile.name) {
      row.append(emojiInput(profile));
    } else {
      const disc = document.createElement("span");
      disc.className = "profile-row-icon" + (profile.emoji ? " has-emoji" : "");
      disc.textContent = discGlyph(profile);
      if (_editing) {
        disc.classList.add("editable");
        disc.title = profile.emoji
          ? `Change ${profile.name}'s emoji`
          : `Give ${profile.name} an emoji`;
        disc.addEventListener("click", (e) => {
          e.stopPropagation();
          openEmojiSlot(profile.name);
        });
        // The tiny ✕ from the artboard, present ONLY when there is something
        // to clear. An always-visible clear button on an empty slot is a
        // control that does nothing — the rule CLAUDE.md opens with.
        if (profile.emoji) {
          const clear = iconButton(
            "profile-row-emoji-clear",
            "✕",
            `Remove ${profile.name}'s emoji`,
          );
          clear.addEventListener("click", (e) => {
            e.stopPropagation();
            void saveEmoji(profile.name, null);
          });
          const wrap = document.createElement("span");
          wrap.className = "profile-row-disc-wrap";
          wrap.append(disc, clear);
          row.append(wrap);
        } else {
          row.append(disc);
        }
      } else {
        row.append(disc);
      }
    }

    // --- name + count -------------------------------------------------
    const text = document.createElement("span");
    text.className = "profile-row-text";
    const nameEl = document.createElement("span");
    nameEl.className = "profile-row-name";
    nameEl.textContent = profile.name;      // textContent — user data
    // Rename has no button of its own (unlike delete's ✕) — the pill is
    // already the busiest row in the popover, and every other rename-style
    // control in this app (the key-detail-panel URL field) is click-to-edit
    // in place, not a dedicated button. The tooltip is the only affordance,
    // so it has to say the actual gesture rather than just "rename" — and the
    // gesture is different in the two modes, so the tooltip is too.
    nameEl.title = _editing
      ? `Click to rename ${profile.name}`
      : `Double-click to rename ${profile.name}`;
    const countEl = document.createElement("span");
    countEl.className = "profile-row-count";
    countEl.textContent = `${count} ${count === 1 ? "key" : "keys"}`;
    text.append(nameEl, countEl);
    row.append(text);

    // --- edit-mode actions --------------------------------------------
    if (_editing) {
      const exp = iconButton("profile-row-act", "⤒", `Export ${profile.name} to a file`);
      exp.addEventListener("click", (e) => {
        e.stopPropagation();
        void exportProfile(profile.name);
      });

      const dup = iconButton("profile-row-act", "⧉", `Duplicate ${profile.name}`);
      dup.addEventListener("click", (e) => {
        e.stopPropagation();
        void duplicateProfile(profile.name);
      });
      row.append(exp, dup);
    }

    // --- delete (both modes) ------------------------------------------
    const del = document.createElement("button");
    del.type = "button";
    // PROBLEM 105 — an armed row says so ON the control, not only in a toast
    // that may have already faded. The fallback profile also carries a warning
    // in its tooltip, so the consequence is discoverable BEFORE the first click.
    const armed = _armedDelete === profile.name;
    del.className = "profile-row-del" + (armed ? " armed" : "");
    del.textContent = armed ? "Delete?" : "✕";
    del.title = profile.name === FALLBACK_PROFILE
      ? `Delete ${profile.name} — WARNING: this is the fallback profile. Keys you have not assigned in your other profiles are rerouted here and will stop working.`
      : `Delete ${profile.name}`;
    del.setAttribute("aria-label", del.title);
    row.append(del);

    row.addEventListener("click", (e) => {
      const t = e.target as HTMLElement;
      if (t.closest(".profile-row-del")) return;
      if (t.closest(".profile-row-act")) return;
      if (t.closest(".profile-row-disc-wrap")) return;
      if (t.closest(".profile-row-icon")) return;
      // EDIT MODE: a click renames, it does not switch. See the file header.
      if (_editing) { startInlineRename(row, profile.name); return; }
      void switchProfile(profile.name);
    });
    row.addEventListener("keydown", (e) => {
      if (e.key !== "Enter") return;
      if (_editing) startInlineRename(row, profile.name);
      else void switchProfile(profile.name);
    });
    // Double-click still renames outside edit mode — that path is untouched.
    row.addEventListener("dblclick", () => {
      if (!_editing) startInlineRename(row, profile.name);
    });
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      void confirmDeleteProfile(profile.name);
    });

    list.appendChild(row);

    // The helper line sits under the row it belongs to, not at the top of the
    // popover: the user is looking at the disc they just clicked.
    if (_editing && _emojiFor === profile.name) list.appendChild(emojiHelper(profile.name));
  });

  // A delete of the LAST row leaves an index past the end.
  if (_pendingUndo && _pendingUndo.index >= _config.profiles.length) {
    list.appendChild(undoRow());
  }

  syncPill();
}

// ---------------------------------------------------------------------------
// Drag to reorder
// ---------------------------------------------------------------------------

/**
 * HTML5 drag-and-drop on the rows, with Rust validating the result.
 *
 * The DOM is moved live during the drag (so the drop lands where it looks like
 * it will), and the ORDER IS ONLY COMMITTED ON DROP. If `reorder_profiles`
 * refuses — which it does for any list that does not match the config exactly
 * — the list is re-rendered from `_config` and the drag simply did not happen.
 * That is why the command validates a set rather than trusting this: the DOM
 * can be stale in ways this file cannot detect.
 */
let _dragging: HTMLElement | null = null;

function wireRowDrag(row: HTMLElement, list: HTMLElement): void {
  row.addEventListener("dragstart", (e) => {
    _dragging = row;
    row.classList.add("dragging");
    // Firefox refuses to start a drag without data on the transfer.
    e.dataTransfer?.setData("text/plain", row.dataset.profileName ?? "");
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  });

  row.addEventListener("dragend", () => {
    row.classList.remove("dragging");
    row.draggable = false;
    _dragging = null;
    void commitOrder(list);
  });

  row.addEventListener("dragover", (e) => {
    e.preventDefault();
    if (!_dragging || _dragging === row) return;
    const r = row.getBoundingClientRect();
    const after = e.clientY > r.top + r.height / 2;
    list.insertBefore(_dragging, after ? row.nextSibling : row);
  });
}

async function commitOrder(list: HTMLElement): Promise<void> {
  if (!_config) return;
  const names = Array.from(list.querySelectorAll<HTMLElement>(".profile-row"))
    .map((el) => el.dataset.profileName ?? "")
    .filter(Boolean);

  const before = _config.profiles.map((p) => p.name);
  if (names.length === before.length && names.every((n, i) => n === before[i])) return;

  try {
    await invoke("reorder_profiles", { names });
    // Reorder the local copy to match, rather than re-fetching: Rust has just
    // validated that these are exactly our profiles, so the mapping is total.
    const byName = new Map(_config.profiles.map((p) => [p.name, p]));
    _config.profiles = names.map((n) => byName.get(n)!).filter(Boolean);
    renderProfileList();
    showToast("↕ Profile order saved");
  } catch (e) {
    // Rust refused — put the list back the way the config says it is. The
    // drag visibly undoes itself, which is the honest outcome.
    showToast(`⚠️ ${e}`);
    renderProfileList();
  }
}

// ---------------------------------------------------------------------------
// Emoji
// ---------------------------------------------------------------------------

/**
 * Open the emoji slot for one profile and ask Windows for its emoji panel.
 *
 * The ORDER MATTERS and it is not obvious: the input has to be focused BEFORE
 * `open_emoji_panel` injects Win+`.`, because the panel inserts into whatever
 * has keyboard focus at the moment it opens. Focusing afterwards would race
 * the panel and, on a slow open, drop the first emoji into nothing.
 *
 * A failed injection is not a dead end — Rust's error says so, and the box is
 * already focused, so the user can press Windows + . themselves or paste.
 */
function openEmojiSlot(name: string): void {
  _emojiFor = name;
  renderProfileList();
  const input = document.getElementById("profile-emoji-input") as HTMLInputElement | null;
  input?.focus();
  invoke("open_emoji_panel").catch((e) => showToast(`⚠️ ${e}`));
}

/**
 * The first grapheme cluster in `s`, or `""` if `s` is empty.
 *
 * 1.0.96 — PROBLEM 235. The picker now saves on the FIRST cluster that lands
 * in the box instead of waiting for Enter, so this has to get "one emoji"
 * exactly right: "👨‍👩‍👧" is 8 UTF-16 code units and 4 code points, all of
 * which must be treated as ONE pick.
 *
 * `Intl.Segmenter` (Chromium 87+, so always present in the WebView2 this app
 * ships on) does this correctly per UAX #29 and is tried first. It is not in
 * this project's `tsconfig.json` `lib` (`ES2020`; raising the whole project's
 * target for one call site is the wrong trade — `preview.ts` made the same
 * call for its own stub), so it is reached through a runtime feature check
 * behind a narrow local cast rather than a type-level `Intl.Segmenter`
 * reference. Falls back to the SAME walk `schema::cluster_count` (Rust,
 * `src-tauri/src/config/schema.rs`) and `preview.ts`'s `stubClusters` already
 * make — ZWJ, variation/skin-tone/tag selectors, combining marks and
 * regional-indicator flag pairs all join the cluster they follow — just
 * stopped at the first cluster boundary instead of counting to the end.
 */
function firstGraphemeCluster(s: string): string {
  if (!s) return "";

  const SegmenterCtor = (
    Intl as unknown as {
      Segmenter?: new (
        locale?: string,
        options?: { granularity?: string },
      ) => { segment(input: string): Iterable<{ segment: string }> };
    }
  ).Segmenter;
  if (SegmenterCtor) {
    for (const { segment } of new SegmenterCtor(undefined, { granularity: "grapheme" }).segment(
      s,
    )) {
      return segment;
    }
    return "";
  }

  let result = "";
  let startedCluster = false;
  let afterZwj = false;
  let regionalOpen = false;
  for (const c of s) {
    const cp = c.codePointAt(0)!;
    const joins =
      (cp >= 0xfe00 && cp <= 0xfe0f) || // variation selectors
      (cp >= 0x1f3fb && cp <= 0x1f3ff) || // emoji skin-tone modifiers
      (cp >= 0xe0020 && cp <= 0xe007f) || // tag characters (subdivision flags)
      cp === 0x20e3 || // combining enclosing keycap
      (cp >= 0x0300 && cp <= 0x036f) || // combining diacritical marks
      (cp >= 0x1ab0 && cp <= 0x1aff) ||
      (cp >= 0x1dc0 && cp <= 0x1dff) ||
      (cp >= 0x20d0 && cp <= 0x20ff) ||
      (cp >= 0xfe20 && cp <= 0xfe2f);
    const regional = cp >= 0x1f1e6 && cp <= 0x1f1ff;

    if (cp === 0x200d) {
      result += c;
      afterZwj = true;
      regionalOpen = false;
      continue;
    }
    if (joins) {
      result += c;
      afterZwj = false;
      continue;
    }
    if (regional && regionalOpen) {
      result += c;
      regionalOpen = false;
      afterZwj = false;
      continue;
    }
    if (afterZwj) {
      result += c;
      afterZwj = false;
      regionalOpen = regional;
      continue;
    }
    // `c` starts a NEW cluster. Stop before consuming it once the first
    // cluster is already complete.
    if (startedCluster) return result;
    startedCluster = true;
    regionalOpen = regional;
    result += c;
  }
  return result;
}

function emojiInput(profile: Profile): HTMLElement {
  const input = document.createElement("input");
  input.id = "profile-emoji-input";
  input.className = "profile-emoji-input";
  // ALWAYS starts empty, even when `profile` already has an emoji — never
  // `profile.emoji ?? ""`. This box now saves on its first grapheme cluster
  // (below), and Windows' emoji panel inserts at the caret rather than
  // replacing the field's contents; a pre-filled OLD emoji would make the
  // panel's insertion a SECOND cluster appended after it, so this function
  // would return the stale first cluster and silently keep the old pick
  // while looking like it saved the new one. The current emoji is already
  // visible on the disc and the top-right pill — this box only ever needs to
  // catch a fresh pick.
  input.value = "";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", `Emoji for ${profile.name}`);
  // NO `maxLength`. An emoji is one GRAPHEME CLUSTER, not one JS character:
  // "👨‍👩‍👧" has `.length === 8`, so `maxLength = 1` or 2 would truncate it
  // into a lone man and a stray joiner. Rust's `emoji_is_valid` is the check,
  // and it counts clusters.
  input.addEventListener("click", (e) => e.stopPropagation());
  // 1.0.96 — PROBLEM 235. The owner: "it can only hold one emoji — as soon as
  // someone picks an emoji it should take it and save it; the option of
  // choosing another shouldn't even come." So the pick and the save are the
  // SAME event — there is no separate confirm step, and no Enter/Esc dance
  // to document. `input` fires for a real keystroke, a paste, AND Windows'
  // emoji-panel insertion alike, so this is the one listener that catches
  // all three.
  input.addEventListener("input", (e) => {
    // REVIEW FIX 2026-09-04 — ignore an `input` fired mid-composition. An IME
    // (and Windows' own handwriting/CJK候補 flow) emits `input` for each
    // intermediate character with `isComposing === true`; saving on the first
    // of those would commit a half-composed glyph and blur the field out from
    // under the composition. The `compositionend` that follows fires its own
    // non-composing `input`, so nothing is lost by waiting for it.
    if ((e as InputEvent).isComposing) return;
    const cluster = firstGraphemeCluster(input.value);
    if (!cluster) return;
    // Blur FIRST, before the round trip to Rust. Windows' emoji panel is
    // documented to close when the element it targets loses focus, and that
    // should not wait on `set_profile_emoji` to settle. (Not independently
    // verified live on this machine this session — see PROBLEM 235's entry
    // in V14_FIXES_AND_CODE.md for why an Esc-injection fallback was
    // considered and rejected rather than added speculatively: it would
    // reach the SAME document-level Escape handler that closes this whole
    // popover, `closeAllPopovers` in main.ts, because an injected key is not
    // distinguishable from a real one at the DOM level.)
    input.blur();
    void saveEmoji(profile.name, cluster);
  });
  input.addEventListener("keydown", (e) => {
    // Stop here, not just for Escape: this box lives inside a popover that
    // treats a global Escape as "close everything" (`closeAllPopovers`), and
    // every other key already meant nothing to this app's shortcuts while
    // typing in a text field.
    e.stopPropagation();
    if (e.key === "Escape") {
      // Closes the box WITHOUT saving — the user backed out before picking
      // anything. Once a pick has landed, the `input` listener above has
      // already saved and hidden this element, so there is nothing left for
      // Escape to undo by the time it would matter.
      e.preventDefault();
      _emojiFor = null;
      renderProfileList();
    }
  });
  return input;
}

/**
 * The helper line from the artboard. 1.0.96 — PROBLEM 235 dropped its Clear
 * button: Clear is a secondary, edit-mode-only action now (the tiny ✕ on the
 * disc itself, wired in the caller below), not part of the primary "pick one"
 * flow this line describes.
 */
function emojiHelper(_name: string): HTMLElement {
  const row = document.createElement("div");
  row.className = "profile-emoji-help";

  const txt = document.createElement("span");
  // No mention of Enter or Esc — there is nothing left for either to do
  // that the pick itself does not already do.
  txt.textContent = "Pick one — it saves on its own.";
  row.append(txt);
  return row;
}

async function saveEmoji(name: string, emoji: string | null): Promise<void> {
  try {
    await invoke("set_profile_emoji", { name, emoji });
    if (_config) {
      const p = _config.profiles.find((x) => x.name === name);
      if (p) p.emoji = emoji;
    }
    _emojiFor = null;
    renderProfileList();
    // **NEVER LEAD A TOAST WITH THE USER'S EMOJI**, even though it reads
    // nicely. `toast.ts` treats a leading glyph as the ICON and splits it off
    // with `Array.from(message)[0]` — one CODE POINT, not one cluster. Measured
    // in the harness 2026-09-04: `👨‍👩‍👧 Emoji set for Gamers` rendered a lone
    // 👨 in the disc and put "‍👩‍👧 Emoji set for Gamers" in the text.
    // The leading position is a vocabulary this app owns (⚡ ⚠️ ❌ ↩ ✅); user
    // data goes in the sentence, where nothing parses it.
    showToast(emoji ? `✅ ${name} now shows ${emoji}` : `✅ Emoji removed from ${name}`);
  } catch (e) {
    // The slot STAYS OPEN on failure: the user's pick is still in the box and
    // closing it would throw away work to report a problem with it.
    showToast(`⚠️ ${e}`);
  }
}

// ---------------------------------------------------------------------------
// Duplicate / export / import
// ---------------------------------------------------------------------------

async function duplicateProfile(name: string): Promise<void> {
  try {
    // Rust owns the naming rule ("Name 2", then "Name 3", never "Name 2 2")
    // and returns what it chose — this must not guess, or the toast and the
    // row would disagree the first time the rule handles an edge case.
    const created = await invoke<string>("duplicate_profile", { name });
    const source = _config?.profiles.find((p) => p.name === name);
    if (_config && source) {
      const i = _config.profiles.indexOf(source);
      _config.profiles.splice(i + 1, 0, {
        ...structuredClone(source),
        name: created,
      });
    }
    renderProfileList();
    showToast(`⧉ Duplicated: ${created}`);
  } catch (e) {
    showToast(`⚠️ ${e}`);
  }
}

async function exportProfile(name: string): Promise<void> {
  try {
    // `null` means the user cancelled the save dialog. That is not a failure
    // and must not produce a toast — a message for "you changed your mind" is
    // noise, and this app already learnt that lesson on the browse dialog.
    const path = await invoke<string | null>("export_profile", { name });
    if (path) showToast(`⤒ Exported ${name}`);
  } catch (e) {
    showToast(`⚠️ ${e}`);
  }
}

async function importProfile(): Promise<void> {
  try {
    const added = await invoke<string | null>("import_profile");
    if (!added) return;                       // cancelled — see exportProfile
    // The imported profile's bindings are only in Rust. Ask for the config
    // back rather than reconstructing it here: `KeyBinding` has six optional
    // fields and a hand-built copy would drop the next one added.
    _config = await invoke<AppConfig>("get_config");
    renderProfileList();
    showToast(`⤓ Imported: ${added}`);
  } catch (e) {
    showToast(`⚠️ ${e}`);
  }
}

// ---------------------------------------------------------------------------
// Switch / rename / delete
// ---------------------------------------------------------------------------

async function switchProfile(name: string): Promise<void> {
  try {
    await invoke("set_active_profile", { name });
    if (_config) _config.active_profile = name;
    renderProfileList();
    closeProfilePopover();
    if (_onProfileSwitch) _onProfileSwitch(name);
    showToast(`👤 Profile: ${name}`);
  } catch (_) {
    showToast("⚠️ Failed to switch profile");
  }
}

function startInlineRename(row: HTMLElement, oldName: string): void {
  const nameEl = row.querySelector<HTMLElement>(".profile-row-name");
  if (!nameEl) return;

  const input = document.createElement("input");
  input.className = "input";
  input.value = oldName;
  input.maxLength = 24;
  input.style.cssText = "height:24px; padding:0 10px; font-size:12px; width:100%;";

  nameEl.replaceWith(input);
  input.focus();
  input.select();
  // Clicking into the field must not also switch profile — nor, in edit mode,
  // restart the rename it is already in.
  input.addEventListener("click", (e) => e.stopPropagation());

  let done = false;
  const commit = async () => {
    if (done) return;
    done = true;
    const newName = input.value.trim();
    if (!newName || newName === oldName) { renderProfileList(); return; }
    if (!PROFILE_NAME_RE.test(newName)) {
      showToast("⚠️ Name must be 1–24 characters");
      renderProfileList();
      return;
    }
    try {
      await invoke("rename_profile", { oldName, newName });
      if (_config) {
        const p = _config.profiles.find((x) => x.name === oldName);
        if (p) p.name = newName;
        if (_config.active_profile === oldName) _config.active_profile = newName;
      }
      renderProfileList();
      showToast(`✅ Renamed → ${newName}`);
    } catch (_) {
      showToast("⚠️ Rename failed");
      renderProfileList();
    }
  };

  input.addEventListener("blur", () => void commit());
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") { e.preventDefault(); input.blur(); }
    if (e.key === "Escape") { done = true; renderProfileList(); }
  });
}

/**
 * PROBLEM 105 — two-step delete, with the fallback warning shown IN THE APP.
 *
 * The first attempt used `window.confirm`. It never appeared: this webview
 * does not render native script dialogs, so the delete went straight through
 * and the user saw no warning at all — worse than none, because the code
 * looked like it was protecting them. Every other destructive control here
 * already uses a two-step "Confirm" button for exactly this reason; this now
 * matches them.
 *
 * MUST match FALLBACK_PROFILE in src-tauri/src/config/schema.rs.
 */
const FALLBACK_PROFILE = "Founders";
/** Which profile row is armed for deletion, and the timer that disarms it. */
let _armedDelete: string | null = null;
let _armedDeleteTimer: number | undefined;

/**
 * PROBLEM 108 — two levels of friction, matched to the consequence.
 *
 * Ordinary profiles get the red "Delete?" pill: a light second click, which is
 * how every other destructive control in this app already behaves and is
 * enough for something the user can rebuild.
 *
 * The FALLBACK profile gets the full panel, because its consequence is not
 * about this profile at all — it silently breaks unassigned keys in every
 * OTHER profile, and that needs sentences the user has time to read.
 */
async function confirmDeleteProfile(name: string): Promise<void> {
  if (!_config || _config.profiles.length <= 1) {
    showToast("⚠️ Cannot delete the last profile");
    return;
  }

  if (name === FALLBACK_PROFILE) {
    const ok = await askConfirm({
      title: `Delete "${name}"?`,
      body: `This is the FALLBACK profile.

Any key you have not assigned in your ` +
            `other profiles is currently rerouted here — those keys will stop working.` +
            `

You will have 30 seconds to undo.`,
      danger: true,
      confirmLabel: "Delete anyway",
    });
    if (!ok) return;
  } else {
    // First click arms the pill; second within 4s deletes.
    if (_armedDelete !== name) {
      _armedDelete = name;
      window.clearTimeout(_armedDeleteTimer);
      _armedDeleteTimer = window.setTimeout(() => {
        _armedDelete = null;
        renderProfileList();
      }, 4000);
      renderProfileList();
      return;
    }
    window.clearTimeout(_armedDeleteTimer);
    _armedDelete = null;
  }

  const index = _config.profiles.findIndex((p) => p.name === name);

  try {
    await invoke("delete_profile", { name });
    _config.profiles = _config.profiles.filter((p) => p.name !== name);
    if (_config.active_profile === name) {
      _config.active_profile = _config.profiles[0].name;
      if (_onProfileSwitch) _onProfileSwitch(_config.active_profile);
    }
    // 1.0.96 — the Undo now stands WHERE THE ROW WAS, for ~10s.
    offerDeleteUndo(name, index);
    renderProfileList();
    showToast(`🗑️ Deleted: ${name}`);
    // PROBLEM 256 — the owner: "after deleting a profile there are two places
    // to undo — one top-middle for seconds, another on the profile list —
    // how long is that one?" ONE undo only, by his decision: the inline row
    // above IS the undo for a profile delete, so the top-middle banner
    // (`offerUndo` in main.ts, PROBLEM 99) is deliberately NOT raised here
    // any more — this used to end with `void offerUndoBanner();`, which is
    // exactly what put a second countdown in front of the user for the same
    // action. `offerUndo`/`offerUndoBanner` still fire for `clearActiveProfile`
    // and `resetActiveProfileToDefaults` in main.ts (untouched) — "keep toasts
    // for other events" was the other half of the instruction, and a delete is
    // the only action with a place-in-the-list to stand in for the banner.
  } catch (e) {
    showToast(`⚠️ ${e}`);
  }
}

/**
 * The 10 second Undo the owner asked for, placed like the pin-clear Undo.
 *
 * **DEVIATION, and it is the same one key-detail-panel already documents.**
 * The brief says "a ~10s toast with Undo". It cannot be a toast:
 * `#toast-container` is `pointer-events: none` (toast.ts's `toastLayer`, set
 * via CSSOM), because the same component renders into the transparent
 * CLICK-THROUGH overlay window, where a button is unreachable by definition.
 * An Undo the user cannot click is not an Undo. So it sits where the deleted
 * row was — closer to hand than the bottom of the window, and it makes the
 * "restores in place" promise visible: the offer occupies the position the row
 * will come back to.
 *
 * **PROBLEM 256 — the countdown is now VISIBLE, ticking, on the button
 * itself** ("Undo · 9s", down to "Undo · 0s"), the owner's explicit ask once
 * the top-middle banner (which already ticked) was removed as the second
 * place to undo — see `confirmDeleteProfile`. Losing that banner must not
 * also lose the only clock the user could see counting down.
 *
 * Two things back it up and neither is redundant: Rust's undo STACK (which is
 * what actually restores, and holds 10/20/30s by profile type — PROBLEM 106),
 * and the timestamped file `delete_profile` wrote to `%LOCALAPPDATA%\
 * SpaceadomBackups` before touching anything, for the user who notices next
 * week.
 */
const DELETE_UNDO_MS = 10_000;
const DELETE_UNDO_S = DELETE_UNDO_MS / 1000;

function offerDeleteUndo(name: string, index: number): void {
  if (_pendingUndo) {
    window.clearTimeout(_pendingUndo.timer);
    window.clearInterval(_pendingUndo.tick);
  }
  _pendingUndo = {
    name,
    index: Math.max(0, index),
    secondsLeft: DELETE_UNDO_S,
    timer: window.setTimeout(() => {
      if (_pendingUndo) window.clearInterval(_pendingUndo.tick);
      _pendingUndo = null;
      renderProfileList();
    }, DELETE_UNDO_MS),
    // Updates the button's OWN text node directly rather than calling
    // renderProfileList() every second: a full re-render tears down and
    // rebuilds every row (drag listeners, any open emoji input, hover state)
    // just to change four characters. renderProfileList() still rebuilds the
    // row from scratch on any OTHER change meanwhile (rename, drag, emoji…),
    // and undoRow() below bakes the CURRENT `secondsLeft` into its initial
    // text, so the two stay in sync however often the list repaints between
    // ticks of this interval.
    tick: window.setInterval(() => {
      if (!_pendingUndo) return;
      _pendingUndo.secondsLeft -= 1;
      const btn = document.getElementById("profile-undo-btn");
      if (btn) btn.textContent = `Undo · ${Math.max(0, _pendingUndo.secondsLeft)}s`;
    }, 1000),
  };
}

function undoRow(): HTMLElement {
  const pending = _pendingUndo!;
  const row = document.createElement("div");
  row.className = "profile-undo-row";
  row.setAttribute("role", "status");

  const txt = document.createElement("span");
  txt.className = "profile-undo-text";
  txt.textContent = `Deleted ${pending.name}`;   // textContent — user data

  const btn = document.createElement("button");
  btn.type = "button";
  btn.id = "profile-undo-btn";
  btn.className = "btn btn-sm";
  btn.textContent = `Undo · ${pending.secondsLeft}s`;
  btn.title = `Put ${pending.name} back where it was`;
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    void undoDelete();
  });

  row.append(txt, btn);
  return row;
}

async function undoDelete(): Promise<void> {
  if (_pendingUndo) {
    window.clearTimeout(_pendingUndo.timer);
    window.clearInterval(_pendingUndo.tick);
  }
  _pendingUndo = null;
  try {
    // Rust restores the WHOLE config as it was before the delete, so the
    // profile comes back at its original index with its bindings, its icons
    // and its emoji — "in place" in the literal sense. Re-read rather than
    // patching the local copy: `undo_last_change` may have restored more than
    // this one profile if other destructive actions were stacked behind it.
    const what = await invoke<string>("undo_last_change");
    _config = await invoke<AppConfig>("get_config");
    renderProfileList();
    showToast(`↩ Undone: ${what}`);
  } catch (_) {
    showToast("⚠️ That undo has expired");
    renderProfileList();
  }
  // PROBLEM 256 — a profile delete no longer raises the banner (see
  // confirmDeleteProfile), but the banner and this row still read from the
  // SAME Rust undo stack, and `clearActiveProfile`/`resetActiveProfileToDefaults`
  // in main.ts still use it. If one of those was stacked BEHIND this delete,
  // this refresh is what surfaces it now that the delete's own entry is gone
  // — and if nothing else is pending, `offerUndo` just finds none and leaves
  // the (already-hidden) banner alone.
  void offerUndoBanner();
}

// ---------------------------------------------------------------------------
// Inline "＋ New profile"
// ---------------------------------------------------------------------------

/**
 * PROBLEM 178 — the name box had no way back.
 *
 * The owner, 2026-08-24: *"when I want to add a new profile, a box changes for
 * me to write the name of a profile. But what happens if I don't write
 * anything in it? The 'add new profile' thing doesn't come up again, it still
 * asks me to write the name. So it shouldn't stay like that. If I had not
 * pressed anything after some time, it should go back how it was before."*
 *
 * He is exactly right. `open()` sets `openBtn.hidden = true`, and the ONLY two
 * routes back were Escape and a SUCCESSFUL create. Click elsewhere, or think
 * better of it, and the "＋ New profile" button is gone — not just for that
 * moment, but for the rest of the session, because the row's state outlives
 * the popover being closed and reopened. A control that can only be undone by
 * completing the very action you decided against is a trap.
 *
 * Three ways back now, and each one exists for a different way of changing
 * your mind:
 *   - **Escape** — you decided against it deliberately. (Already existed.)
 *   - **Clicking elsewhere** — you moved on. Reverts only when the box is
 *     EMPTY: a half-typed name is work, and silently discarding it because
 *     focus moved would be its own bug.
 *   - **Doing nothing** — his actual words, "if I had not pressed anything
 *     after some time". An idle timer, re-armed on every keystroke.
 *
 * The row is also reset whenever the popover closes, so reopening it always
 * shows the button rather than resuming an abandoned box.
 */
const NEW_PROFILE_IDLE_MS = 15_000;

/** Reverts the "＋ New profile" row to its button. Safe to call any time. */
let _resetNewProfileRow: () => void = () => {};

export function resetNewProfileRow(): void {
  _resetNewProfileRow();
  // Closing the popover leaves edit mode too — see exitEditMode.
  exitEditMode();
}

function wireNewProfile(): void {
  const openBtn = document.getElementById("new-profile-btn") as HTMLButtonElement | null;
  const row = document.getElementById("new-profile-row") as HTMLElement | null;
  const input = document.getElementById("new-profile-input") as HTMLInputElement | null;
  const addBtn = document.getElementById("new-profile-add") as HTMLButtonElement | null;
  if (!openBtn || !row || !input || !addBtn) return;

  let idle: number | undefined;

  const close = () => {
    window.clearTimeout(idle);
    idle = undefined;
    row.hidden = true;
    openBtn.hidden = false;
    input.value = "";
    input.classList.remove("error");
  };
  // Only ever discards an EMPTY box. Typed text is the user's work.
  const closeIfEmpty = () => { if (!input.value.trim()) close(); };

  const armIdle = () => {
    window.clearTimeout(idle);
    idle = window.setTimeout(closeIfEmpty, NEW_PROFILE_IDLE_MS);
  };

  const open = () => {
    row.hidden = false;
    openBtn.hidden = true;
    input.value = "";
    input.classList.remove("error");
    input.focus();
    armIdle();
  };

  _resetNewProfileRow = close;

  openBtn.addEventListener("click", open);
  addBtn.addEventListener("click", () => void create(input, close));
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    armIdle();                    // typing is "pressing something" — keep it open
    if (e.key === "Enter") void create(input, close);
    if (e.key === "Escape") close();
  });

  // Focus moving away = you moved on. Deferred by a tick, because clicking the
  // Add button blurs the input FIRST — reverting synchronously here would tear
  // the row down before Add's own click handler ever ran, and the profile
  // would silently not be created. `relatedTarget` is null for a click on
  // non-focusable chrome, so the containment test cannot be relied on alone.
  input.addEventListener("blur", () => {
    window.setTimeout(() => {
      if (row.hidden) return;
      if (row.contains(document.activeElement)) return;   // still inside the row
      closeIfEmpty();
    }, 0);
  });
}

async function create(input: HTMLInputElement, close: () => void): Promise<void> {
  const name = input.value.trim();
  if (!PROFILE_NAME_RE.test(name)) {
    showToast("⚠️ Name must be 1–24 characters");
    input.classList.add("error");
    return;
  }
  input.classList.remove("error");

  try {
    await invoke("create_profile", { name });
    if (_config) {
      _config.profiles.push({
        name,
        emoji: null,
        bindings: Object.fromEntries(
          "abcdefghijklmnopqrstuvwxyz"
            .split("")
            .map((k) => [k, { app: null, web_url: null, label: null }]),
        ),
      });
    }
    close();
    renderProfileList();
    showToast(`✅ Profile created: ${name}`);
  } catch (e) {
    showToast(`⚠️ ${e}`);
  }
}

function closeProfilePopover(): void {
  const pop = document.getElementById("profile-popover");
  const pill = document.getElementById("profile-pill");
  if (pop) pop.hidden = true;
  pill?.setAttribute("aria-expanded", "false");
}
