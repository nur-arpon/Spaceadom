/**
 * setting-subs.ts — 1.0.119 (brief 4 §5). The one-line subtitle under each
 * segmented control in Settings, PER OPTION, so the line follows the choice.
 *
 * Before this the three "math subtitles" (owner, 2026-09-15) were pinned to
 * the ROW: "Sized by the golden ratio." sat under Icon ring / Space ring
 * whichever was chosen, "A Fibonacci cap, for density." under Favourites /
 * All, "Packed like a sunflower's seeds." under Rings / Spiral, and Ring
 * layout had none. Every option now has its own clause, in the same voice
 * (short, plain, one clause), and `paintSubLine` swaps the text when the
 * selection changes — `settings-panel.ts` calls it from each pill's click
 * handler and the row builders render the current one on load.
 *
 * A LEAF: nothing here imports from the app, so `preview.ts` can render the
 * identical lines and `scripts/setting-subs.test.ts` can assert the map is
 * complete against the option tables in `controls.ts` without a DOM.
 */

/** control id → option value → the subtitle. Every option of every pill that
 *  carries a subtitle has an entry; the test asserts it against
 *  `controls.ts`'s option tables so a new option cannot ship silent. */
export const SUB_LINES: Readonly<Record<string, Readonly<Record<string, string>>>> = {
  middlestyle: {
    icon_ring: "App icons around the cursor, sized by the golden ratio.",
    guide_hud: "The same ring the Space key shows.",
  },
  middlescope: {
    my_eight: "Only the apps you pinned.",
    all: "Every key in the profile, plus the specials.",
  },
  alllayout: {
    rings: "A Fibonacci cap, for density.",
    spiral: "Packed like a sunflower’s seeds.",
  },
  hudring: {
    compact: "One tight ring; long names wait until you aim.",
    wide: "One roomy ring, every name written out.",
    double: "Two rings of apps; the specials step aside.",
  },
  touchpadlook: {
    app: "The page wears your app theme’s colours.",
    chocolate: "The design’s dark chocolate, with your accent.",
  },
  // 1.0.123 — a TOGGLE's line, keyed "on"/"off" (the test's TOGGLES table).
  tpslidetoast: {
    on: "One small pill shows the level as you slide.",
    off: "Slides change the level silently.",
  },
};

/** The subtitle for `control` at `option`, or "" when there is none. */
export function subLineFor(control: string, option: string): string {
  return SUB_LINES[control]?.[option] ?? "";
}

/** The always-visible subtitle under a row's label, addressable by id so
 *  `paintSubLine` can swap it in place (a render() would destroy the pill's
 *  sliding indicator — PROBLEM 157). */
export function subLineHtml(control: string, option: string): string {
  return `<div class="set-sub" id="set-${control}-sub">${subLineFor(control, option)}</div>`;
}

/** Swap the subtitle under `control` to the one for `option`, in place. */
export function paintSubLine(root: ParentNode | null | undefined, control: string, option: string): void {
  const el = root?.querySelector<HTMLElement>(`#set-${control}-sub`);
  if (el) el.textContent = subLineFor(control, option);
}
