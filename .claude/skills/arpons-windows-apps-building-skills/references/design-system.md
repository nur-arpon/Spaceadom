# Design System: Ratios, Type, Color, Elevation

Numbers to pick from. Consistency across a screen matters more than the specific value chosen, so pick from one scale and stay in it.

## Spacing — 4 px base

Every gap, pad, and margin comes from this scale. Nothing between steps.

```
4  8  12  16  20  24  32  40  48  64  80  96  128
```

As CSS variables:

```css
:root {
  --space-1: 4px;   --space-2: 8px;   --space-3: 12px;  --space-4: 16px;
  --space-5: 20px;  --space-6: 24px;  --space-8: 32px;  --space-10: 40px;
  --space-12: 48px; --space-16: 64px; --space-20: 80px; --space-24: 96px;
}
```

Typical assignments:

| Relationship | Value |
|---|---|
| Icon to its label | 8 |
| Inside a button (vertical / horizontal) | 8 / 16 |
| Between form fields | 16 |
| Card interior padding | 16–24 |
| Between cards in a grid | 16 |
| Between subsections | 24–32 |
| Between major sections | 48–64 |
| Window edge padding | 24 |

**Proximity is grouping.** Elements that belong together must be closer to each other than to anything else. A label 16 px from its input and 16 px from the next field reads as ungrouped — use 8 and 24 instead. Most "cluttered" interfaces are actually uniform-spacing interfaces.

## Type scale

For a desktop app on Windows, use the Fluent type ramp. These are the real WinUI values, and matching them makes an app sit correctly next to system UI.

| Style | Size | Line height | Weight |
|---|---|---|---|
| Caption | 12 px | 16 px | Regular |
| Body | 14 px | 20 px | Regular |
| Body Strong | 14 px | 20 px | Semibold |
| Body Large | 18 px | 24 px | Regular |
| Subtitle | 20 px | 28 px | Semibold |
| Title | 28 px | 36 px | Semibold |
| Title Large | 40 px | 52 px | Semibold |
| Display | 68 px | 92 px | Semibold |

Default UI font is **Segoe UI Variable**. Windows uses Semibold for emphasis — Bold and Italic are not part of the ramp.

Source: [Microsoft Learn — Typography](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/typography)

**For a custom (non-Fluent) look**, generate a scale from a ratio instead. Multiply upward from a 16 px base:

- **1.2 (minor third)** — dense, information-heavy UI: 13, 16, 19, 23, 28, 33
- **1.25 (major third)** — general purpose: 13, 16, 20, 25, 31, 39
- **1.333 (perfect fourth)** — expressive, marketing-leaning: 12, 16, 21, 28, 38, 50

Round to whole pixels. Use 4–5 steps, not 9 — an unused step is a step someone will misuse.

### Type rules that matter more than the scale

- **Line height falls as size rises.** Body text 1.5; headings 1.2–1.3; display 1.1. A 40 px heading at 1.5 line-height looks broken apart.
- **Measure: 45–75 characters.** In a desktop app, cap prose containers at `max-width: 65ch`. Text running the full width of a 1400 px window is unreadable regardless of font.
- **Two weights per screen is usually enough**, three is the ceiling. Regular plus Semibold covers nearly everything.
- **Never letter-space lowercase body text.** Tighten large display type slightly (`-0.02em` at 40 px+); leave everything under 24 px alone. Uppercase labels take `+0.05em`.
- **One typeface is fine.** If using two, make them obviously different (a geometric display face against a neutral UI face), never two similar sans-serifs.

### Bundling fonts

```bash
npm i @fontsource-variable/inter
```

```ts
import '@fontsource-variable/inter';
```

Segoe UI Variable is already on Windows 11 and needs no bundling — reference it directly. It is not licensed for redistribution, so ship a bundled fallback for anything that might run elsewhere.

```css
:root {
  --font-ui: "Segoe UI Variable Text", "Segoe UI", "Inter Variable", system-ui, sans-serif;
  --font-display: "Segoe UI Variable Display", "Segoe UI", "Inter Variable", sans-serif;
  --font-mono: "Cascadia Code", "Cascadia Mono", ui-monospace, monospace;
}
```

Segoe UI Variable has size-optimized optical variants: **Small** below 12 px, **Text** for 12–18 px, **Display** above 18 px. Using Display for body text is a subtle but real reason UI can look slightly off.

## Color

Author in **OKLCH**. It is perceptually uniform, so equal lightness steps look equally different — unlike HSL, where `hsl(60 100% 50%)` (yellow) is far brighter than `hsl(240 100% 50%)` (blue) at the same stated lightness. Supported in WebView2.

```css
:root {
  --accent: oklch(0.62 0.19 258);
  --accent-hover: oklch(0.67 0.19 258);   /* +0.05 L */
  --accent-active: oklch(0.57 0.19 258);  /* -0.05 L */
}
```

### Building a neutral ramp

Neutrals should not be pure gray. Shift chroma slightly toward the accent hue (0.005–0.02) so the interface reads as one temperature.

```css
/* Dark theme — the default for most Windows utilities */
--bg-base:      oklch(0.17 0.008 258);  /* window background */
--bg-layer-1:   oklch(0.21 0.008 258);  /* cards, panels */
--bg-layer-2:   oklch(0.25 0.008 258);  /* raised, hover */
--bg-layer-3:   oklch(0.29 0.008 258);  /* menus, popovers */
--border-subtle:oklch(0.30 0.008 258);
--border:       oklch(0.38 0.008 258);
--text-muted:   oklch(0.62 0.008 258);
--text-second:  oklch(0.76 0.008 258);
--text:         oklch(0.95 0.005 258);
```

**Never pure black or pure white.** `#000` on an OLED display causes visible smearing during scroll, and pure white text on pure black creates halation. Floor around `oklch(0.15)` and ceiling around `oklch(0.96)`.

### Contrast requirements

- Body text: **4.5:1** minimum against its background
- Large text (18 px+, or 14 px semibold): **3:1**
- UI component boundaries and focus indicators: **3:1**

Do not eyeball this. In dark themes the failure is almost always muted text on a raised surface, where the surface got lighter but the text did not.

### Using color

- **One accent.** A second accent needs a reason — usually semantic (destructive red, success green), never decoration.
- **Accent means state, not decoration.** Selected, focused, active, primary action. An accent-colored heading communicates nothing.
- **Semantic colors** should carry a non-color cue too — an icon or text — since color-blind users and grayscale screenshots both lose the distinction.
- **Read the Windows accent color** rather than hardcoding, when the app should feel system-integrated. See `windows-platform.md`.

## Radius

Radius scales with surface size. A uniform 8 px on everything is a common tell.

| Surface | Radius |
|---|---|
| Small controls: checkbox, badge, tag | 4 px |
| Buttons, inputs, dropdowns | 4 px |
| Cards, panels, list items | 8 px |
| Dialogs, flyouts, menus | 8 px |
| Window corners (Win11 handles this) | 8 px |
| Circular: avatars, icon buttons | 50% |

These are the WinUI values. For a custom look, keep the ratio: nested elements need a smaller radius than their container, specifically `inner = outer − padding`. An 8 px card with 8 px padding needs 4 px children, or the corners look wrong where they nest.

## Elevation

Windows 11 carries depth mostly through **layered surface tint**, not drop shadow. Each layer is lighter than the one beneath (in dark theme). Shadows appear only on genuinely floating surfaces — flyouts, dialogs, tooltips.

```css
--shadow-flyout: 0 8px 16px oklch(0 0 0 / 0.14), 0 0 1px oklch(0 0 0 / 0.12);
--shadow-dialog: 0 32px 64px oklch(0 0 0 / 0.19), 0 0 2px oklch(0 0 0 / 0.15);
```

Two layers each: a large soft shadow for the cast, a tight one for the contact edge. A single large blur looks like a sticker.

Cards and list items inside a window get **no shadow** — separate them with a background tint step, or a 1 px `--border-subtle`, not both.

## Layout

**Grid.** 12 columns divides by 2, 3, 4, and 6, which covers nearly every layout. Gutter 16 or 24.

**Sidebar widths.** Navigation rail 48 px (icons only), compact sidebar 200 px, standard 240–280 px. Below 200 px labels truncate; above 320 px it eats content.

**Content max-width.** Prose 65ch. Forms 480–560 px — a full-width form across 1400 px forces long eye travel between label and field. Dashboards can use full width since they are scanned, not read.

**Optical alignment.** Circular and triangular shapes need to overshoot their box slightly to look aligned with rectangles. A play triangle in a round button needs ~1–2 px right offset to appear centered, because its visual mass sits left of its bounding box. Trust the eye over the number here — this is the one place where measurement lies.

**The 8-point grid conflict.** Fluent controls are 32 px tall, which is on the 4 px grid but not 8. Use 4 px as the base unit and everything reconciles.

## Density

Windows apps run on 24-inch monitors at arm's length, not phones. Web-derived spacing usually feels bloated in a desktop window.

| Element | Desktop height |
|---|---|
| Standard button / input | 32 px |
| Compact list row | 32 px |
| Standard list row | 40 px |
| Comfortable list row | 48 px |
| Menu item | 32 px |
| Toolbar | 40–48 px |
| Titlebar | 32 px |

Minimum click target is 32×32 px for mouse. Touch targets need 40×40 px — relevant for Surface devices and any tablet-mode use.

## Checklist before calling a UI done

- Every spacing value is on the 4 px scale
- Every font size is from one ramp
- Radii scale with surface size; nested radii are reduced by their padding
- One accent color, used for state
- Body text passes 4.5:1 in both themes
- No pure black, no pure white
- Related items are visibly closer than unrelated items
- Focus states are visible on every interactive element, at 3:1 minimum
- Layout holds at the window's minimum size
- Layout holds at 125%, 150%, and 200% DPI scaling
