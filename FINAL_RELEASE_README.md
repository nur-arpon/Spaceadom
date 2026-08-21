> **SUPERSEDED — historical only.** This file describes SpaceToggle OS **V12** and is kept for
> the reasoning it records, not as current guidance. Several of its statements
> are FALSE for the app as it ships today.
>
> For anything current, read in this order: **`CLAUDE.md`** (architecture and
> hard rules), **`V14_FIXES_AND_CODE.md`** (every problem with its fix),
> **`PROJECT_STATUS.md`** (the dated log), **`RELEASE_READINESS.md`** (what is
> left before release). Marked 2026-08-20 during the release-readiness pass.

# SpaceToggle OS V12 — Final Release

Welcome to **SpaceToggle OS V12**. This release marks the complete architectural migration from AutoHotkey V11 to a robust, high-performance Rust and Tauri foundation, while achieving 1:1 functional parity with the original scripts.

## V12 Parity Features

### 1. The Spacebar Modifier Gate
The core interaction model has been perfected. The application hooks the Spacebar universally across Windows. 
- Tapping `Space` passes through as a normal space character seamlessly.
- Holding `Space` turns your entire keyboard into a command matrix without leaking keystrokes to the OS.

### 2. Intelligent "Smart Cascade" App Summoning
Tapping an assigned key summons the associated application. If the app is already open, it intelligently toggles between `Minimized` and `Restored` states. If it was closed, it automatically restarts. This relies on an aggressive native `OnceLock` caching system to guarantee window handles aren't lost in the OS queue.

### 3. Native Audio Gating (Boss Key)
- **Space Toggle UI:** The Keyboard Layout Matrix has been overhauled. Keys are now larger, with the letter centered prominently (26px) and the app label tucked neatly beneath it (10px) to maximize space utilization.
- **Dynamic Smart Cascade:** Fully integrated. Tapping Space + Key focuses an open app window; tapping it again minimizes it. We've added robust internal fail-safes so SpaceToggle OS will never accidentally focus or minimize its own hidden HUD/Settings overlays.
- **Bypass Mode (Space + .):** Pauses all SpaceToggle OS shortcuts so you can type normally. The engine state machine is now rigorously reset upon toggle, eliminating the issue of the "Space" key getting stuck logically in the engine.
- **Boss Key (Space + Esc):** Instantly minimizes all windows and natively mutes system volume using direct Win32 broadcast API commands (bypassing legacy simulated inputs that some Windows versions ignore).
- **Settings Toggle:** Fully functional settings panel with persistent toggle logic. Settings changes (Rollover window, Guide HUD delay, PiP Opacity) are persisted locally.
- **Guide HUD and Toasts:** We've resolved the "white rectangular box" issue on Tauri transparent windows by removing hardware-accelerated blur filters, replacing them with a sleek solid `rgba(10, 15, 28, 0.95)` background.).
- **Frontend:** Tauri + Vite + TypeScript.
- **System Tray:** A non-blocking, asynchronous system tray manages the main dashboard lifecycle.

### Setup Instructions
1. Run the `setup.exe` installer. It installs per-user and never asks for an
   administrator password. (The `.msi` was dropped in 1.0.41 - PROBLEM 129.)
2. The application will launch into the system tray and prompt for Elevation if necessary.
3. Use the Dashboard to configure your `Founders`, `Gamers`, and `Professionals` profiles.
4. Enjoy!
