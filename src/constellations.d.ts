/**
 * Type surface for `constellations.js` — the 20-constellation geometry the
 * owner extracted verbatim from `Help Lab v2-4.dc.html` (2026-08-20).
 *
 * The .js stays BYTE-IDENTICAL to design/constellations.js (same rule as
 * sounds.js): a retyped copy is one that silently drifts, so the types live
 * here instead of being woven into the data.
 */
export interface Constellation {
  name: string;
  season: string;
  fact: string;
  /** [x px in the authored 1400px cell (discarded at runtime), y % of viewport height]. */
  pos: [number, number];
  /** Star points in a local px space (~0–90); index 0 is the key star (r 1.9). */
  pts: [number, number][];
  /** Index pairs into pts — the segments drawn between stars. */
  lines: [number, number][];
}

export declare const CONSTELLATIONS: Constellation[];
