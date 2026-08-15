# Frontend Library Catalog

Versions verified against npm on **2026-08-10**. Re-check with `npm view <pkg> version`.

**Default to importing.** Anything in this file is solved, tested, and edge-cased better than a from-scratch implementation will be. Hand-write only when nothing here fits.

The `Vanilla` column matters: this project may have no framework. A `react-*` package in a plain-TS app is not an option.

---

## Design tokens — import, do not hand-write

Do not author a color ramp, spacing scale, or shadow set by hand. These ship tested ones as CSS variables.

| Package | Version | Vanilla | What it gives you |
|---|---|---|---|
| `open-props` | 1.7.23 | Yes | ~300 CSS custom properties: sizes, colors (OKLCH), shadows, easings, animations, gradients. Pure CSS, no build step, import only what you use. |
| `@radix-ui/colors` | 3.0.0 | Yes | 30+ hand-tuned color scales, 12 steps each, with automatic dark-mode counterparts. Each step has a defined role (step 9 = solid accent, step 11 = accessible text). Contrast is guaranteed by construction. |
| `@fluentui/tokens` | 1.0.0-alpha.23 | Yes | Microsoft's own Fluent design tokens. Still alpha — read the values, but pin carefully if you depend on it. |
| `modern-normalize` | 3.0.1 | Yes | 4 KB CSS reset. Fixes cross-browser defaults without opinionated styling. |

```bash
npm i open-props @radix-ui/colors modern-normalize
```

```css
@import "modern-normalize";
@import "open-props/style";        /* everything */
@import "open-props/easings";      /* or just the parts you want */
@import "@radix-ui/colors/slate-dark.css";
@import "@radix-ui/colors/blue-dark.css";

.button {
  background: var(--blue-9);
  color: var(--slate-12);
  border-radius: var(--radius-2);
  box-shadow: var(--shadow-2);
  transition: background var(--ease-out-3) 150ms;
}
```

Radix Colors is the single highest-leverage import here. Its steps are designed so that step 11 on step 2 always passes contrast, in both themes, for every scale — which removes an entire category of manual checking.

---

## Component libraries

| Package | Version | Vanilla | Notes |
|---|---|---|---|
| `@fluentui/web-components` | 3.0.3 | **Yes** | Microsoft's own Fluent components as standard Web Components. Framework-agnostic — works in plain TS, React, anything. Native Windows look without building controls yourself. **Start here for a Windows-native app with no framework.** |
| `@fluentui/react-components` | 9.74.5 | No | Fluent v9 for React. Large, comprehensive, Microsoft-maintained. |
| `@heroui/react` | 3.2.4 | No | 75+ components, React Aria accessibility, CSS animations (no Motion runtime as of v3). |
| `@base-ui/react` | 1.7.0 | No | Headless primitives from the MUI team. Unstyled, 35+ components. |
| `@radix-ui/react-*` | 1.1.x | No | Headless primitives. What shadcn/ui is built on. |
| shadcn CLI | 4.16.2 | No | Not a package — copies component source into your repo. 267 registries. |

For a vanilla-TS Tauri app targeting Windows, `@fluentui/web-components` is the obvious import:

```bash
npm i @fluentui/web-components
```

```ts
import { setTheme } from '@fluentui/web-components';
import { webDarkTheme } from '@fluentui/tokens';
import '@fluentui/web-components/button.js';
import '@fluentui/web-components/switch.js';

setTheme(webDarkTheme);
```

```html
<fluent-button appearance="primary">Save</fluent-button>
<fluent-switch id="bypass"></fluent-switch>
```

Import only the component modules you use — the package is side-effect-registered per component, so tree-shaking works.

---

## Animation

| Package | Version | Size | Vanilla | Use for |
|---|---|---|---|---|
| `gsap` | 3.15.0 | ~27 KB | **Yes** | Timelines, sequencing, ScrollTrigger, SplitText, MorphSVG, Flip. All plugins free since April 2025. |
| `motion` | 13.0.0 | ~44 KB / 2.3 KB mini | **Yes** (`motion` vanilla API) | Springs, gestures, layout animation. React API is `motion/react`; vanilla `animate()` works anywhere. |
| `@formkit/auto-animate` | 0.10.0 | 3.2 KB | **Yes** | One line animates add/remove/move in any list. Best cost-to-benefit ratio on this page. |
| `animejs` | 4.5.0 | ~17 KB | **Yes** | Lightweight timeline alternative to GSAP. v4 is a full rewrite. |
| `@react-spring/web` | 10.1.2 | ~28 KB | No | Spring physics for React. Motion covers most of this now. |
| `@use-gesture/react` | 10.3.1 | ~10 KB | Partial (`@use-gesture/vanilla`) | Drag, pinch, wheel, hover gesture normalization. |
| `@rive-app/canvas` | 4.31.0 | ~55 KB + WASM | **Yes** | State-machine vector animation. Use `canvas` renderer on Windows, not `webgl2`. |

For a vanilla project, GSAP plus AutoAnimate covers effectively everything:

```bash
npm i gsap @formkit/auto-animate
```

```ts
import autoAnimate from '@formkit/auto-animate';
autoAnimate(document.getElementById('profile-list')!);
```

```ts
import gsap from 'gsap';
gsap.timeline()
  .to('.hud', { duration: 0.2, opacity: 1, scale: 1, ease: 'power2.out' })
  .from('.hud-key', { duration: 0.25, opacity: 0, y: 8, stagger: 0.03 }, '-=0.1');
```

**Do not import an animation library for fades and slides.** CSS transitions with `@starting-style` handle those with zero bytes. Import when you need timelines, springs, gestures, or scroll choreography.

---

## Positioning & overlays

| Package | Version | Vanilla | Use for |
|---|---|---|---|
| `@floating-ui/dom` | 1.8.0 | **Yes** | Tooltips, dropdowns, popovers that flip and shift to stay on screen. Handles multi-monitor edge cases correctly. |

The native **Popover API** and **CSS anchor positioning** now cover simple cases with no dependency. Import Floating UI when you need collision detection against arbitrary boundaries, virtual reference elements, or an arrow that tracks the anchor.

---

## State, data, forms

| Package | Version | Vanilla | Use for |
|---|---|---|---|
| `nanostores` | 1.4.2 | **Yes** | ~1 KB atomic stores, framework-agnostic. The right choice for vanilla TS. |
| `zustand` | 5.0.14 | Partial | Small store, mostly React. |
| `jotai` | 2.20.2 | No | Atomic state for React. |
| `zod` | 4.4.3 | **Yes** | Runtime schema validation. Use it on the boundary where Rust JSON enters TS — this catches IPC contract drift immediately. |
| `@tanstack/react-query` | 5.101.4 | No | Async cache. Overkill for local IPC; use for real network work. |
| `@tanstack/react-table` | 9.1.2 | No | Headless table logic. |
| `@tanstack/react-virtual` | 3.14.9 | No | Virtualization. See below for the vanilla path. |
| `react-hook-form` | 7.85.0 | No | Forms. |
| `idb` | 8.0.3 | **Yes** | Promise wrapper over IndexedDB. |
| `dexie` | 4.4.4 | **Yes** | Higher-level IndexedDB with queries. |
| `comlink` | 4.4.2 | **Yes** | Makes a Web Worker callable like a normal object. Use to move parsing or heavy loops off the UI thread. |

**Persist app state in Rust, not the browser.** `tauri-plugin-store` writes real JSON to disk that survives a WebView data-directory reset. IndexedDB in a Tauri app is fine for caches, wrong for user settings.

**Validate the IPC boundary with Zod.** Rust `serde` and TypeScript interfaces drift silently, and the failure shows up as `undefined` deep in a render. Better still, generate the types — see `tauri-specta` in `libraries-rust.md`.

---

## Virtualization

Any list over ~200 rows needs virtualization or it will cost real RAM and scroll frames.

| Package | Version | Vanilla |
|---|---|---|
| `@tanstack/virtual-core` | (core of 3.14.9) | **Yes** |
| `@tanstack/react-virtual` | 3.14.9 | No |

`virtual-core` is the framework-agnostic engine and works in plain TS. CSS `content-visibility: auto` with `contain-intrinsic-size` is a zero-dependency partial substitute that helps paint cost but not memory.

---

## Charts

| Package | Version | Vanilla | Notes |
|---|---|---|---|
| `uplot` | 1.6.32 | **Yes** | ~45 KB, extremely fast, built for dense time-series. Best choice for a resource monitor or live graph. |
| `chart.js` | 4.5.1 | **Yes** | General purpose, canvas-based, good defaults. |
| `d3` | 7.9.0 | **Yes** | Full toolkit. Import submodules (`d3-scale`, `d3-shape`), never the meta-package. |
| `recharts` | 3.10.1 | No | React, SVG-based. Struggles past a few thousand points. |

Canvas over SVG for anything above ~1000 points — SVG creates a DOM node per element, which shows up directly in memory.

---

## 3D

| Package | Version | Vanilla |
|---|---|---|
| `three` | 0.185.1 | **Yes** |
| `@react-three/fiber` | 9.7.0 | No |
| `@react-three/drei` | 10.7.8 | No |

**Think hard before adding 3D to a Windows desktop app.** WebView2 has no GPU preference override and defaults to the integrated GPU on dual-GPU laptops. A scene that runs at 120 fps on your machine may run at 20 fps on a user's, with no diagnostic. See `performance-budget.md`.

---

## Utilities

| Package | Version | Vanilla | Use for |
|---|---|---|---|
| `date-fns` | 4.4.0 | Yes | Date formatting. Tree-shakes per function. |
| `clsx` | 2.1.1 | Yes | Conditional class names. |
| `tailwind-merge` | 3.6.0 | Yes | Resolves conflicting Tailwind classes. |
| `class-variance-authority` | 0.7.1 | Yes | Typed component variants. |
| `lucide` / `lucide-react` | 1.31.0 | Yes (`lucide`) | 1500+ icons, consistent 24 px grid and 2 px stroke. |
| `cmdk` | 1.1.1 | No | Command palette for React. |
| `sonner` | 2.0.8 | No | Toasts for React. |
| `embla-carousel` | 8.6.0 | Yes | Carousel. |
| `@fontsource-variable/inter` | 5.3.0 | Yes | Self-hosted variable font. |

For offline icons without a runtime dependency:

```bash
npm i -D unplugin-icons @iconify/json
```

Icons compile to inline SVG at build time — no runtime library, no network, no icon font.

---

## Build tooling

| Package | Version |
|---|---|
| `vite` | 8.2.1 |
| `typescript` | 7.0.2 |
| `tailwindcss` | 4.3.3 |
| `@tailwindcss/vite` | 4.3.3 |
| `postcss-preset-env` | 11.3.2 |

Tailwind v4 needs no `tailwind.config.js` — use `@import "tailwindcss"` and configure with `@theme` in CSS. Install the Vite plugin rather than the PostCSS path.

Set the build target explicitly, since Vite's default is far below what WebView2 supports and produces needless transpilation:

```ts
export default defineConfig({
  build: { target: 'chrome120', minify: 'esbuild' },
});
```

---

## What not to import

- **Lenis / smooth-scroll libraries.** Scroll-jacking is hostile on Windows, where users expect the OS scroll behavior their settings define.
- **`moment`.** Unmaintained and large. `date-fns` or `Intl.DateTimeFormat`.
- **`lodash` whole.** Import individual functions, or use the built-in equivalents that now exist.
- **`axios`.** `fetch` is native; for anything crossing a network, prefer doing it in Rust with `reqwest` and passing results over IPC.
- **jQuery, Popper.js, `normalize.css`.** Superseded.
- **A framework, to fix one animation.** If the project is vanilla TS, keep it vanilla.
