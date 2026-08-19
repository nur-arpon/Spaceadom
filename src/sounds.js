/*
 * Spaceadom — sound kit (drop-in module)
 * ============================================================
 * WebAudio synthesis only. No audio assets, no files to ship.
 * Every sound in the Help Lab mockup is here, named, with a note
 * saying exactly WHERE it plays.
 *
 * USAGE
 *   import { Sfx } from "./sounds.js";
 *   const sfx = new Sfx({
 *     enabled: () => settings.sound,      // "Sound ticks" switch
 *     fun:     () => settings.fun,        // "Fun mode" switch
 *     volume:  () => settings.tickVolume, // 0–100 slider, 40 = normal
 *   });
 *   sfx.unlock();                         // call once from any user gesture
 *   sfx.toggleOn("engine");               // then just fire named sounds
 *
 * RULES
 *   - enabled() false  → total silence (except sfx.force* variants).
 *   - fun() false      → every call collapses to a single soft tick,
 *                        EXCEPT theme picks (they stay distinct but plain)
 *                        and the two-step destructive confirmations.
 *   - Nothing plays on first paint / state restore. Only on real changes.
 */

const CLAMP = (v, a, b) => Math.min(b, Math.max(a, v));

export class Sfx {
  constructor(opts = {}) {
    this.opts = {
      enabled: () => true,
      fun: () => true,
      volume: () => 40,
      ...opts,
    };
    this.ctx = null;
  }

  /* ---------- engine ---------- */

  unlock() {                       // call from a click/keydown once
    this._ac();
    if (this.ctx && this.ctx.state === "suspended") this.ctx.resume();
  }

  _ac() {
    if (!this.ctx) {
      const AC = window.AudioContext || window.webkitAudioContext;
      if (!AC) return null;
      this.ctx = new AC();
    }
    return this.ctx;
  }

  _gain(vol) {                     // master gain law used by every sound
    return 0.055 * (vol == null ? 1 : vol) * (CLAMP(this.opts.volume(), 0, 100) / 40);
  }

  /** Pitched blip. f0 → f1 exponential over dur seconds. */
  tone({ f0, f1, dur = 0.12, vol = 1, type = "sine", at = 0, force = false }) {
    if (!force && !this.opts.enabled()) return;
    const c = this._ac(); if (!c) return;
    const v = this._gain(vol); if (v <= 0) return;
    try {
      const t = c.currentTime + at;
      const osc = c.createOscillator(), g = c.createGain();
      osc.type = type;
      osc.frequency.setValueAtTime(f0, t);
      osc.frequency.exponentialRampToValueAtTime(Math.max(30, f1 || f0), t + dur);
      g.gain.setValueAtTime(v, t);
      g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
      osc.connect(g); g.connect(c.destination);
      osc.start(t); osc.stop(t + dur + 0.02);
    } catch (e) {}
  }

  /** Filtered white noise — thrusters, wind-down, drums, warp. */
  noise({ f0, f1, dur = 0.3, vol = 1, ftype = "lowpass", at = 0, force = false }) {
    if (!force && !this.opts.enabled()) return;
    const c = this._ac(); if (!c) return;
    const v = this._gain(vol); if (v <= 0) return;
    try {
      const t = c.currentTime + at;
      const buf = c.createBuffer(1, Math.max(1, Math.floor(c.sampleRate * dur)), c.sampleRate);
      const d = buf.getChannelData(0);
      for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
      const src = c.createBufferSource(); src.buffer = buf;
      const f = c.createBiquadFilter(); f.type = ftype;
      f.frequency.setValueAtTime(f0, t);
      f.frequency.exponentialRampToValueAtTime(Math.max(40, f1 || f0), t + dur);
      const g = c.createGain();
      g.gain.setValueAtTime(v, t);
      g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
      src.connect(f); f.connect(g); g.connect(c.destination);
      src.start(t); src.stop(t + dur + 0.02);
    } catch (e) {}
  }

  /* ---------- 1. primitives ---------- */

  /** Generic UI click. Slider grab, "Open log folder", any plain-mode press. */
  tick() { this.tone({ f0: 600, dur: 0.06 }); }

  /** Barely-there breath. Hover-linger auto-open, slider release. */
  whisper() { this.tone({ f0: 880, dur: 0.035, vol: 0.4 }); }

  /** Plain switch on/off — used for every toggle when Fun mode is OFF. */
  flipOn() { this.tone({ f0: 470, f1: 690, dur: 0.11 }); }
  flipOff() { this.tone({ f0: 690, f1: 450, dur: 0.11 }); }

  /** Description panel opening / closing (press-to-expand rows).
   *  force:true is used when the "Sound ticks" switch is being turned ON,
   *  so the user hears the confirmation of the switch they just enabled. */
  bloomOpen(force = false) {
    this.tone({ f0: 640, dur: 0.08, force });
    this.tone({ f0: 1280, dur: 0.05, vol: 0.35, at: 0.04, force });
  }
  bloomClose() { this.tone({ f0: 420, dur: 0.08, vol: 0.8 }); }

  /* ---------- 2. special-key cards (8 entrances) ---------- */
  /* Index-matched to the 8 card animations. Call cardOpen(i) with the
     same index used to pick the animation, so sound and motion agree. */

  genie()  { this.tone({ f0: 280, f1: 880, dur: 0.32, vol: 0.9 });
             [1200, 1500, 1800].forEach((f, i) => this.tone({ f0: f, dur: 0.05, vol: 0.22, at: 0.12 + i * 0.06 })); }
  warp()   { this.noise({ ftype: "bandpass", f0: 300, f1: 2400, dur: 0.34, vol: 0.9 }); }
  sling()  { this.tone({ f0: 700, f1: 380, dur: 0.11 }); this.tone({ f0: 380, f1: 780, dur: 0.13, at: 0.09 }); }
  hinge()  { this.tone({ f0: 520, f1: 240, dur: 0.16, vol: 0.7 }); this.tone({ f0: 900, dur: 0.05, vol: 0.3, at: 0.14 }); }
  boing()  { this.tone({ f0: 220, f1: 660, dur: 0.18, vol: 0.8 }); this.tone({ f0: 660, f1: 440, dur: 0.14, vol: 0.5, at: 0.16 }); }
  crt()    { this.tone({ f0: 160, f1: 1900, dur: 0.12 }); this.tone({ f0: 2200, dur: 0.06, vol: 0.3, at: 0.1 }); }

  /** Card entrance order — MUST match the ANIMS list in DESCRIPTIONS.md §4. */
  static CARD_SOUNDS = ["genie", "warpDrop", "iris", "hinge", "sling", "unfurl", "boing", "tv"];

  /** Special key / constellation card opening. i = card index (cycles by 8). */
  cardOpen(i = 0) {
    if (!this.opts.fun()) return this.tick();
    switch (Sfx.CARD_SOUNDS[i % 8]) {
      case "warpDrop": this.warp(); this.noise({ f0: 2400, f1: 300, dur: 0.18, vol: 0.5 }); break;
      case "iris":
      case "unfurl":   this.bloomOpen(); break;
      case "hinge":    this.hinge(); break;
      case "sling":    this.sling(); break;
      case "boing":    this.boing(); break;
      case "tv":       this.crt(); break;
      default:         this.genie();
    }
  }
  cardClose() { this.bloomClose(); }

  /* ---------- 3. toggles (Fun mode ON) ---------- */
  /* Call toggleOn/toggleOff with the setting id. Plain mode is handled
     internally: it collapses to flipOn/flipOff. */

  toggleOn(id) {
    if (!this.opts.fun()) return id === "fun" ? this.genie() : this.flipOn();
    switch (id) {
      case "engine":                          // thruster ignition
        this.flipOn(); this.noise({ ftype: "bandpass", f0: 300, f1: 1800, dur: 0.25, vol: 0.5 }); break;
      case "fun":     this.genie(); break;    // personality comes back
      case "sound":   this.bloomOpen(true); break; // force: confirm its own switch
      case "software": this.bloomOpen(); break;
      case "startup": this.sling(); break;    // orbit hop
      case "motion":  this.warp(); break;     // warp stretch
      default:        this.flipOn();
    }
  }

  toggleOff(id) {
    if (!this.opts.fun()) return id === "fun" ? this.convoyOff() : this.flipOff();
    switch (id) {
      case "engine":  this.flipOff(); break;
      case "fun":     this.convoyOff(); break;  // everything winds down
      case "sound":
      case "software": this.bloomClose(); break;
      case "startup": this.sling(); break;
      case "motion":  this.warp(); break;
      default:        this.flipOff();
    }
  }

  /* ---------- 4. "Show me around" convoy ---------- */

  /** ON: nine rising thruster notes, like a formation lighting up. */
  convoyOn() {
    if (!this.opts.fun()) return this.flipOn();
    for (let i = 0; i < 9; i++) this.tone({ f0: 500 + i * 28, dur: 0.08, at: i * 0.08 });
  }

  /** OFF: the wind-down — same notes reversed, each pitch-dropping and
   *  fading, under a long lowpass sweep. Also used when Fun mode is
   *  switched off (the whole personality layer spinning down). */
  convoyOff() {
    if (!this.opts.fun()) return this.flipOff();
    for (let i = 0; i < 9; i++) {
      const f = 500 + (8 - i) * 28;
      this.tone({ f0: f, f1: f * 0.93, dur: 0.08, vol: Math.max(0.25, 1 - i * 0.07), at: i * 0.055 });
    }
    this.noise({ f0: 1100, f1: 160, dur: 0.6, vol: 0.5, at: 0.05 });
  }

  /* ---------- 5. sliders ---------- */

  sliderGrab() { this.tick(); }        // pointer down on any slider
  sliderRelease() { this.whisper(); }  // pointer up

  /* ---------- 6. destructive actions & utility ---------- */

  /** First press of "Reset this profile" / "Clear all" — arms the button. */
  arm() {
    this.tone({ f0: 240, dur: 0.06, type: "triangle", vol: 0.9 });
    this.tone({ f0: 240, dur: 0.06, type: "triangle", vol: 0.9, at: 0.11 });
  }

  /** Second press (fires), "Restore preset profiles", conflicts re-check done. */
  confirm() {
    this.tone({ f0: 523, dur: 0.09, type: "triangle" });
    this.tone({ f0: 659, dur: 0.13, type: "triangle", at: 0.09 });
  }

  /* ---------- 7. theme picks (3-way pill) ---------- */
  /** theme: "earthy" | "war" | "starry". Plain mode → single tick. */
  theme(theme) {
    if (!this.opts.fun()) return this.tick();
    if (theme === "earthy") {                     // warm daylight chime
      this.tone({ f0: 392, dur: 0.09, type: "triangle" });
      this.tone({ f0: 523, dur: 0.12, type: "triangle", at: 0.08 });
    } else if (theme === "war") {                 // two war drums + horn
      this.noise({ f0: 300, f1: 60, dur: 0.16, vol: 1 });
      this.noise({ f0: 300, f1: 60, dur: 0.16, vol: 0.9, at: 0.17 });
      this.tone({ f0: 196, f1: 262, dur: 0.4, type: "triangle", vol: 0.5, at: 0.3 });
    } else {                                      // starry sparkle + pad
      [1046, 1318, 1568].forEach((f, i) => this.tone({ f0: f, dur: 0.09, vol: 0.5, at: i * 0.07 }));
      this.tone({ f0: 220, f1: 330, dur: 0.4, vol: 0.35 });
    }
  }


  /* ============================================================
   * 8. EXTRA SOUND LIBRARY — free resource, nothing wired yet
   * ------------------------------------------------------------
   * None of these are called by the settings UI. They are here so
   * new features can pick a voice that already fits the app:
   * space / sci-fi, pirate & war, nature, and calm UI.
   * All obey enabled() and volume(); they IGNORE fun() so a feature
   * can use one deliberately. Pass {force:true}-style urgency by
   * calling the primitive directly if you ever need to bypass mute.
   * ============================================================ */

  /* ---- 8a. SPACE / SCI-FI ---- */

  /** Long airy sweep upward. Big reveals: onboarding start, first launch. */
  spaceRise() {
    this.tone({ f0: 180, f1: 1400, dur: 1.1, vol: .45, type: "sine" });
    this.noise({ ftype: "bandpass", f0: 400, f1: 3200, dur: 1.2, vol: .3 });
    this.tone({ f0: 270, f1: 2100, dur: 1.1, vol: .2, type: "triangle", at: .06 });
  }

  /** Falling counterpart. Closing a full-screen surface, exiting a mode. */
  spaceFall() {
    this.tone({ f0: 1300, f1: 190, dur: .9, vol: .45 });
    this.noise({ ftype: "bandpass", f0: 3000, f1: 300, dur: 1, vol: .28 });
  }

  /** Deep low hum, ~2s. Ambience-ish one-shot for an empty/idle state. */
  spaceDrone() {
    this.tone({ f0: 62, f1: 58, dur: 2.2, vol: .55, type: "triangle" });
    this.tone({ f0: 93, f1: 88, dur: 2.2, vol: .22, type: "sine", at: .1 });
    this.noise({ f0: 220, f1: 120, dur: 2.4, vol: .18 });
  }

  /** Rapid pitch-up zip with a tail click. Item flying into place. */
  teleportOut() {
    this.tone({ f0: 300, f1: 2600, dur: .22, vol: .8 });
    this.noise({ ftype: "highpass", f0: 900, f1: 5000, dur: .24, vol: .45 });
    this.tone({ f0: 3000, dur: .04, vol: .3, at: .2 });
  }

  /** Mirror of teleportOut — arriving. Pair them for a move animation. */
  teleportIn() {
    this.tone({ f0: 2600, f1: 320, dur: .24, vol: .8 });
    this.noise({ ftype: "highpass", f0: 5000, f1: 800, dur: .26, vol: .4 });
    this.tone({ f0: 520, dur: .07, vol: .35, at: .22, type: "triangle" });
  }

  /** Shimmer that thins out to nothing. Deleting, dismissing, clearing. */
  vanish() {
    [880, 1180, 1560, 2100].forEach((f, i) =>
      this.tone({ f0: f, f1: f * 2.2, dur: .3, vol: .34 - i * .06, at: i * .05 }));
    this.noise({ ftype: "highpass", f0: 1200, f1: 6000, dur: .45, vol: .3, at: .05 });
  }

  /** Reverse of vanish — materialising. New card, generated result. */
  materialize() {
    [2100, 1560, 1180, 880].forEach((f, i) =>
      this.tone({ f0: f * 1.8, f1: f, dur: .28, vol: .18 + i * .06, at: i * .05 }));
    this.tone({ f0: 660, dur: .09, vol: .4, at: .24, type: "triangle" });
  }

  /** Short bright laser tick. Playful confirm, tiny success. */
  laser() {
    this.tone({ f0: 1800, f1: 240, dur: .13, vol: .7, type: "square" });
    this.tone({ f0: 2400, dur: .03, vol: .25 });
  }

  /** Radar/sonar ping with a soft echo. Scanning, searching, detected. */
  sonarPing() {
    this.tone({ f0: 1240, f1: 1180, dur: .16, vol: .7, type: "sine" });
    this.tone({ f0: 1240, f1: 1160, dur: .2, vol: .28, at: .34 });
    this.tone({ f0: 1240, f1: 1150, dur: .24, vol: .12, at: .72 });
  }

  /** Slow warble, like a distant signal. Waiting on a background job. */
  beacon() {
    for (let i = 0; i < 3; i++) {
      this.tone({ f0: 700, f1: 900, dur: .18, vol: .4, at: i * .42 });
      this.tone({ f0: 900, f1: 700, dur: .18, vol: .3, at: i * .42 + .18 });
    }
  }

  /** Rocket burn, ~1.4s of rising filtered noise. Launch / export started. */
  rocketBurn() {
    this.noise({ ftype: "lowpass", f0: 180, f1: 1500, dur: 1.4, vol: .8 });
    this.noise({ ftype: "bandpass", f0: 700, f1: 2600, dur: 1.4, vol: .35, at: .1 });
    this.tone({ f0: 70, f1: 130, dur: 1.4, vol: .4, type: "triangle" });
  }

  /* ---- 8b. PIRATE & WAR (pairs with the Warcry theme) ---- */

  /** Two dry drum hits. Warcry theme pick already uses this shape. */
  warDrum(hits = 2) {
    for (let i = 0; i < hits; i++)
      this.noise({ f0: 300, f1: 60, dur: .16, vol: 1 - i * .08, at: i * .17 });
  }

  /** Marching pattern, 8 beats. Longer transitions, a loading sequence. */
  warMarch() {
    for (let i = 0; i < 8; i++)
      this.noise({ f0: 280, f1: 55, dur: .14, vol: i % 2 ? .55 : .95, at: i * .19 });
  }

  /** Low brass call, two notes rising. Big announcement, level-up. */
  warHorn() {
    this.tone({ f0: 155, f1: 208, dur: .55, vol: .8, type: "triangle" });
    this.tone({ f0: 208, f1: 262, dur: .5, vol: .6, type: "triangle", at: .5 });
    this.noise({ f0: 300, f1: 160, dur: .9, vol: .2 });
  }

  /** Metallic scrape. Arming a destructive action with real menace. */
  swordUnsheath() {
    this.noise({ ftype: "bandpass", f0: 1800, f1: 4200, dur: .3, vol: .55 });
    this.tone({ f0: 2600, f1: 3400, dur: .26, vol: .3, type: "sawtooth" });
  }

  /** Two blades meeting. Rejected action, conflict detected. */
  swordClash() {
    this.noise({ ftype: "bandpass", f0: 3000, f1: 1200, dur: .22, vol: .8 });
    this.tone({ f0: 2200, f1: 1500, dur: .18, vol: .4, type: "square" });
    this.noise({ ftype: "highpass", f0: 4000, f1: 2000, dur: .4, vol: .2, at: .1 });
  }

  /** Cannon: thump, then a long tail. Heavy irreversible action fired. */
  cannon() {
    this.noise({ f0: 400, f1: 40, dur: .5, vol: 1 });
    this.tone({ f0: 90, f1: 38, dur: .6, vol: .9, type: "triangle" });
    this.noise({ ftype: "lowpass", f0: 900, f1: 120, dur: 1.2, vol: .35, at: .12 });
  }

  /** Rope/rigging creak. Panel dragging, resizing, an old-timey hover. */
  ropeCreak() {
    this.tone({ f0: 210, f1: 150, dur: .5, vol: .3, type: "sawtooth" });
    this.noise({ ftype: "bandpass", f0: 500, f1: 240, dur: .55, vol: .25 });
  }

  /** Wooden hull knock, two taps. Plain but characterful button press. */
  woodKnock() {
    this.tone({ f0: 240, f1: 120, dur: .07, vol: .8, type: "triangle" });
    this.tone({ f0: 200, f1: 100, dur: .06, vol: .5, type: "triangle", at: .1 });
  }

  /** Voice-shaped "yarr" — sawtooth growl bent down through a lowpass.
   *  Easter egg only: a hidden shortcut, konami, or the Warcry theme
   *  picked three times in a row. Never on a routine action. */
  pirateYarr() {
    this.tone({ f0: 150, f1: 118, dur: .5, vol: .85, type: "sawtooth" });
    this.tone({ f0: 224, f1: 168, dur: .5, vol: .35, type: "square", at: .02 });
    this.noise({ ftype: "bandpass", f0: 900, f1: 420, dur: .55, vol: .3 });
    this.tone({ f0: 112, f1: 96, dur: .3, vol: .4, type: "sawtooth", at: .42 });
  }

  /** Crowd of pirates answering — three staggered growls. Big celebration. */
  crewCheer() {
    [0, .09, .2].forEach((at, i) => {
      this.tone({ f0: 140 + i * 26, f1: 108 + i * 20, dur: .55, vol: .5 - i * .1, type: "sawtooth", at });
      this.noise({ ftype: "bandpass", f0: 850 + i * 200, f1: 400, dur: .6, vol: .22, at });
    });
  }

  /* ---- 8c. NATURE: ocean, sky, birds ---- */

  /** One wave rolling in and receding, ~1.8s. Pairs with the ocean scene. */
  oceanWave() {
    this.noise({ ftype: "lowpass", f0: 300, f1: 900, dur: .9, vol: .5 });
    this.noise({ ftype: "lowpass", f0: 900, f1: 200, dur: .9, vol: .45, at: .85 });
  }

  /** Bigger crash with spray. A dramatic beat in a Starry-night intro. */
  oceanCrash() {
    this.noise({ ftype: "lowpass", f0: 200, f1: 1400, dur: .5, vol: .9 });
    this.noise({ ftype: "highpass", f0: 1800, f1: 5000, dur: .8, vol: .35, at: .3 });
    this.tone({ f0: 80, f1: 45, dur: .7, vol: .35, type: "triangle" });
  }

  /** Steady wind, ~2.5s. Empty states, long waits, a becalmed screen. */
  windGust() {
    this.noise({ ftype: "bandpass", f0: 380, f1: 1100, dur: 1.3, vol: .5 });
    this.noise({ ftype: "bandpass", f0: 1100, f1: 340, dur: 1.3, vol: .42, at: 1.2 });
  }

  /** Rain-like hiss, ~2s. Alternative quiet ambience one-shot. */
  rainHiss() {
    this.noise({ ftype: "highpass", f0: 2200, f1: 3000, dur: 2, vol: .3 });
    this.noise({ ftype: "bandpass", f0: 900, f1: 1200, dur: 2, vol: .18, at: .05 });
  }

  /** Distant thunder. A hard failure in the Warcry theme. */
  thunder() {
    this.noise({ ftype: "lowpass", f0: 300, f1: 45, dur: 1.6, vol: .9 });
    this.tone({ f0: 55, f1: 32, dur: 1.8, vol: .5, type: "triangle" });
    this.noise({ ftype: "lowpass", f0: 700, f1: 90, dur: 2.2, vol: .3, at: .3 });
  }

  /** Two-note songbird chirp. Small friendly success, item saved. */
  birdChirp() {
    this.tone({ f0: 2400, f1: 3300, dur: .06, vol: .5 });
    this.tone({ f0: 3100, f1: 2500, dur: .07, vol: .4, at: .08 });
  }

  /** Harsher gull cry, falling. Matches the gulls over the ship's masts. */
  gullCry() {
    this.tone({ f0: 1500, f1: 900, dur: .22, vol: .55, type: "sawtooth" });
    this.tone({ f0: 1300, f1: 780, dur: .2, vol: .35, type: "sawtooth", at: .26 });
  }

  /** Soft wing flaps, three beats. Something flying away, dismissed. */
  wingFlap() {
    for (let i = 0; i < 3; i++)
      this.noise({ ftype: "lowpass", f0: 700, f1: 220, dur: .12, vol: .45 - i * .1, at: i * .17 });
  }

  /** Bright arpeggio like a clear sky. Onboarding complete, all-clear. */
  skyChime() {
    [784, 988, 1175, 1568].forEach((f, i) =>
      this.tone({ f0: f, dur: .5, vol: .4 - i * .05, type: "triangle", at: i * .1 }));
  }

  /* ---- 8d. CALM UI ---- */

  /** Wooden marimba-ish note. A gentler alternative to tick(). */
  softPluck() { this.tone({ f0: 520, f1: 500, dur: .16, vol: .55, type: "triangle" }); }

  /** Two-note fifth, warm and neutral. Generic positive acknowledgement. */
  gentleChime() {
    this.tone({ f0: 587, dur: .22, vol: .5, type: "triangle" });
    this.tone({ f0: 880, dur: .3, vol: .35, type: "triangle", at: .1 });
  }

  /** Slow swell up, no attack. A drawer or sheet sliding open. */
  breatheIn() {
    this.tone({ f0: 300, f1: 480, dur: .7, vol: .3, type: "sine" });
    this.noise({ ftype: "lowpass", f0: 500, f1: 1200, dur: .7, vol: .18 });
  }

  /** Swell down. The same surface closing. */
  breatheOut() {
    this.tone({ f0: 480, f1: 280, dur: .7, vol: .3, type: "sine" });
    this.noise({ ftype: "lowpass", f0: 1200, f1: 400, dur: .7, vol: .18 });
  }

  /** Three descending notes. Something dismissed without being deleted. */
  fadeAway() {
    [700, 560, 420].forEach((f, i) =>
      this.tone({ f0: f, dur: .2, vol: .4 - i * .1, type: "triangle", at: i * .12 }));
  }

  /** Four-note resolve. A real milestone: setup done, profile created. */
  successBloom() {
    [523, 659, 784, 1047].forEach((f, i) =>
      this.tone({ f0: f, dur: .28, vol: .5 - i * .06, type: "triangle", at: i * .085 }));
    this.tone({ f0: 1568, dur: .1, vol: .2, at: .4 });
  }

  /** Flat two-note drop. Soft error — nothing broke, but it didn't work. */
  errorThud() {
    this.tone({ f0: 300, f1: 280, dur: .12, vol: .7, type: "triangle" });
    this.tone({ f0: 220, f1: 190, dur: .2, vol: .6, type: "triangle", at: .12 });
  }

  /** Rising two-note nudge. Needs attention, non-blocking notice. */
  notify() {
    this.tone({ f0: 660, dur: .12, vol: .5, type: "triangle" });
    this.tone({ f0: 990, dur: .18, vol: .4, type: "triangle", at: .12 });
  }

  /** Even ticks for a countdown or progress step. n = how many. */
  countTicks(n = 3, gap = .3) {
    for (let i = 0; i < n; i++) this.tone({ f0: 760, dur: .05, vol: .45, at: i * gap });
  }

  /* ---- 8e. helper: audition everything ---- */
  /** Play every library sound in sequence, ~1.6s apart, logging names.
   *  Handy for picking one; call from a dev console. Returns the count. */
  auditionAll(gap = 1600) {
    const names = Object.getOwnPropertyNames(Sfx.prototype).filter(n =>
      !["constructor", "unlock", "tone", "noise", "auditionAll", "_ac", "_gain",
        "cardOpen", "cardClose", "toggleOn", "toggleOff", "theme"].includes(n) && n[0] !== "_");
    names.forEach((n, i) => setTimeout(() => { console.log("sfx:", n); try { this[n](); } catch (e) {} }, i * gap));
    return names.length;
  }
}

/*
 * WHERE EACH SOUND BELONGS — quick map for wiring
 * ------------------------------------------------------------
 * Setting label pressed (description opens)      bloomOpen()
 * Setting label pressed (description closes)     bloomClose()
 * Hover-linger auto-opens a description          whisper()
 * "Show me around" ON / OFF                      convoyOn() / convoyOff()
 * Any toggle flipped                             toggleOn(id) / toggleOff(id)
 * Slider grabbed / released                      sliderGrab() / sliderRelease()
 * Theme pill pick                                theme("earthy"|"war"|"starry")
 * Special-key chip or board key opens a card     cardOpen(index)
 * Constellation pressed (Starry night)           cardOpen(index)
 * Any card closed                                cardClose()
 * "Reset this profile" / "Clear all" 1st press   arm()
 * ...2nd press, or "Restore preset profiles"     confirm()
 * "Re-check now" finishes (after 800ms)          confirm()
 * "Open log folder"                              tick()
 *
 * EXTRA LIBRARY (§8) — not wired, pick as needed
 *   space:    spaceRise, spaceFall, spaceDrone, rocketBurn, beacon
 *   movement: teleportOut, teleportIn, vanish, materialize, laser, sonarPing
 *   pirate:   pirateYarr (easter egg only), crewCheer, cannon, swordUnsheath,
 *             swordClash, ropeCreak, woodKnock
 *   war:      warDrum, warMarch, warHorn, thunder
 *   nature:   oceanWave, oceanCrash, windGust, rainHiss,
 *             birdChirp, gullCry, wingFlap, skyChime
 *   calm UI:  softPluck, gentleChime, breatheIn, breatheOut, fadeAway,
 *             successBloom, errorThud, notify, countTicks
 *   audition: sfx.auditionAll() plays them all in order, names in console
 *
 * NOT IMPLEMENTED ON PURPOSE
 *   No ambient loops anywhere. Starry night is silent apart from these
 *   interaction sounds — no drifting chimes, no whooshes, no ocean bed.
 */
