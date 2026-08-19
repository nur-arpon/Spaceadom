/**
 * Type surface for `sounds.js` (PROBLEM 147).
 *
 * The module is kept BYTE-IDENTICAL to the one the owner supplied
 * (design/sounds.js) — CLAUDE.md's "transcribe, never paraphrase" applies to
 * handed-over design assets, and a retyped copy is one that silently drifts.
 * So the types live here instead of being woven into the source.
 */
export interface SfxOpts {
  /** The "Sound ticks" switch. False = total silence. */
  enabled?: () => boolean;
  /** The "Fun mode" switch. False collapses most sounds to a single tick. */
  fun?: () => boolean;
  /** 0-100; 40 is the neutral point of the gain law. */
  volume?: () => number;
}

export declare class Sfx {
  constructor(opts?: SfxOpts);
  /** Must be called from a real user gesture before anything will sound. */
  unlock(): void;

  tick(): void;
  whisper(): void;
  flipOn(): void;
  flipOff(): void;
  bloomOpen(force?: boolean): void;
  bloomClose(): void;

  cardOpen(i?: number): void;
  cardClose(): void;

  toggleOn(id: string): void;
  toggleOff(id: string): void;

  convoyOn(): void;
  convoyOff(): void;

  sliderGrab(): void;
  sliderRelease(): void;

  arm(): void;
  confirm(): void;

  /** "earthy" | "war" | "starry" */
  theme(theme: string): void;

  /** §8a. Long airy sweep up — "big reveals". Ignores fun() by design. */
  spaceRise(): void;
  /** §8a. Its falling counterpart — "exiting a mode". Ignores fun(). */
  spaceFall(): void;

  /** §8 library — available, deliberately unwired. */
  [extra: string]: unknown;
}
