# SpaceToggle OS — Future Ideas & Roadmap

This document serves as a repository for future concepts, architectural ideas, and enhancements that have been proposed but are not currently implemented in the main application.

## 1. "Typing Calibration" Utility (Smart Rollover)
*   **Concept:** The `rollover_ms` configuration shouldn't just be a static slider. 
*   **Implementation Idea:** Implement a "Typing Calibration" utility in the settings menu. The user types a sample sentence at their natural speed. The Rust backend measures their average KeyDown-to-KeyUp speed for the Spacebar and dynamically calculates the optimal `rollover_ms` (usually 1.2x their average keystroke duration) to guarantee a zero-bug typing experience customized to their fingers.

## 2. "App-State Awareness" (Dynamic Contextual Actions)
*   **Concept:** Context-dependent shortcuts based on the currently active window. 
*   **Implementation Idea:** Allow the active profile to override bindings dynamically. For example, in a "Designer" profile:
    *   If **Photoshop** is active: `Space + B` selects the Brush tool.
    *   If **Figma** is active: `Space + B` triggers a design element helper.
    *   If **Default**: `Space + B` opens the Browser.
*(Note: Currently deferred because Profile Switching via Space+RAlt handles most contextual workflow switching needs).*

## Pressing the Spaceadom wordmark / icon (top-left) — a delight animation
Requested 2026-08-18. Today it does nothing. Ideas from the owner, for later:
- the whole keyboard jumping in a WAVE, key by key, rippling from the wordmark
- a 3D rounding/roll of the board (needs care: transform-only, software-safe)
- any "very pleasing" one-shot animation in the app's existing motion language
Constraints when built: dashboard webview only, transform/opacity only, honour
reduced-motion, and it must never block input (pure decoration, interruptible).

## 8. The ring as a mouse-only command palette (parked 2026-09-17, owner: "keep for future")
*   **Where it came from:** the owner's brainstorm the morning after round 6 — "a lazy person who wants to keep one hand on the mouse: what would they want?" — plus a research pass over Hacker News threads on Kando / pie menus, AutoHotPie and PowerToys GitHub issues, and the tech press (Reddit was unreachable to the agent; a logged-in pass is still owed).
*   **Decided and built the same day:** Screenshot tile (Space + / → Win+Shift+S) and Voice Typing (Space + ; → Win+H).
*   **Ranked by evidence, none built yet:**
    1.  A user-defined **"send this shortcut" tile** — the #1 use of every pie-menu tool; turns the ring from a fixed list into a palette.
    2.  **Always-on-top toggle** for the window under the cursor (Windows has no shortcut; PowerToys' most-praised small utility).
    3.  **Snap left / right / quarter** for the window under the cursor.
    4.  **Clipboard history** (Win+V); Copy / Paste tiles for drag-select-then-paste one-handed.
    5.  **Minimise this / close this / maximise-restore** — window verbs the keyboard cannot aim.
    6.  **Show desktop, virtual desktop left/right, Task view, Lock, Sleep.**
    7.  **Mic mute, play/pause/next, volume by scrolling while the ring is up** (Space+scroll already proves the mechanics).
    8.  **Emoji picker (Win+.)**, undo/redo, select-all when over a text field.
    9.  **Browser verbs when over a browser:** new tab, close tab, reopen closed tab, back/forward.
    10. **Restart Explorer, Task Manager** — for when only the mouse still works.
    11. **Quick places:** Downloads, Recent, a chosen folder.
    12. **The compact on-screen keyboard as a tile** (see the 2026-09-17 chat: TabTip invocation first, drawing our own second).
*   **Design rules the research supports:** tiles should be VERBS ON WHAT IS UNDER THE CURSOR wherever possible (the thing no keyboard launcher can do); never move the cursor (done, round 6); keep the app small and native (Electron pie menus at 100 MB are mocked); hit-targets must not feel like "a wedge at an angle" (the proximity pick and the fisheye already answer that).
*   **Underserved, nobody ships well:** type-to-filter while the ring is up (hold middle, type a letter, the ring jumps); shareable ring layouts (export/import a ring as a file); a "decoupled" ring for CAD/design apps that never touches the pointer.

## 9. Launch plan (research pass 2026-09-17, agent over HN/press/subreddit rule pages; Reddit threads themselves still owed)
*   **Channels, ranked:** r/SideProject (welcomes "here's what I built": real demo GIF, story, stack, reply to every comment) → r/AutoHotkey (SpaceToggle OS was an AHK script: a real bridge) → r/Windows11 and r/software (read each one's live self-promo rules first) → winget / AlternativeTo / MajorGeeks (passive search discovery, list once) → a Windows-utility YouTuber (Chris Titus Tech is the model of GitHub + YouTube for a solo tool) → Product Hunt last (skews SaaS). ARM64 testers: windowsonarm.org's compatibility list and Discord.
*   **The "built with AI" angle:** "vibe-coded" apps are mocked in 2025-26 (insecure, abandoned, cookie-cutter). Lead with the OPPOSITE, which is true here: a systems-level Rust app, 16 MB, no telemetry, and the 880 KB engineering journal. Quirky pitch: "I let AI help me build a Windows tool — and wrote down every single time it screwed up. All 880 KB of it." Product line: "Hold Space, tap a letter — never touch the taskbar again."
*   **Video:** the gesture in the first second, before any name.
*   **Do-nots:** identical text across subreddits on one day; a download gated behind a signup; "AI built this" as the whole pitch.
*   **Two weeks:** Store live → r/SideProject → listings → 2-3 reels → r/AutoHotkey + r/Windows11 → one YouTuber → windowsonarm.org → LinkedIn/Instagram recap.
