
# Tauri Motion Skill

Build beautiful, performant animations for Tauri v2 desktop apps on Windows.

This skill synthesizes August 2026 research into animation libraries, UI component registries, AI-native MCPs, and Tauri v2-specific constraints. Use it to guide architecture, library selection, and implementation patterns that avoid common pitfalls in desktop WebView2 apps.

**Version data verified against the npm registry on 2026-08-10.** Before quoting a version number to a user, re-check with `npm view <pkg> version` — everything below moves fast.

---

## Quick Decision Tree

**You're building a Tauri v2 + React desktop app and want beautiful animations.** Answer these three questions to land on a stack:

### 1. What's your primary animation need?

**Component transitions, enter/exit, layout shifts** → Motion 13 (`motion/react`)
Root reason: `layoutId` shared-element animation and `AnimatePresence` are best-in-class for component-driven animation. Bundle size ~44 KB.

**Authored timelines, scroll choreography, SVG morphing, SplitText effects** → GSAP 3.15 (`gsap`)
Root reason: all plugins (ScrollTrigger, MorphSVG, SplitText, Flip) are now 100% free since Webflow's April 2025 acquisition. Bundle size ~27 KB. Use `useGSAP` hook for React discipline.

**Micro-interactions on lists/grids without manual code** → AutoAnimate
Root reason: one line of code animates add/remove/move. Bundle size 3.2 KB.

**Interactive vector animation (mascots, complex toggles, loading states)** → Rive
Root reason: state-machine-driven `.riv` files are smaller and more powerful than Lottie. Designer-authoring in Rive's SaaS editor (free tier available; paid tiers for teams). Bundle the WASM locally for a Tauri app (~1 MB once).

**CSS-only for maximum performance** → Native CSS (see section: CSS-native approaches)
Root reason: Tauri targets Chromium 151 (evergreen WebView2), which supports `@starting-style`, `transition-behavior: allow-discrete`, View Transitions (same-document and cross-document), scroll-driven animations (`animation-timeline: scroll()/view()`), popover API, anchor positioning, and container queries. No JS, runs on the compositor thread, zero bytes. Start here for anything scroll-linked, reveal-on-enter, route transitions, or state-change animations.

### 2. Which UI component library?

**shadcn/ui** (Radix-backed, copy-paste source code into your repo)

- Default starting point. 267 built-in registries in the shadcn CLI v4 (`npx shadcn@latest add @registry-name/component-name`).
- Aceternity UI (200+ animated components via `@aceternity`), Magic UI (50+ via `@magicui`), motion-primitives, React Bits, COSS/Origin UI, Kokonut UI all install via the CLI.
- **Gotcha:** Aceternity and Magic UI ship with hardcoded color values and often demo images from Unsplash. Grep and replace before shipping in a bundled desktop app (no CDN access).
- **Honest take:** using shadcn defaults (indigo gradient, Inter font, three-card grid) reads as "AI-generated." Pair with `design-system.md`, use a deliberately different typeface, and you're gold.

**HeroUI v3** (2026 rebuild, dropped Framer Motion, ships CSS keyframes instead)

- Newest tier. v3 removed animation runtime entirely — 75+ web components with CSS animations and CSS variables for theming.
- **Single-package win:** batteries-included (form elements, table, sidebar) + accessibility (React Aria) + no Motion dependency.
- Ships its own official MCP server for Claude Code integration and Agent Skills. Use if you want minimal JS animation overhead.

**Base UI** (headless, unstyled, 35+ primitives)

- `@base-ui/react` 1.7.0. The MUI team's substrate. Zero animation opinions — you paint it with Tailwind. **basecn** is shadcn-style components rebuilt on Base UI.
- Use if you want headless primitives and plan to hand-author animation with Motion or GSAP.

**Hero UI or HeroUI?** Official branding is "HeroUI" (one word). It's rebranding from NextUI as of v3 (March 2026). npm package is `@heroui/react`.

### 3. How do I keep it from looking like every other AI-generated SaaS app?

Read **`design-system.md`** in this skill's references — it's the single most valuable thing. The gist: start with taste and process (hero as thesis, deliberate typography, structure encodes information, **deliberate motion in service of the subject**, no excess), then layer libraries. You'll avoid the purple-gradient/bounce-easing/scroll-jacking tells.

For **fonts**: use Fontsource's variable packages (`@fontsource-variable/inter` + one display face like `@fontsource-variable/space-grotesk`). Bundle them locally with Vite — no Google Fonts CDN in a desktop app.

For **motion specifics to avoid:** bounce/elastic easing is now a recognized AI tell. Scroll-jacking (hijacking wheel/trackpad with Lenis) is actively hostile on Windows (users expect native scroll). Don't animate `box-shadow`, `border-radius`, or `width`/`height` directly — they trigger layout thrash and paint on every frame. Use `transform`, `opacity`, `filter` only.

---

## Animation Library Details

### Motion 13 (framer-motion)

**Current version:** 13.0.0 (2026-08-05)
**License:** MIT, fully free commercially
**Bundle:** ~44 KB gzip (full); `motion/mini` is ~2.3 KB WAAPI-only
**Weekly npm downloads:** 17.5M (`motion`) + 42.7M (`framer-motion` — same code, legacy name)

**What it does best:**

- React component enter/exit with `AnimatePresence`
- Shared-element animation via `layoutId` — genuinely no one else does this well
- Spring-based gesture response and drag/drop physics
- Declarative variants and orchestrated `onAnimationComplete` callbacks

**Import path:** `import { motion } from 'motion/react'` (use `motion`, not `framer-motion`, for new code)

**Key APIs for Tauri:**

```tsx
// Enter/exit with layout
<AnimatePresence mode="popLayout">
  {items.map(item => (
    <motion.div key={item.id} layoutId={`item-${item.id}`} exit={{ opacity: 0 }} initial={{ opacity: 0 }} animate={{ opacity: 1 }}>
      {item.name}
    </motion.div>
  ))}
</AnimatePresence>

// Drag with spring physics
<motion.div drag dragElastic={0.2} dragTransition={{ type: 'spring', stiffness: 300, damping: 30 }} />

// Scroll-linked animation (see CSS approach instead for desktop)
import { useScroll, useTransform } from 'motion/react';
const { scrollY } = useScroll();
const opacity = useTransform(scrollY, [0, 300], [1, 0.5]);
```

**Gotchas:**

- React 19.0–19.2 compatible. R3F v9 pins `react <19.3`. If you plan to use both Motion and React Three Fiber, stay on React 19.0–19.2 for now.
- `useGSAP` is GSAP-specific; Motion doesn't need a Tauri context hook. Just use Motion directly.

**Windows/Tauri specifics:**

- Motion writes inline `style` attributes, so if you set a strict CSP, include `style-src-attr 'unsafe-inline'`.
- Runs on the main thread when animating layout; minimize animated list items if performance matters.

**When NOT to use:**

- Scroll choreography over many elements (GSAP ScrollTrigger is more capable).
- Complex keyframed sequences that need precise timing control (GSAP timelines).
- SVG morphing (GSAP MorphSVG).

### GSAP 3.15 (greensock)

**Current version:** 3.15.0 (2026-04-13)
**License:** GSAP Standard "no charge" (free, including all plugins, commercial use OK)
**Bundle:** ~27 KB gzip core; plugins load on-demand
**Weekly npm downloads:** 4.4M

**What it does best:**

- Authored timelines with frame-perfect sequencing
- Scroll choreography (ScrollTrigger — literally the most capable scroll tool on the web)
- SVG morphing (MorphSVG)
- Text effects (SplitText)
- Flip transitions (FLIP + state diffing)
- Animating *anything* including Three.js object properties

**All plugins are now free.** As of v3.13 (April 2025), Webflow made GSAP 100% open and free, including ScrollTrigger, SplitText, MorphSVG, DrawSVG, Inertia, Physics2D, ScrambleText, Flip. Read the [GSAP Standard License](https://gsap.com/community/standard-license/): permitted for all uses except building a no-code visual animation editor (which would compete with Webflow's tool).

**Key APIs for Tauri:**

```ts
// Timeline with sequence
gsap.timeline()
  .to('.header', { duration: 0.3, opacity: 0, y: -20 })
  .to('.content', { duration: 0.6, opacity: 1, y: 0 }, '<+=0.2')
  .to('.footer', { duration: 0.4, opacity: 1 });

// Scroll-trigger (WAY more capable than Motion's useScroll)
ScrollTrigger.create({
  trigger: '.section',
  start: 'top 80%',
  end: 'bottom 20%',
  onEnter: () => console.log('entered'),
  markers: true // dev-only visual markers
});

// SplitText for character/word animation
gsap.registerPlugin(SplitText);
const split = new SplitText('.headline', { type: 'chars,words' });
gsap.to(split.chars, { duration: 0.5, opacity: 0, y: 20, stagger: 0.05 });

// MorphSVG
gsap.to('circle', { morphSVG: '#target-shape', duration: 1 });
```

**React integration:** use the `useGSAP` hook (`@gsap/react` 2.1.2) to avoid tween leaks in StrictMode:

```tsx
import gsap from 'gsap';
import { useGSAP } from '@gsap/react';

export function MyComponent() {
  const containerRef = useRef(null);

  useGSAP(() => {
    // Tweens created here are scoped to this component
    gsap.to('.box', { duration: 1, x: 100 });
  }, { scope: containerRef });

  return <div ref={containerRef}><div className="box">Animate me</div></div>;
}
```

**Windows/Tauri specifics:**

- All GSAP code is imperative and lives outside React's render cycle — this is its strength and its gotcha. The `useGSAP` hook + `scope` pattern prevents accidental tween duplication in development.
- No CSP issues; GSAP doesn't write inline styles.

**When NOT to use:**

- Simple component enter/exit (Motion's `AnimatePresence` is cleaner).
- Layout animations (Motion's `layoutId` has no GSAP equivalent; write it with CSS or Motion).

### AutoAnimate

**Current version:** 0.10.0 (2026-07-10)
**License:** MIT
**Bundle:** 3.2 KB gzip
**npm:** ~1.2M/week

**What it does:** one line. Animates add/remove/move of a list/grid's direct children.

```tsx
import { useAutoAnimate } from '@formkit/auto-animate/react';

export function List({ items }) {
  const [parent] = useAutoAnimate();
  return (
    <ul ref={parent}>
      {items.map(item => <li key={item.id}>{item.name}</li>)}
    </ul>
  );
}
```

No configuration. Items fade in, fade out, and smoothly shift position when the list changes. That's it.

**Use when:** you have a list or grid and want effortless animation without building a timeline. Cost/benefit is unbeatable.

**Skip when:** you need custom animation logic (different timing per item, conditional animations, etc.). At that point, use Motion or hand-author with CSS.

### Rive

**Current version:** @rive-app/react-canvas 4.31.0 (2026-08-07)
**License:** MIT (runtimes); SaaS editor (paid)
**Bundle:** ~55 KB JS + WASM (~0.5–1 MB); can bundle locally
**npm:** ~1.0M/week

**What it does:** runtime for `.riv` files — designer-authored interactive vector animation with state machines, transitions, and data binding.

**Designer tool pricing:** Free ($0, 3 collaborative files); Cadet $9/seat/mo; Voyager $32/seat/mo.

**Use when:**

- You have a designer authoring mascots, complex loading states, or interactive toggles
- Interactive animation over traditional Lottie files
- Small file sizes matter (`.riv` files are tiny, usually <100 KB for complex animations)

**Windows/Tauri specifics:**

- Bundle the WASM locally: `@rive-app/canvas` ships an embedded WASM fallback, but for a desktop app, download the runtime `.wasm` and pass it to the runtime explicitly.
- Use `canvas` renderer on Windows, not `webgl2` — WebView2's GPU selection is broken and will pick the integrated GPU on dual-GPU laptops (see Tauri section below).
- No network dependency if you bundle the WASM + `.riv` files.

**Gotcha:** `.riv` files are opaque to your build pipeline and live only in the Rive cloud editor. If you need to adjust animation after shipping, you're editing in the SaaS tool. For internal tools, this is fine; for something that needs offline-editable sources, consider R3F + glTF instead.

### CSS-Native Approaches (the Win for Desktop)

Tauri v2 targets Chromium 151 (evergreen WebView2). This unlocks features that are compositor-threaded and run at full frame rate even when your main JS is busy:

| Feature | Since | What it replaces | Use case |
|---|---|---|---|
| **View Transitions** (same-document) | Chromium 111 | JS-driven route/modal animations | Cross-fade route changes, modal entry/exit |
| **View Transitions** (cross-document) | Chromium 126 | Turbo/SPA route transitions | Preserve scroll position across MPA routes |
| **`@starting-style`** | Chromium 117 | CSS resets for initial render | Animate `display: none → block` without JS |
| **`transition-behavior: allow-discrete`** | Chromium 117 | Discrete property timing | Auto-size accordion, reversible visibility |
| **`interpolate-size: allow-keywords`** | Chromium 129 | `useMeasure` hooks, JS height measurement | Animate `height: 0 → auto` with pure CSS |
| **Scroll-driven animations** (`animation-timeline: scroll()`) | Chromium 115 | ScrollTrigger, Lenis, main-thread scroll listeners | Scroll-progress bars, reveal-on-enter, parallax |
| **Popover API** | Chromium 116 | Popper.js, Floating UI | Tooltips, dropdowns, dismissible overlays — no portals needed |
| **Anchor positioning** | Chromium 125 | Floating UI middleware | Position sticky popovers to arbitrary elements |
| **Container queries** | Chromium 105 | Media queries for components | Component-scoped styling |

**Why this matters for Tauri:** these are **compositor-threaded**, meaning they animate smoothly even if your Rust backend is busy or JavaScript is parsing a large chunk. This is a categorical advantage over JavaScript-driven animation.

**Start here for anything scroll-linked:**

```css
/* Scroll-progress bar */
@supports (animation-timeline: scroll()) {
  #progress {
    animation: grow linear;
    animation-timeline: view();
    animation-range: entry 0% cover 100%;
  }
  @keyframes grow { to { width: 100%; } }
}
```

**View Transitions for route changes:**

```tsx
import { useNavigate } from 'react-router-dom';

const navigate = useNavigate();
const handleNav = () => {
  if (!document.startViewTransition) {
    navigate('/page'); // fallback
    return;
  }
  document.startViewTransition(() => navigate('/page'));
};
```

React Router also supports `<Link viewTransition>` / `navigate(to, { viewTransition: true })` — prefer that when you're already inside the router.

**Animate to `auto` (accordion pattern):**

```css
.accordion-content {
  max-height: 0;
  overflow: hidden;
  transition: max-height 0.3s ease-out;
}
.accordion-content.open {
  max-height: 1000px; /* just bigger than content */
}

/* Or with interpolate-size (Chromium 129+) for true auto: */
@supports (interpolate-size: allow-keywords) {
  :root { interpolate-size: allow-keywords; }
  .accordion-content {
    height: 0;
    overflow: hidden;
    transition: height 0.3s ease-out;
  }
  .accordion-content.open {
    height: auto;
  }
}
```

**When to reach for a library instead:** when you need spring physics, when you're animating a VDOM list with complex gesture interaction (Motion's strengths), or when the animation isn't scroll-driven or route-triggered. CSS is great for state-change animations; Motion/GSAP are great for gesture and timeline-based animation.

---

## Component Registries

### shadcn CLI v4

**Current version:** 4.16.2 (2026-08-06)

You install component source code into `components/ui/` and own it. The CLI has built-in awareness of **267 registries**. Install via:

```bash
npx shadcn@latest init
npx shadcn@latest add @aceternity/3d-card
npx shadcn@latest add @magicui/marquee
npx shadcn@latest add @motion-primitives/text-effect
npx shadcn@latest search @magicui --query "marquee"
```

**Key registries for Tauri:**

- `@aceternity` — 200+ components, heavy Motion use, high visual impact
- `@magicui` — 50+ components, lighter weight, CSS-heavy
- `@motion-primitives` — tasteful micro-interactions, beta but actively developed
- `@react-bits` — 110+ animated components (4 variants each: JS-CSS, JS-TW, TS-CSS, TS-TW)
- `@coss` — Cal.com's design system, rebuilt on Base UI, 500+ components
- `@kokonutui` — 100+ components, React/Tailwind/Motion
- `@basecn` — shadcn style using Base UI instead of Radix

**Offline gotcha:** many Aceternity and Magic UI components ship demo images from `images.unsplash.com`. Before shipping your Tauri app, grep the `components/` directory for `http` and replace remote URLs with local `src/assets/` paths.

**Colors/typography:** shadcn components use Tailwind CSS variables. To customize, edit `globals.css` (the `--primary`, `--secondary` etc. tokens) or use `shadcn init --defaults` and pick a different base.

### HeroUI v3 (March 2026 rewrite)

**Current version:** @heroui/react 3.2.4 (2026-08-07)

v3 removed Framer Motion entirely and ships CSS-based animations. Includes 75+ web components (buttons, tables, cards, modals, sidebars, form controls) with built-in accessibility via React Aria. **Single-package win** if you don't want to manage a registries ecosystem.

```bash
npm i @heroui/react @heroui/styles tailwindcss
npx shadcn@latest init # still works; HeroUI v3 is shadcn-compatible
```

v3 ships its own MCP server and Agent Skills for Claude Code — seamless AI integration.

**When to use HeroUI over shadcn:** you want batteries-included (forms, tables, sidebar built-in), you want no animation runtime overhead, or you want official AI-native tooling.

---

## MCPs for AI-Driven UI Work

### shadcn MCP (official, zero-config)

```bash
npx shadcn@latest mcp init --client claude
```

Or in `.mcp.json`:

```json
{
  "mcpServers": {
    "shadcn": {
      "command": "npx",
      "args": ["shadcn@latest", "mcp"]
    }
  }
}
```

No account, no key. Claude can browse registries, search components, view source before installing, and install via natural language. It reads your `components.json` so it knows your Tailwind version, aliases, base library (Radix vs Base UI), and icon set.

### Chrome DevTools MCP

```bash
claude mcp add chrome-devtools -s user -- npx chrome-devtools-mcp@latest
```

**Why:** performance traces with millisecond granularity. When animation is janky, Chrome DevTools MCP captures a trace showing exactly which frames dropped, whether it was paint/layout/composite, and where the CPU time went. Invaluable for debugging Tauri WebView2 animation issues.

### Figma MCP (if design-first)

```bash
claude plugin install figma@claude-plugins-official
```

Reads Figma designs, extracts component tokens, and pulls color/typography/layout data directly.

### Context7 (live docs for 1000+ libraries)

```bash
npx -y @upstash/context7-mcp@latest
```

Serves up-to-date docs for Motion, GSAP, Tailwind, shadcn, and hundreds of other libraries. Claude can query it directly rather than relying on training data.

---

## Tauri v2 Windows Specifics

**Tauri tooling as of 2026-08-10:** `@tauri-apps/api` 2.11.1, `@tauri-apps/cli` 2.11.4.

### WebView2 Is Chromium 151

- Evergreen; self-updates to the latest Chromium approximately every 4 weeks — Microsoft is moving to a **2-week cadence starting with v152** ([announcement](https://github.com/MicrosoftEdge/WebView2Announcements/issues/137)).
- All modern CSS features work: View Transitions, scroll-driven animations, `@starting-style`, popover API, anchor positioning, container queries, OKLCH colors, `:has()`, subgrid.
- **GPU selection is broken** — WebView2 defaults to the integrated GPU on dual-GPU laptops with no override. If your Tauri app uses heavy WebGL (R3F, Spline, Babylon.js), users with a discrete GPU will get iGPU performance and you won't know why. WebView2 issue [#5072](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5072) has been open since Jan 2025 with no fix.
- No `powerPreference: 'high-performance'` override available.

### Window Effects & Native Chrome

Custom titlebar + Mica visual effect:

```json
{
  "app": {
    "windows": [
      {
        "label": "main",
        "decorations": false,
        "transparent": true,
        "windowEffects": { "effects": ["mica"] },
        "width": 1200, "height": 800
      }
    ]
  }
}
```

Drag region in HTML:

```html
<div class="titlebar" data-tauri-drag-region>
  <button id="minimize">–</button>
  <button id="maximize">▢</button>
  <button id="close">✕</button>
</div>
```

```css
[data-tauri-drag-region] { app-region: drag; }
[data-tauri-drag-region] button { app-region: no-drag; }
```

```js
import { getCurrentWindow } from '@tauri-apps/api/window';
const w = getCurrentWindow();
document.getElementById('minimize').onclick = () => w.minimize();
document.getElementById('maximize').onclick = () => w.toggleMaximize();
document.getElementById('close').onclick = () => w.close();
```

**Window effects on Windows:**

- **Mica** — Windows 11 only, no jank during drag/resize, **use this**
- **Acrylic** — Windows 10v1809+, but causes jank during drag/resize on 10v1903+ and 11
- **Tabbed** — Windows 11 only
- **Blur** — Windows 7+, but causes jank on 11 build 22621

For Windows-only Tauri apps, use Mica + `transparent: true` and you're good.

### Animation Performance on WebView2

**Compositor-only properties** (60 fps even on integrated GPU):

- `transform` (translate, rotate, scale)
- `opacity`
- `filter` (but not large blur; keep it minimal)

**Causes jank:**

- Animating `width`/`height`/`top`/`left` (triggers layout)
- Animating `box-shadow`/`border-radius` (triggers paint)
- `backdrop-filter` with large blur radius over many elements while resizing
- Main-thread scroll handlers (use CSS `animation-timeline: scroll()` instead)

**GPU selection for WebGL:**

- Assume integrated GPU
- Budget accordingly; profile on actual Windows hardware

### Fonts & Icons (Offline)

**Fonts via Fontsource:**

```bash
npm i @fontsource-variable/inter @fontsource-variable/space-grotesk
```

```ts
// main.tsx
import '@fontsource-variable/inter';
import '@fontsource-variable/space-grotesk';
```

```css
@theme {
  --font-sans: "Inter Variable", system-ui, sans-serif;
  --font-display: "Space Grotesk Variable", sans-serif;
}
```

No Google Fonts CDN. All fonts bundled locally with your app.

**Icons via Lucide:**

```bash
npm i lucide-react
```

Tree-shakes per-icon. Already a shadcn default.

**Or, for offline Iconify:**

```bash
npm i @iconify/json unplugin-icons -D
```

```ts
// vite.config.ts
import Icons from 'unplugin-icons/vite';
export default { plugins: [Icons({ compiler: 'jsx' })] };
```

```tsx
// usage: automatically generates icon components at build time
import IconHeart from '~icons/lucide/heart';
```

---

## Project Setup Checklist

### 1. Init Tauri v2 + Vite + React 19

```bash
npm create tauri-app@latest -- --template vite-react
cd <project>
npm install
npm run tauri dev   # opens dev server + Tauri app window
```

### 2. Add Motion & GSAP

```bash
npm install motion gsap
npm install -D @gsap/react
```

### 3. Add shadcn

```bash
npx shadcn@latest init
# Select: TypeScript, Tailwind CSS, dark mode, project style: Default
```

### 4. Add fonts

```bash
npm i @fontsource-variable/inter @fontsource-variable/space-grotesk
# Edit main.tsx to import them
```

### 5. Add Tauri plugins

```bash
npm install @tauri-apps/plugin-window-state tauri-plugin-prevent-default
# then in src-tauri/: cargo add tauri-plugin-window-state tauri-plugin-prevent-default
```

Check the exact JS package name on npm before installing — Tauri renamed several plugin packages between v1 and v2.

### 6. Tailwind v4 needs no config file

Tailwind v4 (currently 4.3.3) drops `tailwind.config.js`. Just put `@import "tailwindcss";` in your globals.css and configure with `@theme`.

### 7. Create `.mcp.json` for Claude

```json
{
  "mcpServers": {
    "shadcn": { "command": "npx", "args": ["shadcn@latest", "mcp"] },
    "chrome-devtools": { "command": "npx", "args": ["-y", "chrome-devtools-mcp@latest"] }
  }
}
```

### 8. Performance settings

In `vite.config.ts`:

```ts
export default defineConfig({
  build: {
    target: 'chrome120', // raise from stale chrome105 default
    minify: 'esbuild'
  }
});
```

In `src-tauri/tauri.conf.json`:

```json
{
  "app": { "windows": [{ "label": "main", "visible": false }] }
}
```

Show the window from your frontend after first paint to avoid the white flash.

---

## Common Patterns

### Fade In on App Launch

```tsx
// main.tsx
function App() {
  useEffect(() => {
    // Ensure content is painted, then show window
    requestAnimationFrame(() => requestAnimationFrame(async () => {
      document.documentElement.classList.add('ready');
      await invoke('show_window');
    }));
  }, []);
  return <div className="app">...</div>;
}
```

```css
html {
  opacity: 0;
  transform: scale(.985);
}
html.ready {
  opacity: 1;
  transform: none;
  transition: opacity 0.3s ease-out, transform 0.4s cubic-bezier(0.2, 0.8, 0.2, 1);
}
```

```rust
// src-tauri/src/lib.rs
#[tauri::command]
fn show_window(window: tauri::Window) {
    let _ = window.show();
}
```

### Scroll-Progress Bar

```css
/* Tailwind v4 + CSS-only */
@supports (animation-timeline: scroll()) {
  #progress-bar {
    position: fixed;
    top: 0;
    left: 0;
    height: 3px;
    background: linear-gradient(to right, var(--color-primary), var(--color-accent));
    animation: progress linear;
    animation-timeline: view();
    animation-range: entry 0% cover 100%;
  }
  @keyframes progress {
    to { width: 100%; }
  }
}
```

### Modal with View Transition

```tsx
import { useNavigate } from 'react-router-dom';

export function ModalLink({ to, children }) {
  const navigate = useNavigate();

  return (
    <button onClick={() => {
      if (!document.startViewTransition) {
        navigate(to);
        return;
      }
      document.startViewTransition(() => navigate(to));
    }}>
      {children}
    </button>
  );
}
```

### List Animation with AutoAnimate

```tsx
import { useAutoAnimate } from '@formkit/auto-animate/react';

export function TodoList({ todos }) {
  const [parent] = useAutoAnimate();

  return (
    <ul ref={parent}>
      {todos.map(t => <li key={t.id}>{t.text}</li>)}
    </ul>
  );
}
```

### Timeline with GSAP

```tsx
import gsap from 'gsap';
import { useGSAP } from '@gsap/react';
import { useRef } from 'react';

export function AnimatedSequence() {
  const container = useRef(null);

  useGSAP(() => {
    gsap.timeline()
      .to('.step-1', { duration: 0.4, opacity: 1, y: 0 })
      .to('.step-2', { duration: 0.4, opacity: 1, y: 0 }, '-=0.2')
      .to('.step-3', { duration: 0.4, opacity: 1, y: 0 }, '-=0.2');
  }, { scope: container });

  return (
    <div ref={container}>
      <div className="step-1 opacity-0 translate-y-4">Step 1</div>
      <div className="step-2 opacity-0 translate-y-4">Step 2</div>
      <div className="step-3 opacity-0 translate-y-4">Step 3</div>
    </div>
  );
}
```

### Micro-Interactions with Motion

```tsx
import { motion } from 'motion/react';

export function ToggleButton({ active, onToggle }) {
  return (
    <motion.button
      onClick={onToggle}
      animate={{ backgroundColor: active ? '#3b82f6' : '#e5e7eb' }}
      whileHover={{ scale: 1.05 }}
      whileTap={{ scale: 0.95 }}
      transition={{ type: 'spring', stiffness: 300, damping: 30 }}
    >
      {active ? 'On' : 'Off'}
    </motion.button>
  );
}
```

---

## Sources

Full research from the August 2026 research cycle; npm versions re-verified 2026-08-10.

- [Motion.dev](https://motion.dev/)
- [GSAP](https://gsap.com/) + [GSAP Standard License](https://gsap.com/community/standard-license/)
- [GSAP now free announcement](https://webflow.com/blog/gsap-becomes-free)
- [Tauri v2](https://v2.tauri.app/)
- [shadcn/ui CLI v4](https://ui.shadcn.com/docs/cli)
- [HeroUI v3](https://heroui.com/docs/react/releases/v3-0-0)
- [Fontsource](https://fontsource.org/)
- [Rive](https://rive.app/)
- [AutoAnimate](https://auto-animate.formkit.com/)
- [WebView2 release cadence / Chromium 151+](https://github.com/MicrosoftEdge/WebView2Announcements/issues/137)
- [Base UI 1.7](https://base-ui.com/react/overview/releases)
- [Chrome DevTools MCP](https://github.com/ChromeDevTools/chrome-devtools-mcp/)
- `design-system.md` in this skill: the reference for taste + process
