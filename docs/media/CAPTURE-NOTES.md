# Media owed for the README

Two visuals are referenced from the top of `README.md` and neither is in this
folder yet. Both are owed by the owner — here is exactly what was tried, why
it stopped short, and the fastest path to finish it.

## `hud-starry.png` — the hero screenshot

**What was checked.** `preview.html` + `src/preview.ts` render the real
dashboard components (`keyboard-matrix.ts`, `starry-sky.ts`, and friends)
against a stub config, specifically so the design can be inspected without
the Rust backend (`npm run dev`, then open `/preview.html?dark&sky`). This
was run and visually confirmed on 2026-09-05: the Starry-night theme, the
moon, the constellations and the galleon all render correctly at 1400×800.

**Why no PNG landed here.** The one piece the brief specifically asked for —
the radial "hold Space" HUD ring — cannot be captured through this harness at
all, only through the real app. Ring previews in Settings
(`components/controls.ts`, `showRingPreview`) call `invoke("preview_hud_layout",
…)`, a real Tauri command; outside the Tauri runtime `invoke()` throws, the
call is caught and swallowed by design (`showRingPreview`'s own comment:
"EVERY FAILURE IS SWALLOWED, on purpose"), and nothing is drawn — the ring
itself is rendered by the separate native overlay window described in
CLAUDE.md ("Window rules"), not by anything in the DOM this harness renders.
Confirmed by reading the source, not by guessing.

Beyond that, the agent session that looked into this had no tool able to
write the *pixel bytes* of a browser-rendered page to a file on disk — the
available browser tool returns screenshots inline for viewing, not as
saveable files, and a DOM-to-canvas workaround (serializing the page into an
SVG `<foreignObject>` and rasterizing that to a `<canvas>`) hit a tainted-canvas
security error when exported. So even the settings-panel fallback the brief
allowed for was viewed, not saved.

**Fastest path to finish it** (owner, on the real machine):

1. `npm run tauri dev` — the real app, not the browser preview.
2. Switch to the Starry night theme.
3. Hold Space for about half a second so the radial guide appears, and take a
   real screenshot (Win+Shift+S) while it's up. That is the shot the brief
   wants: the actual HUD, not a preview harness's idea of it.
4. Crop/save as `docs/media/hud-starry.png` at roughly 1400×800.

If a truer "hero" shot is wanted without the ring, the plain dashboard
(`npm run dev`, open `/preview.html?dark&sky` in any browser, screenshot at
1400×800) already looks right and needs no native app running — that one
*can* be done by an agent with an OS-level screenshot tool, just not by the
one used in this pass.

## A short GIF (mentioned in README, not yet named)

Nothing has been recorded. The natural one, given the one-sentence pitch at
the top of the README ("Hold Space, tap any app's initial letter — boom! it
opens. Hold Space, tap the app's initial again — boom! it's gone."), is a
5–10 second loop of exactly that: hold Space, tap a letter, app opens; repeat,
app minimises. Record at whatever the owner finds easiest (ScreenToGif,
Xbox Game Bar, etc.), keep it under a few MB, and drop it in this folder —
`docs/media/demo.gif` is the name README currently expects.
