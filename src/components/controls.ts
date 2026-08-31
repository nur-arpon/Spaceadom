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
 */
/**
 * Which character each switch performs when Fun mode is on (spec §2).
 * The mapping is deliberately by CHARACTER, not by row, so two switches that
 * mean the same kind of thing move the same way:
 *
 *   thr  thruster    engine ignition, and the "show me around" convoy — the
 *                    two whose SOUNDS are already thruster notes
 *   fun  orbit hop   the personality switch itself, accent -> sage track
 *   orb  orbit hop   run at startup (spec: "same as fun"), hiding the
 *                    board, where the knob arcs up and away like the layout,
 *                    showing the HUD's special keys, and switching the ring
 *                    between the new and classic layouts — a ring being
 *                    re-formed, or an inner one appearing and disappearing,
 *                    IS an orbit
 *   rng  sonar ring  sound ticks, the software overlay and point-to-launch —
 *                    each is "something went out and came back" (the pointer
 *                    pings a chip and the launch answers)
 *   wrp  warp smear  visual effects, and the guide-to-toast flight — the
 *                    switch that governs a smear is best shown as one
 */
const TOGGLE_CHAR: Record<string, string> = {
  around: "thr", engine: "thr", fun: "fun", sound: "rng",
  startup: "orb", motion: "wrp", hideboard: "orb", software: "rng",
  flight: "wrp", hudpointer: "rng", hudspecials: "orb", hudlayout: "orb",
};

/**
 * A MISSING ENTRY FALLS BACK TO `wrp` SILENTLY (see `toggleSwitchHtml`), so
 * every new SWITCH id belongs in the table above. SEGMENTED PILLS do not:
 * they have no knob to animate, so `theme` has never had an entry and neither
 * does `hudrows`. That absence is deliberate, not an oversight — adding one
 * would be dead data, because `segRowHtml` below never consults this table.
 */

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
  "Two rows are in use, so there is no inner ring left for these to sit in. " +
  "Pick 1 row — or Auto, which uses one ring whenever your apps fit — and they " +
  "come back. What you chose here is remembered either way, and the special " +
  "keys themselves keep working.";

/**
 * The reason line shown under the "Shortcut rows" pill when "New ring layout"
 * is off and the pill therefore has nothing to do.
 *
 * SAME PATTERN AS `SPECIALS_INERT_NOTE` ABOVE, on purpose, and it lives here
 * for the same reason: both the panel and `preview.ts` render it, and prose
 * duplicated across two files drifts the moment one is edited. It says the
 * same three things — why the control is greyed out, what to change to get it
 * back, and that nothing the user chose was thrown away. If you edit one of
 * these two notes, read the other; a user who meets both should not feel they
 * were written by different people.
 */
export const ROWS_INERT_NOTE =
  "Rows only apply to the new ring layout. The classic ring uses its own fixed " +
  "shape. Turn the new layout back on to choose how many rows — what you " +
  "picked here is remembered either way.";

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
): string {
  const idx = Math.max(0, opts.findIndex(([v]) => v === value));
  const style = indicatorStyle ? ` style="${indicatorStyle}"` : "";
  return `
    <div class="theme-seg" style="--seg-i:${idx}" role="radiogroup" aria-label="${ariaLabel}">
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
 * The switch itself. `preview.ts` renders this same function, so the dev
 * harness can never drift from the app — it went on showing a "Dark mode"
 * switch for three versions after the theme pill replaced it.
 */
export function toggleSwitchHtml(id: string, on: boolean, anim?: "on" | "off"): string {
  return `
    <span class="toggle-switch" data-char="${TOGGLE_CHAR[id] ?? "wrp"}"${anim ? ` data-anim="${anim}"` : ""}>
      <input type="checkbox" id="set-${id}" ${on ? "checked" : ""} />
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
