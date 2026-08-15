# Motion & Physics

Concrete constants for motion that feels right. Pick from these ladders instead of inventing values.

## Duration ladder

Duration scales with distance travelled and surface size. A toggle and a full-screen panel should not share a number.

| Interaction | Duration | Notes |
|---|---|---|
| Hover / focus tint | 80–120 ms | Must feel instant. Above 150 ms the cursor outruns it. |
| Button press / release | 80–100 ms | Press faster than release. |
| Toggle, checkbox, small state flip | 150–200 ms | |
| Tooltip, small popover appear | 150 ms in, 100 ms out | Exit always faster than entry. |
| Dropdown, context menu | 200 ms | |
| Card expand, inline detail | 250–300 ms | |
| Dialog / modal entry | 250–300 ms | |
| Page or route transition | 300–400 ms | Ceiling for anything the user waits on. |
| Full-window overlay, HUD | 200–250 ms | Keep short — these interrupt. |
| Ambient / decorative loop | 2000 ms+ | Must never demand attention. |

Two rules that matter more than the exact number:

- **Exit is faster than entry**, typically 60–70% of it. Users have already decided; do not make them wait to leave.
- **Larger surface, longer duration.** Something crossing 600 px cannot use the same 150 ms as something crossing 40 px, or it appears to teleport.

## Easing

Never `linear` for anything a user initiates. Never `ease-in-out` by reflex — it is slow at both ends and reads as mushy.

```css
/* Entering — decelerate. Fast start, gentle settle. The default for most UI. */
--ease-out: cubic-bezier(0.2, 0.8, 0.2, 1);

/* Exiting — accelerate away. */
--ease-in: cubic-bezier(0.4, 0, 1, 1);

/* Moving between two on-screen positions. */
--ease-in-out: cubic-bezier(0.4, 0, 0.2, 1);

/* Emphasis — a slight, disciplined overshoot. Use sparingly, on one element. */
--ease-emphasis: cubic-bezier(0.34, 1.4, 0.64, 1);

/* Windows Fluent's own curves */
--ease-fluent-standard: cubic-bezier(0.8, 0, 0.2, 1);
--ease-fluent-accelerate: cubic-bezier(0.7, 0, 1, 0.5);
--ease-fluent-decelerate: cubic-bezier(0.1, 0.9, 0.2, 1);
```

**Bounce and elastic easing on ordinary UI is the single clearest "AI made this" signal.** A button that wobbles on click is not delightful, it is noisy. Reserve overshoot for one moment of genuine emphasis per screen, at most.

## Spring physics

Springs describe motion by physical behavior rather than a fixed duration, which is why gesture-driven and interruptible motion should use them. A spring can be redirected mid-flight; a tween cannot without a visible seam.

The three parameters:

- **Stiffness** — how hard it pulls toward target. Higher is faster and snappier.
- **Damping** — resistance. Higher settles sooner; too low oscillates.
- **Mass** — inertia. Usually leave at 1; raising it makes motion feel heavy and slow.

Critical damping (settles fastest with no overshoot) is at `damping = 2 × sqrt(stiffness × mass)`. For stiffness 300, mass 1, that is ≈ 34.6. Damping below that overshoots; above it eases in slowly.

| Feel | stiffness | damping | mass | Use for |
|---|---|---|---|---|
| Instant, no overshoot | 500 | 45 | 1 | Hover, focus rings, tiny state |
| Snappy (default UI) | 300 | 30 | 1 | Buttons, toggles, most transitions |
| Smooth, slight settle | 200 | 26 | 1 | Cards, panels, dropdowns |
| Gentle, weighted | 120 | 20 | 1 | Large surfaces, dialogs |
| Playful, visible bounce | 300 | 15 | 1 | One accent moment only |
| Drag release | 400 | 40 | 1 | Follow-through after a gesture |

CSS has native spring easing in recent Chromium via `linear()`, but generating the keyframe list by hand is impractical — use a generator or a JS library when you need a true spring, and a tuned `cubic-bezier` otherwise.

## Choreography

**Stagger.** When several items enter together, offset them by 20–50 ms each. Below 20 ms reads as simultaneous; above 60 ms the last item feels forgotten. Cap total stagger at ~300 ms — for a long list, stagger the first 6–8 items and show the rest at once.

**Sequence order.** Container first, then contents. A dialog scrim fades, the surface scales in, then its children stagger. Reverse exactly on exit.

**Overlap.** Consecutive steps should overlap 30–50%. Strictly sequential animation feels like a slideshow. In GSAP that is a negative position offset (`'-=0.2'`); in CSS it is a shorter `animation-delay` than the previous duration.

**One focal point.** At any instant one thing should be the subject. Multiple independent animations competing for attention is the most common cause of an interface feeling cheap.

## Transform origin and distance

Motion should originate where the interaction happened. A menu opened from a button in the top-right should scale from its top-right corner, not its center. Set `transform-origin` to match, or use anchor positioning.

Entry distance: **8–24 px** for small elements, **32–48 px** for panels. Anything sliding more than ~64 px reads as travel rather than appearance, and needs a longer duration to match. Combine a small translate with a scale from 0.96–0.98 rather than 0.8 — large scale jumps look like a zoom effect.

## Compositor-only properties

These animate on the GPU compositor thread and hold frame rate even when JavaScript is busy or the Rust backend is saturated:

- `transform` — translate, scale, rotate
- `opacity`
- `filter` — cheap for `brightness`/`saturate`; `blur` over a large area is not

Everything else triggers layout or paint each frame. Specifically avoid animating `width`, `height`, `top`, `left`, `margin`, `padding`, `box-shadow`, `border-radius`, `background-color` on large surfaces.

Substitutions:

- Animating size → animate `scale`, or use `interpolate-size: allow-keywords` with `height: auto`
- Animating shadow → put the shadow on a pseudo-element and animate that element's `opacity`
- Animating position → `translate`, never `top`/`left`
- Animating background color on a large surface → overlay a tinted layer and animate its `opacity`

`will-change: transform` promotes an element to its own layer. Apply it before the animation starts and remove it after — leaving it on permanently costs GPU memory per element, which matters at scale (see `performance-budget.md`).

## Reduced motion

Windows users enable this more often than web users, and Tauri apps inherit the setting.

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
    scroll-behavior: auto !important;
  }
}
```

The blanket override is the safe floor, but the better treatment keeps *opacity* transitions and drops *movement*. Reduced motion targets vestibular discomfort from travel and parallax, not from things appearing. A cross-fade at 150 ms is usually fine and preserves the sense of continuity.

Check it in JS before starting an imperative timeline:

```ts
const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
```

## CSS-native techniques

WebView2 is Chromium 151. All of these work with no dependency.

```css
/* Animate in from display:none — no JS, no mount hook */
.panel {
  opacity: 1;
  translate: 0 0;
  transition: opacity 200ms var(--ease-out),
              translate 200ms var(--ease-out),
              display 200ms allow-discrete;
}
.panel[hidden] { opacity: 0; translate: 0 8px; }

@starting-style {
  .panel { opacity: 0; translate: 0 8px; }
}
```

```css
/* Height to auto, properly */
:root { interpolate-size: allow-keywords; }
.accordion { height: 0; overflow: hidden; transition: height 250ms var(--ease-out); }
.accordion.open { height: auto; }
```

```css
/* Reveal on scroll — compositor thread, no scroll listener */
@supports (animation-timeline: view()) {
  .reveal {
    animation: fade-up linear both;
    animation-timeline: view();
    animation-range: entry 10% cover 35%;
  }
  @keyframes fade-up {
    from { opacity: 0; transform: translateY(16px); }
    to   { opacity: 1; transform: none; }
  }
}
```

**View Transitions** handle cross-state morphs that would otherwise need a FLIP library:

```ts
if (!document.startViewTransition) { render(); }
else { document.startViewTransition(() => render()); }
```

Give the shared element the same `view-transition-name` in both states and the browser interpolates position, size, and appearance between them. This replaces most of what `layoutId` and GSAP Flip are used for, with no dependency.

## Window-level motion

Tauri windows are OS windows — the DOM cannot animate their bounds smoothly. Animating window size or position frame-by-frame from Rust produces visible tearing on Windows, since each resize triggers a full WebView2 relayout.

Prefer: a fixed-size window whose *contents* animate. For an overlay or HUD, create the window at final size, transparent and hidden, then fade the DOM contents in. For a launcher or command palette, size the window once to the maximum extent and let the visible surface animate inside it.

If a window must resize, do it in one step and let the content cross-fade — a single jump reads as intentional, a 60-step animated resize reads as broken.
