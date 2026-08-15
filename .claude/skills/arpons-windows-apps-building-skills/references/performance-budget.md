# RAM, GPU & Performance Budgets

Tauri's selling point is that it is lighter than Electron. That advantage is easy to give away — a Tauri app can idle heavier than a Chrome tab if nobody is watching.

## Budgets

Targets for a Windows desktop utility. Measure against these rather than guessing.

| Metric | Good | Acceptable | Investigate |
|---|---|---|---|
| Idle RAM, tray utility (window hidden) | < 40 MB | < 80 MB | > 120 MB |
| Idle RAM, window visible | < 90 MB | < 150 MB | > 250 MB |
| Idle CPU, background | ~0% | < 0.3% | > 1% |
| Cold start to first paint | < 400 ms | < 800 ms | > 1.5 s |
| Warm start | < 200 ms | < 400 ms | > 700 ms |
| Installer size | < 5 MB | < 15 MB | > 30 MB |
| GPU memory | < 60 MB | < 150 MB | > 300 MB |
| Interaction to visible response | < 100 ms | < 200 ms | > 300 ms |

**Sustained CPU while idle is the worst offense** in a tray app. It drains battery, spins fans, and gets the app uninstalled. A background utility should be at genuine 0% when nothing is happening.

## Where the memory actually goes

A Tauri app's RAM is mostly **not** your code:

- **WebView2 runtime host process** — 30–60 MB baseline, unavoidable, shared across your windows
- **One renderer process per window** — 15–40 MB each
- **GPU process** — 20–50 MB, shared
- **Your Rust process** — usually 5–15 MB unless you are holding data
- **Your DOM, JS heap, images, canvas backing stores** — this is the part you control

Task Manager shows these as separate `msedgewebview2.exe` entries under your app. Judging by the Rust process alone gives a number three to five times lower than reality.

### Multi-window cost

**Every Tauri window is a separate WebView2 renderer process.** A main window plus an overlay is not 1.1× the memory — it is closer to 1.5–2×.

This creates a real tradeoff for overlays:

| Approach | RAM | Show latency |
|---|---|---|
| Pre-created hidden window | +20–40 MB always | ~5 ms |
| Create on demand | 0 when hidden | 100–300 ms |
| Single window, DOM-toggled overlay | ~0 extra | ~5 ms |

For a HUD that must appear instantly, pre-creating is usually right and the 30 MB is the price. But if the overlay can live in the main window's DOM — as a full-screen fixed layer — that is strictly better on both axes. Only use a separate window when it genuinely must be always-on-top over other applications, click-through, or outside the main window's bounds.

### Common leaks

- **Event listeners never removed.** Tauri's `listen()` returns an unlisten function. Not calling it on teardown leaks the handler and everything it closes over. Same for `setInterval`.
- **Detached DOM nodes.** Removing an element while a JS reference survives keeps the whole subtree alive. Frequent in hand-written vanilla code that caches `querySelector` results.
- **Unbounded arrays.** Log buffers, event history, undo stacks. Cap them explicitly.
- **`will-change` left on permanently.** Each promoted layer costs GPU memory. Add before animating, remove after.
- **Large base64 images in the DOM.** A base64 icon costs ~33% more than binary and is held as a string, decoded bitmap, *and* GPU texture simultaneously.
- **Rust `Vec`s that only grow.** Cached window handles, key history. Bound them.

## GPU

### The WebView2 GPU problem

WebView2 picks the **integrated GPU** on dual-GPU laptops and offers no override. `powerPreference: 'high-performance'` is ignored. [WebView2Feedback #5072](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5072), open since January 2025.

Consequences:

- A WebGL scene that runs at 120 fps on your desktop may run at 20 fps on a user's laptop with a discrete GPU sitting idle
- You cannot detect or fix this from inside the app
- Users can force it per-application in Windows Graphics Settings, but expecting that is unreasonable

**Budget for integrated graphics.** If a feature needs a discrete GPU, it does not belong in a WebView2 app.

### GPU memory costs

- Each composited layer: `width × height × 4 bytes`. A full-screen layer at 2560×1440 is ~14 MB.
- `backdrop-filter` needs a **backdrop copy plus the blurred result** — roughly double, and it re-renders whenever anything behind it moves.
- Canvas backing store at DPR 2 is 4× the CSS-pixel memory.
- `will-change: transform` promotes eagerly. Twenty elements with it is twenty layers.

Inspect layers in DevTools → Rendering → Layer borders. More than a handful of unexpected layers means over-promotion.

### backdrop-filter

The most expensive common CSS property, and the reason glassmorphism regularly tanks frame rate.

- One or two blurred surfaces is fine
- Blurred surfaces over animating content re-blur every frame
- `backdrop-filter` during window resize is the worst case — the whole backdrop recomputes per resize event
- Large blur radii cost more than small ones, superlinearly

For a glass HUD: use Windows' native **acrylic or Mica on the window itself** rather than CSS `backdrop-filter`. DWM composites it out of process, essentially free, and it looks more native. Reserve CSS blur for small in-window surfaces.

## Startup

Cold start breakdown for a typical Tauri app:

| Stage | Cost |
|---|---|
| Process start, Rust init | 20–50 ms |
| WebView2 runtime init | 100–250 ms |
| First HTML/CSS/JS parse | 30–100 ms |
| Your JS bootstrap | varies |
| First paint | — |

WebView2 init dominates and is not yours to optimize. What you control:

- **Do not block on Rust work before showing the window.** Registry reads, file scans, and network calls go after first paint, reported in over an event.
- **Ship less JS.** Every 100 KB of parsed JS is roughly 10–30 ms on a low-end machine.
- **Inline critical CSS** in `index.html` so first paint does not wait on a stylesheet request.
- **Lazy-load anything not on the first screen** — settings panels, charts, editors — via dynamic `import()`.
- **Set `build.target`** to `chrome120` or higher. Vite's default transpiles for browsers WebView2 will never be, producing bigger and slower output.
- **Fonts must be local.** A CDN font request stalls text rendering for the length of a network round trip, and fails entirely offline.

Avoid the white flash: start `"visible": false`, then show after first paint.

```ts
requestAnimationFrame(() => requestAnimationFrame(async () => {
  document.documentElement.classList.add('ready');
  await getCurrentWindow().show();
}));
```

## Binary and installer size

```toml
[profile.release]
codegen-units = 1
lto = true
opt-level = "s"
panic = "abort"
strip = true
```

Typical reductions: LTO plus `codegen-units = 1` cuts 15–30%; `strip` removes debug symbols (often several MB); `opt-level = "s"` trades a little speed for size.

The `windows` crate's feature list is a direct size lever — every enabled module compiles in. Audit it: feature lists tend to accumulate duplicates and modules added for since-deleted code.

Frontend side: run `npx vite-bundle-visualizer` and look for a large dependency pulled in for one function.

## Measuring

**RAM.** Task Manager → Details, sum your app's process and all `msedgewebview2.exe` children. Or from Rust with `sysinfo`. Measure after five minutes idle, not at launch — leaks show up over time.

**Frame rate.** WebView2 DevTools (`Ctrl+Shift+I` in dev) → Performance. Record while interacting. Look for long tasks over 50 ms and dropped frames.

**Rendering.** DevTools → Rendering → enable Paint flashing (green = repaint; a whole screen flashing on a small change means a layer problem) and Frame rendering stats.

**Rust timing.** `tracing` spans around commands:

```rust
#[tracing::instrument]
#[tauri::command]
async fn load_profiles() -> Result<Vec<Profile>, AppError> { /* ... */ }
```

**Chrome DevTools MCP** gives Claude direct access to traces — useful, with the caveat that it profiles Chrome, not WebView2. Numbers are directionally right; GPU behavior is not.

## Diagnostic order

When the app feels slow:

1. **Is it CPU or GPU?** Task Manager per-process. High GPU with low CPU points at compositing — layers, blur, over-promotion.
2. **Is it startup or runtime?** Different problems.
3. **Is it the frontend or Rust?** Time the command with `tracing`. A 300 ms blocking registry read looks exactly like a slow UI from the outside.
4. **Is it one window or all of them?** Per-renderer memory isolates it.
5. **Does it reproduce with animations disabled?** Toggle reduced motion. If it fixes it, the problem is layer or paint cost.
6. **Does it reproduce on integrated graphics?** If the machine has dual GPUs, force integrated in Windows Graphics Settings and retest — that is what most users will get regardless.

## Habits that keep it light

- Virtualize any list over ~200 rows
- Debounce resize and scroll handlers; better, use CSS scroll-driven animations and skip the handler
- `content-visibility: auto` with `contain-intrinsic-size` on long off-screen sections
- Ship SVG or WebP, not PNG, and never base64 anything large
- One shared IPC event stream rather than dozens of narrow listeners
- Explicit caps on every growing buffer
- Move heavy parsing to a Worker via `comlink`, or to Rust
- Remove `will-change` when the animation ends
- Re-measure idle RAM before each release; a slow climb across versions is easy to miss
