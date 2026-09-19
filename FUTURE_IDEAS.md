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
*   **Reddit, read for real (2026-09-17, via the owner's logged-in session, JSON API):** native Windows utilities land on r/Windows11 / r/software right now (Dynamic Edge 609▲, Fluent Sensors 116▲, Everything-in-Start 120▲, ProtonSearch 70▲ on r/SideProject). The first questions every time: idle resource use (37▲), "documentation or GitHub?" (18▲), "how is it better than PowerToys?" (13▲) — answer all three in the post body. The AI backlash is real and specific: FluentTaskScheduler on r/Windows11 (2026-05) drew "is the tool LLM-generated?" (50▲) and "not using a vibe coded system tool" (19▲) — write the post in the owner's own voice, keep the AI-journal angle for r/SideProject and video, never for r/Windows11. Pie/radial menus are a small niche there (MightyPie 19▲, Radify 19▲): lead with "Hold Space, tap a letter", the ring is the second act. AHK users hand-roll cursor menus — the SpaceToggle-OS origin is the bridge for r/AutoHotkey.

## 10. The "little windows" the owner sketched (2026-09-18) — popup timer, popup notepad, circular picture frame

The owner showed a Sticky Notes window (pink, a "+", a "…", the B/I/U/strike/list/image bar at the bottom) and asked for a timer, a notepad and a round picture frame in Spaceadom's own design language — minimal, same feel as the dashboard. Judged the same day: none is a one-evening job, and two of them Windows already ships.

*   **What works TODAY without code:** bind Windows **Clock** (timers, focus sessions, world clock) and **Sticky Notes** to letters in the dashboard — the picker lists Store apps, launching is an AUMID activation — and they show up on the ring like any other app. That is the owner's own rule from the same day ("shortcut to the default ones of Windows itself").
*   **Popup timer (ours):** one small always-on-top Tauri window, `decorations: false`, Mica/acrylic like the toasts, one number, one ring that drains, presets (5 / 10 / 25 / custom), a chime through Core Audio. Opens from a ring tile, lives at a screen corner, dismisses on click. Medium: a new window + payload + a countdown thread + the design pass. Nothing hard, just a whole small feature.
*   **Popup notepad (ours):** the same window shell with a contenteditable, autosaved to `%APPDATA%\Spaceadom\notes\` as plain text, one note per colour, a "+" for another. Medium-plus because persistence, focus rules (it must NOT steal focus from the app the owner is working in when it opens from the ring) and the law-6 question (Space held over our own window) all apply.
*   **Circular picture frame:** needs a decision first — a round always-on-top viewer for one image (a photo of someone, a reference), or a webcam bubble? Both are a borderless layered window with a circular clip (`WS_EX_LAYERED` + region, or a transparent Tauri window with `border-radius: 50%` on the page). The webcam version means media capture in WebView2, which means the camera permission prompt — a different animal. Medium either way once it is decided.
*   **Design language for all three (so they don't look like Sticky Notes):** the dashboard's tokens — its radius, its type scale, its Mica surface, the accent from the OS theme (`os-theme` already reads it), no toolbar row at the bottom, actions on hover only, one control per window. A 24 × 24 grid; nothing smaller than the toast text.
*   **Order, if and when:** timer first (least state), then the frame (once decided), then the notepad.

## 11. Parked on 2026-09-19 with reasons (Phase A step 3, 1.0.118)

The owner's verdict after an hour on 1.0.117: the settings catalogue and most toggles are "a shortcut app, not a utility". The rule that came out of it is at the top of CLAUDE.md — **reversible, or it does not ship** — and everything below failed it or was not wanted. One line each, with the reason, so nobody re-proposes them cold.

*   **Keyboard detection (exact legends via `GetKeyboardLayout` / `GetKeyNameText`):** the LEGENDS are knowable — `GetKeyNameText` on each scan code under the active `GetKeyboardLayout` gives the real cap text — but the SHAPE (ISO vs ANSI, the ⏎ tall/wide, the extra `<>` key, a 60% board) is not exposed by Windows at all; it would need an ISO/ANSI heuristic from the layout id plus a "show me your keyboard" press-through pass where the user taps each key once. Parked: a wrong guess draws a board that is not his, and the press-through pass is a whole onboarding step for a cosmetic gain.
*   **Mic mute everywhere:** fails the rule — a muted mic is a state the user has to discover and recover from (the meeting where nobody hears you). Would need a persistent on-screen indicator AND auto-unmute on exit before it could ship.
*   **Stay awake (caffeine):** same rule — a laptop that never sleeps in a bag is a state to recover from; needs a persistent indicator and a hard auto-off on exit. Parked with mic mute.
*   **See-through window:** already exists — hold the middle button and scroll (`opacity.rs`, Space + wheel), so a bound key would be a second way to the same thing. Nothing to build.
*   **Auto profile by context (switch profiles by the foreground app):** owner: **never** — a wrong context ruins the whole experience, and the app cannot know what he is "doing", only what is in front.
*   **Windows settings catalogue + the slow toggles (Bluetooth, Wi‑Fi, dark mode, night light):** HIDDEN, not deleted, behind `FEATURES.windowsCatalogue` / `FEATURES.hazardousToggles` (`src/config/features.ts` mirrored by `src-tauri/src/features.rs`); everything stays compiled and tested and comes back by flipping two constants. Screen off and Sleep are neutralised at run time as well. Reason: "a shortcut app, not a utility", and screen off cost the owner a restart.
*   **Show desktop:** stays in the catalogue JSON, not listed anywhere; it returns as a tile on the mouse-ring palette (§8) rather than as a key.


## 12. Video guide — "everything this app can do" (2026-09-19)

Proven the same day it shipped: I asked the free Gemini, in plain
language, for a PowerShell one-liner to do what I wanted, pasted it into
Space + a key under Advanced mode › Run command, and it just worked. That
is the demo for the video — you do not need to know PowerShell, you need
to be able to ask for it. The guide walks the app top to bottom: hold
Space and tap (apps), the specials (boss key, PiP, force close, cycle
profile), the Space ring and the mouse ring, profiles, and then the
"ask an AI for a command, paste it, it is a shortcut now" moment as the
finale. Record after the Shortcuts page and the touchpad land, so the
guide is not re-shot in a month.

## 13. A whole-app "Chocolate" theme (2026-09-19)

The touchpad page's design (`design/spaceadom-touchpad_1.html`) carries a
complete dark-brown token set — surfaces, lines, four text levels, accent
per theme. I liked it enough to wonder about the whole app in it. Decision
for now: the touchpad PAGE can be chocolate (a Settings toggle, "Touchpad
page: Matches the app / Chocolate", default Chocolate; the home thumbnail
always matches the app). If chocolate wins after living with it, this
becomes the fourth theme — keyboard, overlay, both rings, settings and the
night scene all need the palette, so it is a project, not a toggle.

## 14. "Next microphone" (parked 2026-09-19)

Same code as "Next speaker" (1.0.125) on the capture side — cycle the
default input among connected, enabled microphones with a toast. Parked
until the speaker one has lived a while; a wrong default mic is the kind of
thing people only discover mid-call, so it needs the same care as mic mute.

## 15. "Next speaker": choose which outputs to cycle (parked 2026-09-19)

1.0.125 cycles EVERY active output. With SteelSeries Sonar installed that is
five virtual outputs plus the Realtek speakers, so reaching the speakers is
five presses. Next: a tick-list in Settings, "Cycle these outputs", every
device listed, all ticked by default; Bluetooth devices appear when
connected. Owner: "later we will let user choose which devices to cycle
between."

## 16. The two rings are one ring with two switches — and the settings panel needs a redesign (2026-09-19)

Owner's observation: "the mouse ring shows no names and the Space ring shows
names — that's the only difference." Not quite (the Space ring is a
full-screen cloud of every key, keyboard-driven; the mouse ring is a
cursor-anchored tile ring, pointer-driven, release-to-launch), but the
SETTINGS for them have sprawled: Point to launch, Middle button shows,
Middle-button ring shows, Choose your favourites, All layout, Ring layout,
Show special keys, Guide-to-toast motion, Guide HUD delay. The simplification
to design: two switches that apply to BOTH rings — "Ring style: Icons /
Pills" and "Show names" — plus "Favourites / All", and everything else
under Advanced. Also on the table: summoning the mouse ring while Space is
held (Space → Space ring, mouse → mouse ring stays the default).

Bigger: the settings panel itself — a full-screen settings page using the
blank space left and right of the keyboard, Advanced as a real section, a
Claude Design pass first. Parked until the touchpad and audio work settle.
