/**
 * controls.ts — the settings switch and slider markup, and the Fun-mode
 * character each one performs.
 *
 * A LEAF module on purpose. It was briefly a pair of exports on
 * settings-panel.ts, and importing that into `preview.ts` dragged main.ts in
 * behind it: main's bootstrap ran in the dev harness, failed on a missing
 * Tauri `invoke`, and blanked the page with the fatal-error screen — the
 * harness rendered nothing at all. Nothing here imports from the app.
 *
 * Spec: design/design-system-overhaul-3.md §2. The motion itself is in
 * styles/characters.css; this file only decides WHICH character a row gets.
 *
 * `@tauri-apps/api` is the ONE import here and it does not cost the leaf
 * property: `invoke` reads `window.__TAURI_INTERNALS__.invoke` at CALL time,
 * so `preview.ts` keeps working with its stub backend and nothing from the app
 * is dragged in behind it.
 */
import { invoke } from "@tauri-apps/api/core";
/**
 * Same reasoning as the `invoke` import above: `getVersion` reads a value
 * Tauri's runtime exposes at CALL time, and `openUrl` is a thin wrapper over
 * `window.__TAURI_INTERNALS__` too — neither drags anything from `main.ts`
 * behind it, so the ABOUT section's markup-and-data helpers below can live in
 * this leaf module and be rendered identically by `settings-panel.ts` and by
 * `preview.ts` (PROBLEM 148's whole reason for existing).
 */
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
// PROBLEM 253 (a parallel lane's work, landed while this one was in progress).
//
// RECONCILED 2026-09-05: this comment used to warn that the feature's own
// files said "251". They do not any more — the one surviving stale "251",
// in `diagnostics.rs`, turned out to be pointing at PROBLEM 254 (the portable
// data root) rather than at 253 at all, and was corrected to 254 in the
// wiring pass. Nothing in this repository now labels the
// safe-mode/diagnostics/report-dialog feature anything but 253. The note is
// kept rather than deleted because PROJECT_STATUS.md's 2026-09-05 entry
// records the mislabel, and a reader arriving from there needs to know it was
// resolved and how.
//
// report-dialog.ts is ITSELF a leaf module (imports only
// `invoke`), built explicitly so both `main.ts`'s safe-mode banner and this
// file's About section could call the one dialog without either dragging the
// other's dependencies in. See that file's doc comment.
import { openReportDialog } from "./report-dialog";
/**
 * Which character each switch performs when Fun mode is on (spec §2).
 * The mapping is deliberately by CHARACTER, not by row, so two switches that
 * mean the same kind of thing move the same way:
 *
 *   thr  thruster    engine ignition — the switch whose SOUND is already a
 *                    thruster note
 *   fun  orbit hop   the personality switch itself, accent -> sage track
 *   orb  orbit hop   run at startup (spec: "same as fun"), hiding the
 *                    board, where the knob arcs up and away like the layout,
 *                    and showing the HUD's special keys — an inner ring
 *                    appearing and disappearing IS an orbit
 *   rng  sonar ring  sound ticks, point-to-launch and the crash-report
 *                    opt-out — each is "something went out and came back"
 *                    (the pointer pings a chip and the launch answers; a
 *                    crash report leaves the machine and an answer comes
 *                    back as a fixed build)
 *   wrp  warp smear  visual effects, and the guide-to-toast flight — the
 *                    switch that governs a smear is best shown as one
 *
 * 2026-09-01 — `around`, `software` and `hudlayout` were REMOVED from this
 * table because their switches were removed from the panel: "Show me around"
 * became the header link, "Software overlay" became the Conflicts-area
 * "The ring isn't showing?" tool, and "New ring layout" was folded into the
 * Compact/Wide/Double pill. A stale entry here is dead data, and dead data in
 * a lookup table is how the next reader concludes a control still exists.
 * `sendlogs` was ADDED in the same pass: it had NEVER had an entry, so the
 * one switch in the panel that concerns what leaves the machine was silently
 * performing the fallback character (see `toggleChar` below).
 *
 * 2026-09-10 — `middlering` ADDED with the row itself (PROBLEM 263), rather
 * than discovered missing five versions later the way `sendlogs` was. It is
 * `rng` on the character's own terms, not because it sits next to
 * `hudpointer` in the panel: press the middle button and the ring goes out,
 * release it and the launch comes back — the same "something went out and
 * came back" round trip point-to-launch is `rng` for, performed by the hand
 * instead of the cursor.
 */
const TOGGLE_CHAR: Record<string, string> = {
  engine: "thr", fun: "fun", sound: "rng",
  startup: "orb", motion: "wrp", hideboard: "orb",
  flight: "wrp", hudpointer: "rng", hudspecials: "orb", sendlogs: "rng",
  middlering: "rng", tpslidetoast: "rng",
};

/**
 * SEGMENTED PILLS have no entry here and must not get one: they have no knob
 * to animate, so `theme` has never had one and neither does `hudring`. That
 * absence is deliberate — `segRowHtml` below never consults this table.
 *
 * THE FALLBACK IS NO LONGER SILENT. It used to be a bare
 * `TOGGLE_CHAR[id] ?? "wrp"`, and that is exactly how `sendlogs` shipped for
 * five versions wearing a character nobody chose for it: the switch looked
 * perfect, so nothing ever pointed at the table. A missing id still renders —
 * a settings row must never fail to draw because of a decoration — but it now
 * says so once, in the console and in the Rust log if the bridge is there.
 */
const _charWarned = new Set<string>();
export function toggleChar(id: string): string {
  const hit = TOGGLE_CHAR[id];
  if (hit) return hit;
  if (!_charWarned.has(id)) {
    _charWarned.add(id);
    console.warn(
      `controls: no Fun-mode character for switch "${id}" — falling back to "wrp". ` +
      `Add it to TOGGLE_CHAR in src/components/controls.ts.`,
    );
  }
  return "wrp";
}

/**
 * The reason line shown under "Show special keys" when the rows pill is on
 * "2 rows" and the switch therefore has nothing to do.
 *
 * It lives here, beside the markup, because BOTH the panel and `preview.ts`
 * render it and prose duplicated across two files drifts the moment one is
 * edited. It says the three things the user needs: why it is greyed out, what
 * to change to get it back, and that nothing they chose was thrown away.
 */
export const SPECIALS_INERT_NOTE =
  "Two rings of apps use all the room — pick Compact or Wide and they come back.";

/**
 * THE SPACE RING'S SHAPE, as ONE 3-way pill (owner, 2026-09-01).
 *
 * It replaces two controls that had to be read together — the "New ring
 * layout" switch and the "Shortcut rows" pill — and whose dependency
 * (`ROWS_INERT_NOTE`, deleted with them) existed only to explain why one of
 * them was sometimes dead. Three named shapes have no dead state to explain.
 *
 * NO SCHEMA CHANGE. The three names map onto the two config fields that are
 * already there, and Rust reads exactly what it always did:
 *
 *   Compact = hud_magnetic_layout true  + hud_band_count "auto"
 *   Wide    = hud_magnetic_layout false            (band count left alone)
 *   Double  = hud_magnetic_layout true  + hud_band_count "two"
 *
 * `hud_band_count: "one"` IS RETIRED — the UI never writes it again. A config
 * that still holds it (anything a 1.0.89–1.0.95 user selected "1 row" on)
 * shows **Compact**, because forced-one and auto look identical whenever the
 * labels fit, and the next press of the pill normalises it to "auto". See
 * `ringConfigFor`: that is why picking the option you are already on is NOT a
 * no-op here, unlike the theme pill.
 */
export const RING_OPTS: ReadonlyArray<readonly [string, string]> = [
  ["compact", "Compact"],
  ["wide", "Wide"],
  ["double", "Double"],
];

export type RingLayout = "compact" | "wide" | "double";

/**
 * Config → pill. A PURE function, in the leaf module, so `preview.ts` reads
 * the mapping the app reads and the two cannot drift.
 *
 * `magnetic` takes the raw field: `!== false`, never `=== true`, because the
 * key is absent from every config written before 1.0.89 and absent means the
 * new ring (the same rule `applyHudLayout` applies in the overlay).
 */
export function ringLayoutFor(magnetic: unknown, band: unknown): RingLayout {
  if (magnetic === false) return "wide";
  return band === "two" ? "double" : "compact";
}

/**
 * Pill → config. Returns the two fields to write; `band === null` means
 * "leave `hud_band_count` exactly as it is".
 *
 * Wide deliberately does NOT touch the band count: a user who had Double and
 * takes a detour through Wide gets Double's row count back when they return,
 * which is the same "presentation, not a lost preference" rule the inert
 * treatment follows.
 */
export function ringConfigFor(
  layout: RingLayout,
): { magnetic: boolean; band: "auto" | "two" | null } {
  if (layout === "wide") return { magnetic: false, band: null };
  if (layout === "double") return { magnetic: true, band: "two" };
  return { magnetic: true, band: "auto" };
}

/* ===========================================================================
   PROBLEM 267 — the middle-button ICON RING's three settings, as leaf data so
   `preview.ts` renders the identical pills and tiles the panel does.
   =========================================================================== */

/** "Middle button shows" — WHAT the middle button raises (owner, 2026-09-13:
 *  a choice, not a replacement). Mirrors Rust's `MiddleRingStyle`. */
export const MIDDLE_STYLE_OPTS: ReadonlyArray<readonly [string, string]> = [
  ["icon_ring", "Icon ring"],
  ["guide_hud", "Space ring"],
];
export type MiddleStyle = "icon_ring" | "guide_hud";
/** Config → pill. Absent = the icon ring (Rust's serde default). */
export function middleStyleFor(raw: unknown): MiddleStyle {
  return raw === "guide_hud" ? "guide_hud" : "icon_ring";
}

/** "Middle-button ring shows" — the favourites, or all (artboard 8; round 3
 *  naming: "Favourites / All"; the wire value `my_eight` is unchanged). */
export const MIDDLE_SCOPE_OPTS: ReadonlyArray<readonly [string, string]> = [
  ["my_eight", "Favourites"],
  ["all", "All"],
];
export type MiddleScope = "my_eight" | "all";
export function middleScopeFor(raw: unknown): MiddleScope {
  return raw === "all" ? "all" : "my_eight";
}

/** 2026-09-15 — how the "All" scope is arranged. "Spiral" is the owner's
 *  confirmed name for the phyllotaxis layout; use it verbatim. */
export const ALL_LAYOUT_OPTS: ReadonlyArray<readonly [string, string]> = [
  ["rings", "Rings"],
  ["spiral", "Spiral"],
];
export type AllLayout = "rings" | "spiral";
/** Config → pill. Absent = rings, which is what every install already draws. */
export function allLayoutFor(raw: unknown): AllLayout {
  return raw === "spiral" ? "spiral" : "rings";
}

/** TOUCHPAD T2 — the touchpad page's look. Mirrors Rust's `TouchpadLook`.
 *  Default Chocolate (the design's dark tokens with the theme's accent);
 *  "app" maps every surface onto the app's own theme variables. */
export const TOUCHPAD_LOOK_OPTS: ReadonlyArray<readonly [string, string]> = [
  ["app", "Matches the app"],
  ["chocolate", "Chocolate"],
];
export type TouchpadLookOpt = "app" | "chocolate";
/** Config → pill. Absent = chocolate (Rust's serde default). */
export function touchpadLookFor(raw: unknown): TouchpadLookOpt {
  return raw === "app" ? "app" : "chocolate";
}

/** The three-state control on every App-exceptions tile (artboard 8). Order
 *  and wording are the design's. Mirrors Rust's `ExceptionScope`. */
export const EXC_SCOPE_OPTS: ReadonlyArray<readonly [string, string]> = [
  ["off_entirely", "Off entirely"],
  ["space_only", "Space only"],
  ["middle_only", "Middle only"],
];
export type ExcScope = "off_entirely" | "space_only" | "middle_only";
export function excScopeFor(raw: unknown): ExcScope {
  return raw === "space_only" || raw === "middle_only" ? raw : "off_entirely";
}

/**
 * One App-exceptions row as the panel holds it. `get_config` returns objects
 * (Rust rewrites the pre-1.0.110 string form on its first save), but a
 * config that has not been saved since the upgrade still arrives as strings
 * — this is the ONE place the two shapes become one.
 */
export interface ExcRow { exe: string; scope: ExcScope }
export function normaliseExceptions(raw: unknown): ExcRow[] {
  if (!Array.isArray(raw)) return [];
  const out: ExcRow[] = [];
  for (const r of raw) {
    if (typeof r === "string") {
      const exe = r.toLowerCase();
      if (exe && !out.some((o) => o.exe === exe)) out.push({ exe, scope: "off_entirely" });
    } else if (r && typeof r === "object" && typeof (r as { exe?: unknown }).exe === "string") {
      const exe = (r as { exe: string }).exe.toLowerCase();
      if (exe && !out.some((o) => o.exe === exe)) {
        out.push({ exe, scope: excScopeFor((r as { scope?: unknown }).scope) });
      }
    }
  }
  return out;
}

/** The disclosure line under the "Choose your favourites" picker (artboard
 *  9), verbatim. Here, beside the option tables, for the same reason
 *  `SPECIALS_INERT_NOTE` is: the panel and the harness must print one string. */
export const EIGHT_PICKER_NOTE = "Site icons are fetched once, when you bind the link.";
/** The most favourites a user may tick — Rust's `FAVOURITES_MAX` (round 3). */
export const FAVOURITES_MAX = 15;
/** The favourites shown when none are chosen — Rust's `FAVOURITES_DEFAULT`:
 *  the first six bound letters, one full inner ring. */
export const FAVOURITES_DEFAULT = 6;

/**
 * The default favourites when none are chosen: the first six bound letters,
 * sorted — Rust's `favourites_for` for an empty list, mirrored so the picker
 * can show what the ring WILL do before anything is written. A stored list
 * is filtered to letters that are still bound, in its own order, up to
 * `FAVOURITES_MAX`.
 */
export function effectiveFavourites(
  stored: readonly string[] | undefined,
  boundLetters: readonly string[],
): string[] {
  const bound = boundLetters.map((c) => c.toLowerCase());
  const chosen: string[] = [];
  for (const raw of stored ?? []) {
    const c = raw.toLowerCase();
    if (bound.includes(c) && !chosen.includes(c)) chosen.push(c);
    if (chosen.length >= FAVOURITES_MAX) break;
  }
  if (chosen.length) return chosen;
  return [...bound].sort().slice(0, FAVOURITES_DEFAULT);
}

/**
 * THE INERT TREATMENT, shared by every settings row that another row can
 * switch off. One implementation, called from three places: the "Show special
 * keys" switch (greyed at 2 rows), the "Shortcut rows" pill (greyed at the
 * classic ring layout), and `preview.ts`, which renders both states without a
 * backend.
 *
 * IT LIVES HERE FOR THE SAME REASON THE TWO NOTES ABOVE DO. The harness and
 * the panel must show the identical dead control, and an inert treatment
 * copied into a second file drifts — usually by leaving `disabled` off, which
 * looks perfect and leaves the control fully operable from the keyboard.
 *
 * Four things, and every one of them is load-bearing:
 *
 *   · reduced opacity, so it READS as unavailable;
 *   · `pointer-events:none`, so the mouse cannot reach it;
 *   · `disabled` on every control inside the wrapper, because
 *     `pointer-events` does NOTHING for the keyboard — without this the row
 *     is still in the Tab order and still operable by Space/Enter, which is
 *     the half of "greyed out" that gets shipped broken;
 *   · the `.set-note` under the row shown, so there is a visible REASON.
 *
 * `.set-note` and not `.sma-note`: the latter is hidden unless "Show me
 * around" is on, which would hide the explanation from exactly the person who
 * just met a dead control. The row LABEL is deliberately OUTSIDE `wrap`, so
 * it stays live and pressing it still opens the description.
 *
 * Nothing here writes to any config. Passing `false` undoes all of it, which
 * is what makes the greying a presentation state and not a lost preference.
 */
export function paintInert(
  wrap: HTMLElement | null | undefined,
  note: HTMLElement | null | undefined,
  inert: boolean,
): void {
  if (wrap) {
    wrap.style.opacity = inert ? ".45" : "";
    wrap.style.pointerEvents = inert ? "none" : "";
    wrap
      .querySelectorAll<HTMLInputElement | HTMLButtonElement>("input, button")
      .forEach((el) => { el.disabled = inert; });
    wrap.closest<HTMLElement>(".set-row")?.setAttribute("aria-disabled", String(inert));
  }
  if (note) note.style.display = inert ? "" : "none";
}

// ---------------------------------------------------------------------------
// REVIEW FIXES 2026-09-05 (H6) — WHO OWNS "RUN AT STARTUP"
// ---------------------------------------------------------------------------
//
// THE BUG. `settings-panel.ts` decided the row's state with
// `packaged && !mayChange`, and read `packaged` as "this is a Microsoft Store
// install". It is not. `commands.rs::get_packaged_startup` answers the PORTABLE
// copy first, and it answers it with `(true, "portable", false, "<the shortcut
// note>")` — a `packaged: true` that is a lie told to reuse a tuple. Two
// consequences, and the second is the one that shipped:
//
//   1. the whole inert treatment for a portable copy depended on that lie. Any
//      future tidy-up of that tuple — an honest `packaged: false`, which is
//      what a reader would call correct — silently makes the row live again,
//      and a control that does nothing is the exact thing CLAUDE.md forbids.
//   2. the toggle path could still claim success. `set_startup_enabled` returns
//      `Ok(())` for a portable copy while `startup.rs::apply_task_enabled`
//      logs "PORTABLE — 'Run at startup' has nothing to apply to" and writes
//      NOTHING; the frontend's only guard was the same `packaged` read, so
//      switching OFF toasted "Won't start with Windows" — a completed action
//      announced for a write that never happened — and switching ON toasted
//      "Windows decides this one", which is wrong in a more confusing way:
//      Windows decides nothing here, the user unzipped a folder.
//
// THE FIX IS THESE FOUR PURE FUNCTIONS, and they live in this LEAF module for
// the same reason `paintInert` above does: `preview.ts` must be able to render
// and exercise the identical decision with no backend, and a copy of it in
// `settings-panel.ts` would drift the first time either was edited. The panel
// calls them; the harness calls them; there is one rule.
//
// The tuple's field NAMES are taken as documentation, not as truth. What the
// row actually needs to know is "may this app change it?" — `mayChange` — and
// "is it this app's own portability that is stopping it?" — the state string.
// Neither question needs `packaged` to mean anything.

/** The four values `commands.rs::get_packaged_startup` returns, named. */
export type StartupOwnership = {
  /** `true` for a Store package AND (misleadingly) for a portable copy. */
  packaged: boolean;
  /** Lower-cased `StartupTaskState`, e.g. "enabled", "disabledbyuser" — or
   *  the synthetic [`PORTABLE_STARTUP_STATE`] for an unzipped copy. */
  state: string;
  /** False whenever something other than this app decides the answer. */
  mayChange: boolean;
  /** The sentence shown under an inert row; "" when the row is live. */
  note: string;
} | null;

/**
 * The `state` `commands.rs` invents for the unzipped copy. It is not a real
 * `StartupTaskState`, which is exactly why it is safe to match on: no Windows
 * state will ever collide with it.
 */
export const PORTABLE_STARTUP_STATE = "portable";

/** Is this the unzipped, no-installer copy? */
export function startupIsPortable(p: StartupOwnership): boolean {
  return p?.state === PORTABLE_STARTUP_STATE;
}

/**
 * Should the row be greyed, with its note showing?
 *
 * **`!mayChange`, and deliberately NOT `packaged && !mayChange`.** That is the
 * whole correction: the question is "can this app change it", and the answer
 * comes from the field that says so. An unpackaged install answers
 * `(false, "unavailable", true, "")` → live, exactly as it always was; a Store
 * install Windows is holding → inert; the portable copy → inert, on its own
 * merits rather than on a `packaged: true` it should never have had to claim.
 *
 * `null` (the command has not answered yet, or does not exist in this build)
 * is LIVE, which is the same degrade path the row has always taken — but see
 * `settings-panel.ts`'s toggle handler, which now refuses to act on a `null`
 * until it has asked. A row that is live for the two seconds before the first
 * answer arrives is fine; a row that WRITES during those two seconds is not.
 */
export function startupRowIsInert(p: StartupOwnership): boolean {
  return !!p && !p.mayChange;
}

/**
 * Is the switch showing ON?
 *
 * For a packaged install this is what WINDOWS says, not what config says — the
 * two can differ the instant the user touches Task Manager, and a row rendered
 * from a stale config would confidently show the wrong thing.
 * `EnabledByPolicy` counts as on; every `disabled*` counts as off.
 *
 * A portable copy is always OFF, whatever its config holds. It has no Run key
 * and no Scheduled Task and never will (`startup.rs`), so `run_at_startup:
 * true` left behind by an installed copy whose config folder was carried over
 * would otherwise draw a switch that is on and inert and lying.
 */
export function startupShownAsOn(p: StartupOwnership, fromConfig: boolean): boolean {
  if (startupIsPortable(p)) return false;
  if (!p?.packaged) return fromConfig;
  return p.state === "enabled" || p.state === "enabledbypolicy";
}

/** What actually happened when the switch was used. */
export type StartupOutcome =
  /** The Run key / Scheduled Task / startupTask moved. Say so. */
  | "written"
  /** The command returned Ok and wrote nothing at all. Never claim success. */
  | "nothing-written"
  /** The app asked and Windows kept its own answer. */
  | "refused";

/**
 * Read the state AFTER `set_startup_enabled` and decide what may be claimed.
 *
 * THE POINT OF THIS FUNCTION is `"nothing-written"`. `set_startup_enabled`
 * returns `Ok(())` for a portable copy having written nothing, so a `try`
 * that did not throw is not evidence that anything happened — and "the toast
 * appeared, therefore it worked" is how a setting gets reported as working for
 * a build in which it cannot work.
 *
 * `p == null` is `"written"`, and that is not a shrug: it means the ownership
 * query is unavailable, which is the case for an ordinary NSIS or MSI install
 * talking to a build without the command — and in that case
 * `set_startup_enabled` genuinely did flip the HKCU Run value. Claiming
 * success there is correct; claiming it for the portable copy is not.
 */
export function startupOutcome(p: StartupOwnership, wanted: boolean): StartupOutcome {
  if (startupIsPortable(p)) return "nothing-written";
  if (p?.packaged && startupShownAsOn(p, wanted) !== wanted) return "refused";
  return "written";
}

/**
 * A 3-way segmented pill — the `theme` control's markup, extracted so a second
 * one can exist without a copy.
 *
 * It lives HERE and not in `settings-panel.ts` for the reason this whole file
 * exists: `preview.ts` renders the real markup, and importing `settings-panel`
 * would drag `main.ts`'s bootstrap into the dev harness and blank the page
 * (PROBLEM 148). The harness went on showing a "Dark mode" switch for three
 * versions after the theme pill replaced it; a pill the harness cannot draw is
 * the same failure waiting to happen again.
 *
 * `indicatorStyle` is how a pill picks its own indicator colour. The theme
 * pill's three segments are coloured per THEME by CSS
 * (`.theme-seg-ind[data-seg="warcry"]` and friends), which is meaningful only
 * for that control; any other pill passes a plain token instead —
 * `background:var(--st-accent)` — so it re-tints with the theme without a new
 * CSS rule or a new token.
 */
export function segRowHtml(
  group: string,
  opts: ReadonlyArray<readonly [string, string]>,
  value: string,
  indicatorStyle = "",
  ariaLabel = group,
  describedBy = "",
): string {
  const idx = Math.max(0, opts.findIndex(([v]) => v === value));
  const style = indicatorStyle ? ` style="${indicatorStyle}"` : "";
  const desc = describedBy ? ` aria-describedby="${describedBy}"` : "";
  // PROBLEM 255 follow-up — no `--seg-i`/`--seg-n` here any more. Those drove
  // the indicator by ARITHMETIC (`i * 100%` of an assumed equal-fraction
  // width), which is exactly what broke once segments started sizing to
  // their own content instead of `1fr`. Positioning is now MEASURED after
  // insertion — `positionSegIndicator`/`wireSegIndicators` below read the
  // active button's real `offsetLeft`/`offsetWidth` and write `--ind-x`/
  // `--ind-w` onto this container (styles.css reads those). This markup
  // just needs `.is-on` on the right button, which it already sets.
  return `
    <div class="theme-seg" role="radiogroup" aria-label="${ariaLabel}"${desc}>
      <span class="theme-seg-ind" data-seg="${opts[idx][0]}"${style}></span>
      ${opts
        .map(
          ([v, l], n) => `<button type="button" class="theme-seg-opt${n === idx ? " is-on" : ""}"
                 data-${group}-set="${v}" role="radio"
                 aria-checked="${n === idx}">${l}</button>`,
        )
        .join("")}
    </div>`;
}

/**
 * Measures the CURRENTLY ACTIVE segment inside one `.theme-seg` container and
 * writes its real box onto the container as `--ind-x`/`--ind-w` (both px),
 * which `.theme-seg-ind` (styles.css) reads for its `transform`/`width`.
 *
 * This is the fix for PROBLEM 255's follow-up bug: the indicator used to be
 * positioned by ARITHMETIC (`--seg-i` × an assumed `100% / --seg-n` width),
 * which is only correct when every segment is the same width. The moment
 * segments started sizing to their own label ("Starry night" is nearly twice
 * "Auto"), that arithmetic put the indicator over the WRONG rect — covering
 * neighbouring text instead of matching its own label. Reading the button's
 * own `offsetLeft`/`offsetWidth` is correct for any label set, any font size
 * step (the compact-popover shrink in styles.css included), and any segment
 * count, because it asks the browser what actually got laid out instead of
 * predicting it.
 */
export function positionSegIndicator(seg: HTMLElement): void {
  const on = seg.querySelector<HTMLElement>(".theme-seg-opt.is-on");
  if (!on) return;
  seg.style.setProperty("--ind-x", `${on.offsetLeft}px`);
  seg.style.setProperty("--ind-w", `${on.offsetWidth}px`);
}

/** Re-measures every segmented pill currently in the document. Used as the
 *  resize/font-load callback below, where the pill that needs re-measuring
 *  may not be the one that was passed to `wireSegIndicators` originally
 *  (settings-panel.ts re-renders new DOM into the same panel element). */
function positionAllSegIndicators(): void {
  document.querySelectorAll<HTMLElement>(".theme-seg").forEach(positionSegIndicator);
}

let _segResizeObserver: ResizeObserver | null = null;
let _segFontWired = false;

/**
 * Wires the measured-indicator machinery for every `.theme-seg` under `root`:
 * an immediate position (so the indicator is correct on first paint, before
 * any interaction), a `ResizeObserver` per pill so it re-measures when its
 * OWN box changes size — the popover (280px) vs the expanded panel is exactly
 * this, see `#settings-panel:not(.expanded) .theme-seg-opt` in styles.css —
 * and a one-time `document.fonts.ready` hook, because a label's real width
 * before its font finishes loading is a guess, and a wrong guess here is the
 * whole class of bug this feature exists to fix.
 *
 * Safe to call on every render, like `wireSegRowsKeyboard` above: it re-finds
 * the pills fresh each time. The `ResizeObserver` is disconnected and
 * recreated rather than accumulated, so repeated renders never stack
 * observers on detached elements; `document.fonts.ready` is wired only once
 * per document since the promise itself only ever resolves once.
 */
export function wireSegIndicators(root: ParentNode): void {
  const groups = Array.from(root.querySelectorAll<HTMLElement>(".theme-seg"));
  groups.forEach(positionSegIndicator);

  if (typeof ResizeObserver !== "undefined") {
    _segResizeObserver?.disconnect();
    _segResizeObserver = new ResizeObserver(positionAllSegIndicators);
    groups.forEach((g) => _segResizeObserver!.observe(g));
  }

  if (!_segFontWired && typeof document !== "undefined" && document.fonts) {
    _segFontWired = true;
    document.fonts.ready.then(positionAllSegIndicators).catch(() => {});
  }
}

/**
 * ARROW-KEY NAVIGATION for a segmented pill (ARIA radiogroup pattern,
 * accessibility pass). A native `<input type="radio">` group gets Left/Right
 * moving the selection from the browser for free; `role="radio"` on a
 * `<button>` does not, so without this the pill was reachable by Tab and
 * activatable by Space/Enter but had no arrow-key behaviour at all.
 *
 * Left/Up moves to the previous segment, Right/Down to the next, wrapping at
 * the ends; Home/End jump to the first/last. The focused segment is CLICKED,
 * not just focused — the click handlers that actually write the value are
 * already wired elsewhere (settings-panel.ts, preview.ts), and dispatching a
 * real click through them is what keeps this from becoming a second path
 * that can drift from the first.
 *
 * Deliberately NOT a roving tabindex (the fuller version of this ARIA
 * pattern): every segment stays an ordinary Tab stop. `render()` rebuilds
 * this panel's whole subtree after most changes, and a roving tabindex kept
 * correct only by re-reading `.is-on` at render time is one more place for a
 * repaint to leave stale state; multiple Tab stops per pill is the more
 * forgiving trade for two 3–4 option pills in a small panel.
 *
 * Safe to call on every render: it is scoped to `root` and re-finds the
 * buttons fresh each time, so re-wiring costs nothing beyond what `render()`
 * already pays to rebuild the DOM.
 */
export function wireSegRowsKeyboard(root: ParentNode): void {
  root.querySelectorAll<HTMLElement>(".theme-seg").forEach((group) => {
    const opts = Array.from(group.querySelectorAll<HTMLButtonElement>(".theme-seg-opt"));
    opts.forEach((btn, i) => {
      btn.addEventListener("keydown", (e) => {
        let next = -1;
        if (e.key === "ArrowRight" || e.key === "ArrowDown") next = (i + 1) % opts.length;
        else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = (i - 1 + opts.length) % opts.length;
        else if (e.key === "Home") next = 0;
        else if (e.key === "End") next = opts.length - 1;
        else return;
        e.preventDefault();
        opts[next].focus();
        opts[next].click();
      });
    });
  });
}

/**
 * The switch itself. `preview.ts` renders this same function, so the dev
 * harness can never drift from the app — it went on showing a "Dark mode"
 * switch for three versions after the theme pill replaced it.
 */
/**
 * `ariaLabel` and `describedBy` — accessibility pass. The visible row text
 * lives on a SEPARATE `<button data-desc>` beside this switch (press-to-expand
 * needs the text to mean "explain this" and only the switch to mean "change
 * this" — see the PROBLEM 144 note on `toggleRow` in settings-panel.ts), so
 * without an explicit label the checkbox itself has no accessible name at
 * all: a screen reader landing on it by Tab announces nothing but "checkbox".
 * `role="switch"` overrides the implicit `checkbox` role with the one that
 * actually matches what this control does (an immediate on/off, not a form
 * selection) — supported by every screen reader this app has to answer to,
 * and `aria-checked` is set explicitly because overriding the role away from
 * `checkbox` is exactly the case where a browser's automatic mirroring of the
 * native `checked` property cannot be assumed.
 */
export function toggleSwitchHtml(
  id: string, on: boolean, anim?: "on" | "off",
  ariaLabel = "", describedBy = "",
): string {
  const label = ariaLabel ? ` aria-label="${ariaLabel}"` : "";
  const desc = describedBy ? ` aria-describedby="${describedBy}"` : "";
  return `
    <span class="toggle-switch" data-char="${toggleChar(id)}"${anim ? ` data-anim="${anim}"` : ""}>
      <input type="checkbox" id="set-${id}" role="switch" aria-checked="${on}"${label}${desc} ${on ? "checked" : ""} />
      <label class="toggle-track" for="set-${id}">
        <span class="toggle-thumb"><i class="toggle-flame"></i></span>
        <span class="toggle-ring"></span>
      </label>
    </span>`;
}

/**
 * Which character each slider performs when Fun mode is on (spec §3).
 * Same shape as TOGGLE_CHAR: the row decides nothing, the id does.
 */
const SLIDER_CHAR: Record<string, string> = {
  wpm: "comet", huddelay: "planet", opacity: "starfield",
};

/**
 * Wraps a native range in the decoration shell (styles/characters.css §3).
 *
 * The input stays a real `<input type="range">` — arrow keys, Home/End, the
 * screen-reader value and every existing input/change listener keep working.
 * The wrapper only carries `--p` (the value as 0..1), which is what positions
 * the comet's tail, the planet's orbit ring and the fill of every track. That
 * is also why the decorations can be pure CSS: nothing has to measure the DOM.
 */
export function sliderShell(id: string, input: string, min: number, max: number, value: number): string {
  const p = max > min ? (value - min) / (max - min) : 0;
  const extra = SLIDER_CHAR[id] === "starfield"
    ? '<i class="sld-star"></i><i class="sld-star"></i><i class="sld-star"></i>'
    : SLIDER_CHAR[id] === "planet" ? '<i class="sld-orbit"></i>'
    : '<i class="sld-tail"></i>';
  return `
    <span class="sld" data-char="${SLIDER_CHAR[id] ?? "comet"}" data-dir="1"
          id="sld-${id}" style="--p:${p.toFixed(4)}">${input}${extra}</span>`;
}

// ---------------------------------------------------------------------------
// GROUP HEADINGS (owner's 2026-09-01 redesign)
// ---------------------------------------------------------------------------

/**
 * The four group icons, as INLINE SVG rather than characters.
 *
 * No icon library — that rule is not up for discussion here (no CDN, nothing
 * external, and the app already has a hand-drawn vocabulary). Characters were
 * the other candidate and were rejected on measurement grounds: the app
 * bundles its own fonts (`src/assets/fonts/`), so a glyph like ◐ or ⏻ either
 * exists in that face or silently falls back to whatever Windows substitutes,
 * at a different weight and baseline from the row beside it. A 13px stroked
 * path cannot fall back to anything.
 *
 * `currentColor` throughout, so each heading's icon is the heading's colour in
 * all three themes for free, and `stroke-width:1.6` matches the app's other
 * hairlines (the 1.5px borders on .key and .input).
 */
const GROUP_ICONS: Record<string, string> = {
  // Appearance — a disc half-filled: the light/dark of a look.
  appearance:
    '<circle cx="7" cy="7" r="5.4"/><path d="M7 1.6a5.4 5.4 0 0 0 0 10.8z" fill="currentColor" stroke="none"/>',
  // Behaviour — two sliders: the settings that change how the app acts.
  behaviour:
    '<path d="M2 4.5h10M2 9.5h10"/><circle cx="5" cy="4.5" r="1.6" fill="currentColor"/><circle cx="9.5" cy="9.5" r="1.6" fill="currentColor"/>',
  // The Space ring — a ring with a chip on it, which is literally the HUD.
  ring:
    '<circle cx="7" cy="7" r="5"/><circle cx="7" cy="2" r="1.5" fill="currentColor" stroke="none"/>',
  // Privacy — a padlock, the one universally-read symbol for "kept in".
  privacy:
    '<rect x="2.6" y="6" width="8.8" height="6" rx="1.8"/><path d="M4.8 6V4.4a2.2 2.2 0 0 1 4.4 0V6"/>',
  // Maintenance — an arrow going round: re-check, re-open, re-run. Nothing
  // here changes a setting, which is the whole reason it is a separate group
  // from the one below it.
  maintenance:
    '<path d="M11.6 7a4.6 4.6 0 1 1-1.35-3.25"/><path d="M11.9 1.9v2.9H9"/>',
  // Danger zone — the warning triangle, and it is the ONLY icon in this table
  // that carries a non-colour cue for the same meaning its tint carries. A
  // grayscale screenshot, or a colour-blind reader, still sees which group
  // asks for a second thought (design-system.md: semantic colour always needs
  // a second cue).
  danger:
    '<path d="M7 1.9 12.6 11.6H1.4z"/><path d="M7 5.6v2.6"/><circle cx="7" cy="10" r=".7" fill="currentColor" stroke="none"/>',
  // About — a plain "i" in a circle. The one heading icon here that is not
  // trying to say anything clever: this group is reference information
  // (version, install kind, links), not a preference.
  about:
    '<circle cx="7" cy="7" r="5.4"/><path d="M7 6.4v3.4" stroke-linecap="round"/><circle cx="7" cy="4.3" r=".9" fill="currentColor" stroke="none"/>',
  // 1.0.119 (brief 4 §6) — "For power users": a bolt.
  power: '<path d="M7.8 1.4 3.4 7.7h3.1l-.9 4.9 4.6-6.4H7z"/>',
};

/**
 * A group heading: small uppercase label with its icon.
 *
 * It is a plain `<div>`, NOT a button: pressing a heading must not do
 * anything, because every OTHER piece of text in this panel that looks like
 * this (`.set-row-label`) opens a description when pressed. A heading that
 * looked identical and did nothing would teach the user that pressing labels
 * is unreliable.
 */
export function groupHeadingHtml(icon: keyof typeof GROUP_ICONS | string, label: string): string {
  const path = GROUP_ICONS[icon] ?? "";
  return `
    <div class="set-group-head">
      <svg class="set-group-icon" viewBox="0 0 14 14" width="13" height="13"
           fill="none" stroke="currentColor" stroke-width="1.6"
           stroke-linecap="round" stroke-linejoin="round"
           aria-hidden="true" focusable="false">${path}</svg>
      <span>${label}</span>
    </div>`;
}

// ---------------------------------------------------------------------------
// SEARCH (owner's 2026-09-01 redesign)
// ---------------------------------------------------------------------------

/**
 * Filter the panel's rows in place. Returns how many are still showing.
 *
 * ZERO COST WHEN UNUSED, and that is a design constraint, not a nicety: this
 * panel re-renders after every toggle, and anything that walked the DOM to
 * build a search index at render time would pay for a feature nobody had
 * asked for yet. So there is no index. The haystack for a row is its own
 * `textContent` — LABEL PLUS DESCRIPTION, which is the whole point (a user
 * who searches "crash" must find "Don't send logs", whose label says neither
 * word) — computed the first time that row is actually filtered and cached on
 * the element in `dataset.search`. An empty query does nothing but clear
 * `hidden`, so backspacing to nothing costs one pass and no string work.
 *
 * `hidden` and not a class: `.set-item[hidden]`/`.set-group[hidden]` have
 * their own `display:none` companions in styles.css, because a class rule
 * that sets `display` beats the UA's `[hidden]` — the trap this stylesheet
 * already records for #profile-popover.
 *
 * The engine row (`.set-engine`) is deliberately NOT `.set-filterable`, so
 * this function never touches it. It is the one row pinned sticky so the
 * main on/off switch stays reachable at any scroll depth — 1.0.96 review fix:
 * being filterable meant a query that didn't match it hid the one row that
 * was supposed to always be reachable, defeating the reason it is pinned.
 */
export function filterSettings(root: HTMLElement, query: string): number {
  const q = query.trim().toLowerCase();
  let shown = 0;

  root.querySelectorAll<HTMLElement>(".set-filterable").forEach((row) => {
    if (!q) {
      row.hidden = false;
      shown++;
      return;
    }
    let hay = row.dataset.search;
    if (hay === undefined) {
      hay = (row.textContent ?? "").toLowerCase().replace(/\s+/g, " ");
      row.dataset.search = hay;
    }
    const hit = hay.includes(q);
    row.hidden = !hit;
    if (hit) shown++;
  });

  // A heading with nothing under it is worse than no heading — it reads as a
  // group whose contents failed to load. Hide the whole group instead.
  root.querySelectorAll<HTMLElement>(".set-group").forEach((group) => {
    const any = Array.from(group.querySelectorAll<HTMLElement>(".set-filterable"))
      .some((row) => !row.hidden);
    group.hidden = !any;
  });

  const empty = root.querySelector<HTMLElement>("#set-search-empty");
  if (empty) empty.hidden = !q || shown > 0;
  return shown;
}

// ---------------------------------------------------------------------------
// EXPAND (owner's 2026-09-01 redesign)
// ---------------------------------------------------------------------------

/**
 * The full-screen takeover, as a CLASS ON THE PANEL ITSELF.
 *
 * WHY NOT SKY MODE'S CODE PATH, which is the other stage takeover this app
 * has. `applySkyMode` hides every child of #stage except #gear-dock and the
 * return arrow, writes `hide_keyboard` to the config, and — critically —
 * CLOSES THIS PANEL on the way in (settings-panel.ts's own
 * `closeSettingsPanel`, called from applySkyMode). Reusing it to show the
 * panel bigger would mean the takeover's first act is to close the thing
 * being taken over. It also persists, and an expand is a look-at-it-now
 * gesture, not a preference.
 *
 * So this is the sky-mode *pattern* (one class, one surface grows to fill the
 * stage, Esc returns) on a different element, and nothing about it is
 * persisted. #settings-panel keeps its identity, which is what makes the
 * expand free: main.ts already stops click propagation on this element
 * (PROBLEM 98), so an expanded panel keeps surviving clicks inside itself and
 * closing on clicks outside without a single line in main.ts.
 */
export function setPanelExpanded(panel: HTMLElement, on: boolean): void {
  panel.classList.toggle("expanded", on);
  // PROBLEM 255 follow-up — this is the ONE deterministic place both segmented
  // pills' container width actually changes (280px popover vs the full-width
  // expanded panel; see `#settings-panel:not(.expanded) .theme-seg-opt` in
  // styles.css for the font-size half of this). It is called directly here
  // rather than trusted to the `ResizeObserver` set up in `wireSegIndicators`:
  // measured in the browser pane, a `ResizeObserver` on a BACKGROUNDED/hidden
  // page can go arbitrarily long without firing (Chromium throttles it with
  // the rest of the rAF family), and this class toggle is a same-tick,
  // synchronous width change — `width: auto` is set immediately, nothing
  // animates the number itself (only `st-pop-in`'s transform/opacity does) —
  // so reading `offsetLeft`/`offsetWidth` right after the toggle is already
  // correct. The observer stays as a second line of defence for resizes this
  // function does not cause (window/DPI changes).
  positionAllSegIndicators();
  // Marked on <body> too, so the gear button underneath can be dimmed out of
  // the way — it is the one control that would otherwise sit ON TOP of the
  // full-screen panel it opened.
  document.body.classList.toggle("settings-expanded", on);
  const btn = panel.querySelector<HTMLElement>("#set-expand");
  btn?.setAttribute("aria-pressed", String(on));
  btn?.setAttribute("aria-label", on ? "Shrink settings" : "Expand settings");
  btn?.setAttribute("title", on ? "Back to the small panel (Esc)" : "Fill the window");
}

// ---------------------------------------------------------------------------
// THE RING PREVIEW'S VISIBILITY, and why it lives HERE
// ---------------------------------------------------------------------------

/**
 * How long a `preview_hud_layout` projection stays on screen.
 *
 * **MIRRORS `PREVIEW_MS` in `src-tauri/src/guide_hud/mod_impl.rs`, which is
 * 4000.** There is no event to listen to: `hide_preview_hud` calls
 * `hide_guide_hud_pending(false)` and emits nothing the dashboard can hear —
 * grepped 2026-09-04, and the only signal in that file is the log line. So the
 * frontend's only honest option is to mirror the number and say out loud that
 * it is mirrored. If that constant ever changes, this one has to change with
 * it; a stale value here does not break anything, it just makes the re-fire
 * window disagree with the ring by the size of the drift.
 *
 * The window is a CEILING on the Rust side too — a real Space-hold or a newer
 * preview supersedes it early through the epoch counter — so this clock can be
 * optimistic (think a preview is up when the user's Space-hold already took
 * it) but never the reverse. Optimistic is the safe direction: the worst case
 * is one re-fire that Rust's own epoch discipline then resolves.
 */
export const RING_PREVIEW_MS = 4000;

/**
 * When the projection this app last raised is due to take itself down.
 *
 * Module scope, so it survives `settings-panel.ts`'s `render()` — which
 * rebuilds the whole panel after most toggles and would wipe any state kept
 * on an element.
 */
let _ringPreviewUntil = 0;

/** Is a projection this app raised still on screen? */
export function isRingPreviewShowing(now: number = Date.now()): boolean {
  return now < _ringPreviewUntil;
}

/**
 * Forget any projection — used when the panel closes and by the harness
 * between measurements. Never fires a command.
 */
export function clearRingPreview(): void {
  _ringPreviewUntil = 0;
}

/**
 * Raise the projection, and remember that it is up.
 *
 * EVERY FAILURE IS SWALLOWED, on purpose and permanently. `preview_hud_layout`
 * is built in a different lane; a settings panel that threw — or that left a
 * pill un-moved — because a preview command was missing would be a worse
 * outcome than no preview at all. The caller has already applied and saved the
 * value by the time this runs, so the preview is decoration on a change that
 * has already happened.
 *
 * A failure CLEARS the window rather than leaving it. If the command is not in
 * this build, no ring is on screen, and a later toggle must not conclude that
 * one is.
 */
export async function showRingPreview(layout: RingLayout): Promise<void> {
  try {
    await invoke("preview_hud_layout", { layout });
    _ringPreviewUntil = Date.now() + RING_PREVIEW_MS;
  } catch (_) {
    _ringPreviewUntil = 0;
  }
}

/**
 * Re-fire the projection **only if one is already on screen** (owner,
 * 2026-09-04).
 *
 * The bug: tap Compact/Wide/Double, the ring appears, then flip "Show special
 * keys" while it is still up — and the ring goes on showing the OLD payload,
 * because Rust builds the projection once at `preview_hud_layout` time and
 * nothing re-sends it. The change only appeared the next time a preview was
 * raised.
 *
 * The guard is the whole feature. A toggle pressed with no ring on screen must
 * NOT surprise-launch one: the user asked to change a setting, not to be shown
 * a full-screen overlay. So this returns `false` and fires nothing unless the
 * clock says a projection this app raised is still up.
 *
 * Returns whether it re-fired, which is what makes it testable.
 */
export async function refreshRingPreview(layout: RingLayout): Promise<boolean> {
  if (!isRingPreviewShowing()) return false;
  await showRingPreview(layout);
  return true;
}

// ---------------------------------------------------------------------------
// ABOUT (owner's decided feature 1) — a LEAF module for the same reason every
// other piece of settings markup above lives here: `settings-panel.ts` wires
// the real backend calls, `preview.ts` renders the identical markup off stub
// data, and neither may fork a second copy that drifts (PROBLEM 148).
// ---------------------------------------------------------------------------

/** One row of `src/generated/third-party.json`. */
export interface ThirdPartyEntry {
  name: string;
  version: string;
  license: string;
  url: string;
}

const GITHUB_URL = "https://github.com/nur-arpon/Spaceadom";
/**
 * PROBLEM 256 — the button label used to just say "Licence", which reads as
 * neutral about WHICH licence. The repo's `LICENSE` file itself was rewritten
 * from MIT to the source-visible/proprietary text on 2026-09-05, but this
 * link still points at GitHub, and the tree has been UNCOMMITTED since
 * 1.0.95 — so until the next push, `github.com/.../blob/main/LICENSE` keeps
 * serving the OLD MIT file underneath this button. The label now states the
 * real licence directly so a reader is not depending on that page being
 * current; the link stays pointed at `LICENSE` for anyone who wants the full
 * text once a commit catches the file up.
 */
const ABOUT_LINKS = {
  github: GITHUB_URL,
  issues: `${GITHUB_URL}/issues`,
  privacy: `${GITHUB_URL}/blob/main/PRIVACY.md`,
  licence: `${GITHUB_URL}/blob/main/LICENSE`,
} as const;
export type AboutLinkKind = keyof typeof ABOUT_LINKS;

/**
 * Opens one of the About section's four links.
 *
 * THREE of them are the system browser, via `openUrl` — GitHub, Privacy
 * policy, Licence. "Report a problem" is the ONE exception (PROBLEM 253 — the
 * "251" this comment used to hedge about is gone from the tree; see the import
 * comment above), landed in a parallel lane while this one was in progress: it opens the
 * in-app report dialog instead of a bare Issues link, because that dialog
 * builds a diagnostic zip on this machine and lets the user decide whether to
 * attach it to what they file, rather than sending them to a blank GitHub
 * form with nothing useful pasted in. `report-dialog.ts` is a leaf module for
 * exactly this reason — this file can call it without dragging `main.ts`
 * into `preview.ts`'s harness, the same way it already imports `invoke`.
 *
 * EVERY FAILURE IS SWALLOWED for the `openUrl` branch, same rule as
 * `showRingPreview` above: a webview has no window to fall back to, so a
 * rejected `openUrl` (missing `opener:default` capability on some future
 * window, the harness's absent Tauri runtime) must not throw past a click
 * handler. The caller may still tell the user it failed — `settings-panel.ts`
 * does — but this function itself never does. `openReportDialog()` is
 * synchronous and cannot fail the same way (it only ever builds DOM).
 */
export async function openAboutLink(kind: AboutLinkKind): Promise<void> {
  if (kind === "issues") {
    openReportDialog();
    return;
  }
  await openUrl(ABOUT_LINKS[kind]);
}

/**
 * `(version, installKind)`, or `null` if neither source could answer.
 *
 * TWO SOURCES, in order of preference, and BOTH are allowed to be missing —
 * this is reference information, never something worth a fatal error over:
 *
 *   1. `get_about_info` (commands.rs) — the real answer, including the
 *      install kind (setup.exe / .msi / Microsoft Store / a dev build with
 *      neither). It may not exist in an older build, or in one from a lane
 *      that has not landed it yet.
 *   2. `@tauri-apps/api/app`'s `getVersion()` — every Tauri build has had
 *      this since 1.0, so it is the floor: at minimum the version still
 *      shows, with no install-kind line, rather than the row showing nothing
 *      at all because the newer command was not there yet.
 *
 * Returns `null` only when NEITHER source answers — the dev harness with no
 * Tauri runtime behind it, where the row simply falls back to the bare app
 * name (see `aboutRowHtml`).
 */
export async function fetchAboutInfo(): Promise<{ version: string; installKind: string } | null> {
  try {
    const [version, installKind] = await invoke<[string, string, string]>("get_about_info");
    return { version, installKind };
  } catch (_) {
    try {
      return { version: await getVersion(), installKind: "" };
    } catch (_e) {
      return null;
    }
  }
}

/**
 * Runs a manual update check and returns the sentence to show under the
 * button — never throws.
 *
 * `check_for_updates_now` (another lane's work, landing in parallel with this
 * one) both RETURNS an `UpdateStatus` and emits one over the `update-status`
 * event as the check progresses; this only reads the return value; the CALLER
 * is responsible for also listening to the event so a check that takes a
 * moment (download, install) updates the same line as it goes rather than
 * appearing to hang. If the command does not exist in this build yet, the
 * fallback sentence says so plainly rather than showing a raw error.
 */
export async function requestUpdateCheck(): Promise<string> {
  try {
    const status = await invoke<{ message?: string }>("check_for_updates_now");
    return status?.message || "Checked — nothing to report.";
  } catch (_) {
    return "Coming in the next build.";
  }
}

/**
 * The `update-status` states, from `updater.rs`'s `STATE_*` constants.
 *
 * The UI contract that goes with them, stated once here because both this
 * file and `settings-panel.ts` depend on it: **switch on the states you
 * handle, and show `message` verbatim for anything else.** `message` is
 * always a finished English sentence, never a code and never a fragment, so
 * a state added in a later build can never produce a blank panel.
 */
export const UPDATE_STATES = {
  checking: "checking",
  upToDate: "up_to_date",
  downloading: "downloading",
  installing: "installing",
  error: "error",
  busy: "busy",
  notEligible: "not_eligible",
} as const;

/**
 * **Is this state one the app can still come back from?**
 *
 * `checking`, `downloading` and `installing` are all "still going"; the other
 * four are terminal. It matters for exactly one thing — whether the "Check
 * for updates" button may be pressed again — and it is a named function
 * rather than an inline test because of what happens on the way out:
 *
 * **THE NSIS LEG NEVER RETURNS.** `check_for_updates_now`'s promise does not
 * resolve when an update actually installs: PROBLEM 233's sequence is stop
 * the hook, run Tauri's exit cleanup, spawn the installer, exit — so the
 * process is gone before any `.then()` runs, and a `finally` that re-enables
 * the button never executes. A UI that drives its own state from the
 * command's RETURN is therefore correct only in the case where nothing
 * happened. Driving it from the EVENT instead is correct in both, which is
 * why `emit_always` guarantees a terminal status once `downloading` has been
 * emitted (a daily check that dies mid-download used to be free to say
 * nothing; leaving a progress line at 60% forever is PROBLEM 135's hidden
 * window in a different costume).
 */
export function updateStateIsBusy(state: string | undefined): boolean {
  return (
    state === UPDATE_STATES.checking ||
    state === UPDATE_STATES.downloading ||
    state === UPDATE_STATES.installing
  );
}

/** What `rollback_available` returns when there IS a way back. */
export type RollbackTarget = { version: string; path: string; kind: string };

/**
 * The previous version's archived installer, or `null`.
 *
 * `null` is the answer on a dev build, on a Store package, on a portable
 * copy, and — the one worth knowing — after the FIRST auto-update this
 * machine ever takes: the copy being left was installed by hand and its
 * installer was never ours to keep. So a rollback button that is missing is
 * usually not a fault, and the row says nothing at all rather than showing a
 * dead control (CLAUDE.md: a control that does nothing is worse than a
 * missing control).
 *
 * Never throws: an older build has no such command.
 */
export async function fetchRollbackTarget(): Promise<RollbackTarget | null> {
  try {
    return (await invoke<RollbackTarget | null>("rollback_available")) ?? null;
  } catch (_) {
    return null;
  }
}

/**
 * Go back to the previous version. Returns the sentence to show.
 *
 * **THIS NORMALLY DOES NOT RETURN AT ALL** — same reason as
 * `updateStateIsBusy` above: on the NSIS leg the process exits so its
 * installer can replace the exe. A resolved promise here means the rollback
 * did NOT happen, and Rust's `Err` string is the explanation. So the caller
 * must paint its "starting…" state BEFORE the call and treat a return as
 * news, not as completion.
 */
export async function requestRollback(): Promise<string> {
  try {
    const msg = await invoke<string>("rollback_to_previous");
    return msg || "Going back to the previous version…";
  } catch (e) {
    return String(e) || "Couldn't go back to the previous version.";
  }
}

/**
 * Group third-party entries by licence and render the collapsible list's
 * INNER markup (the caller owns the show/hide wrapper).
 *
 * Grouped and counted per licence, as specified — a flat list of several
 * hundred packages is not itself information; "how many entries wear which
 * licence" is what a reader actually wants from this section. Sorted by group
 * SIZE (largest first) so the handful of licences that cover almost every
 * dependency (MIT, Apache-2.0/MIT dual) sit at the top, with names inside each
 * group sorted alphabetically so a specific package can be scanned for.
 *
 * PURE MARKUP, no DOM, no lazy-render state — that lives in the caller
 * (`settings-panel.ts` renders this into the list box only on first expand,
 * per the owner's "lazy-rendered on expand" instruction), so this function
 * itself has nothing to get wrong about timing.
 */
/**
 * REVIEW FIXES 2026-09-05 (LOW) — HTML-escape anything interpolated into an
 * `innerHTML` template in this file.
 *
 * WHY, given that every value here "comes from us". Two of them do not, and
 * the distinction is not one a reader can check at a glance:
 *
 *   - `updateStatusText` is `updater.rs`'s `message` field, arriving over a
 *     Tauri event. `settings-panel.ts`'s listener has an explicit contract to
 *     print it VERBATIM for any state it does not recognise, which is exactly
 *     the property that makes an unescaped interpolation load-bearing: the
 *     next state added upstream is printed here without anyone reading it.
 *   - `rollbackVersion` is a version string `updater.rs` reads off a directory
 *     name on disk, and a directory name is user-writable.
 *
 * The other two — `version` from `package_info()`, `installKind` from a fixed
 * set of sentences — are safe today. They are escaped anyway, because "safe
 * today" is a property of the CURRENT producer and this is a template that
 * outlives it, and because a template with three escaped holes and two bare
 * ones invites the next person to copy the wrong one.
 *
 * `renderThirdPartyGroups` gets the same treatment: `third-party.json` is
 * generated at build time from the dependency tree, so its `name`, `url`,
 * `version` and `license` strings are written by hundreds of package authors,
 * not by this project. The `url` needs it most — it lands in an `href` AND in
 * a `data-tp-link` the click handler feeds straight to `openUrl`, so a `"` in
 * it would break out of both attributes at once.
 *
 * DELIBERATELY A LOCAL COPY of `key-detail-panel.ts`'s identical function
 * rather than an import: this file is a LEAF module (see the header — it is
 * why `preview.ts` can render these rows at all), and importing from
 * `key-detail-panel.ts` would drag the whole key editor into the harness and
 * into any future bundle that wanted one settings row. Five characters, two
 * copies, both trivially checkable; that trade is the same one `theme-resolve.ts`
 * documents from the other direction.
 */
function escapeHtml(s: string): string {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!,
  );
}

export function renderThirdPartyGroups(entries: readonly ThirdPartyEntry[]): string {
  const groups = new Map<string, ThirdPartyEntry[]>();
  for (const e of entries) {
    const list = groups.get(e.license);
    if (list) list.push(e); else groups.set(e.license, [e]);
  }
  const licences = Array.from(groups.keys()).sort(
    (a, b) => groups.get(b)!.length - groups.get(a)!.length,
  );
  return licences
    .map((lic) => {
      const items = groups.get(lic)!.slice().sort((a, b) => a.name.localeCompare(b.name));
      return `
        <div class="set-about-tp-group">
          <div class="set-about-tp-license">${escapeHtml(lic)} <span class="set-about-tp-count">· ${items.length}</span></div>
          <ul class="set-about-tp-items">
            ${items
              .map(
                (e) => `<li><a href="${escapeHtml(e.url)}" data-tp-link="${escapeHtml(e.url)}">${escapeHtml(e.name)}</a> <span class="set-about-tp-ver">${escapeHtml(e.version)}</span></li>`,
              )
              .join("")}
          </ul>
        </div>`;
    })
    .join("");
}

/**
 * The whole ABOUT row's markup. `info === null` is the dev-harness case (no
 * Tauri runtime to answer either source `fetchAboutInfo` tries) and shows the
 * bare app name with no version — never a blank row, per CLAUDE.md's "a
 * control that does nothing is worse than a missing control" read onto
 * information instead of a control: a row that shows NOTHING reads as broken,
 * not as "the answer is unavailable right now".
 *
 * `.set-filterable` on the wrapper — the owner's instruction that search must
 * find the About rows — is applied by the CALLER (it wraps this markup in the
 * same `.set-item.set-filterable` every other row in this panel uses), not
 * here, so this stays pure markup with no opinion about the panel around it.
 */
/**
 * `rollbackVersion` (PROBLEM 249) is the version this copy can go BACK to, or
 * `null`. Null renders no button at all rather than a disabled one: there is
 * nothing the user could do to make a way back appear, and a permanently
 * greyed control in an About box is a question with no answer. The version is
 * IN THE LABEL — "Roll back to 1.0.99", never a bare "Roll back" — because
 * this is the one button in the panel whose consequence is a different
 * program, and a person about to press it is entitled to know which one they
 * are going to end up with.
 */
export function aboutRowHtml(
  info: { version: string; installKind: string } | null,
  thirdPartyCount: number,
  thirdPartyOpen: boolean,
  updateStatusText = "",
  rollbackVersion: string | null = null,
): string {
  // REVIEW FIXES 2026-09-05 (LOW) — every hole escaped; see `escapeHtml`
  // above for which two of these are not written by this project and why the
  // other two are escaped anyway.
  const versionLine = info?.version
    ? `Spaceadom · v${escapeHtml(info.version)}` : "Spaceadom";
  const kindLine = info?.installKind
    ? `<div class="set-note" id="set-about-kind" style="margin-top:2px;">${escapeHtml(info.installKind)}</div>` : "";
  // Beside "Check for updates", not under it: they are the two directions of
  // the same journey and separating them would make going back feel like a
  // different kind of act from going forward.
  // `rollbackVersion` is read off a DIRECTORY NAME on disk (updater.rs), so it
  // is the least trusted string in this function despite looking like the most
  // ordinary one. It lands in an attribute AND in visible text; both escaped.
  const rollbackSafe = rollbackVersion === null ? null : escapeHtml(rollbackVersion);
  const rollbackBtn = rollbackSafe
    ? `<button type="button" class="btn set-act-inline" id="set-about-rollback"
               data-rollback-version="${rollbackSafe}">Roll back to ${rollbackSafe}</button>`
    : "";
  return `
    <div class="set-about-id">
      <span class="set-row-label" id="set-about-version" style="cursor:default;">${versionLine}</span>
      ${kindLine}
    </div>
    <div class="set-act-grid" style="margin-top:10px;">
      <button type="button" class="btn set-act-inline" id="set-about-check-update">Check for updates</button>
      ${rollbackBtn}
    </div>
    ${/* `updateStatusText` is updater.rs's `message`, printed VERBATIM for any
          state the listener does not recognise — which is exactly why it must
          be escaped here rather than trusted: the contract guarantees that a
          sentence nobody in this file has read will reach this hole. */ ""}
    <div class="set-note" id="set-about-update-status" role="status" aria-live="polite" style="min-height:14px;">${escapeHtml(updateStatusText)}</div>

    <div class="set-act-grid" style="margin-top:10px;">
      <button type="button" class="btn set-act-inline" data-about-link="github">GitHub</button>
      <button type="button" class="btn set-act-inline" data-about-link="issues">Report a problem</button>
      <button type="button" class="btn set-act-inline" data-about-link="privacy">Privacy policy</button>
      <button type="button" class="btn set-act-inline" data-about-link="licence">Source-visible, proprietary — see LICENSE</button>
    </div>

    <div class="set-about-tp-wrap">
      <button type="button" class="set-row-label" id="set-about-tp-toggle"
              aria-expanded="${thirdPartyOpen}" aria-controls="set-about-tp-list">
        Third-party software · ${thirdPartyCount}
      </button>
      <div class="set-about-tp-list"${thirdPartyOpen ? "" : " hidden"} id="set-about-tp-list"></div>
    </div>`;
}
