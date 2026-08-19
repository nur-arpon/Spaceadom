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
 *   orb  orbit hop   run at startup (spec: "same as fun"), and hiding the
 *                    board, where the knob arcs up and away like the layout
 *   rng  sonar ring  sound ticks and the software overlay — both are
 *                    "something went out and came back"
 *   wrp  warp smear  visual effects
 */
const TOGGLE_CHAR: Record<string, string> = {
  around: "thr", engine: "thr", fun: "fun", sound: "rng",
  startup: "orb", motion: "wrp", hideboard: "orb", software: "rng",
};

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
