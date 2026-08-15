# SpaceToggle OS — Core Mission & Non-Negotiable Aim

## 1. The Core Philosophy
SpaceToggle OS transforms the **Spacebar** from a simple character key into a universal, system-wide hyper-modifier, effectively turning the entire keyboard into an instant command matrix.
The **undeniable, non-negotiable** goal is to provide a "layman-friendly" visual interface where users can easily assign and change their preferred apps or websites to any `Space + Key` shortcut without writing code or scripts, and **without disturbing normal typing**.

## 2. Non-Negotiable Functionalities

### The Modifier Gate (Typing Protection)
- **Tapping Space**: Must always insert a normal space character seamlessly.
- **Holding Space**: Must never leak repeated space characters to the OS.
- **Rollover Protection**: Fast typing (e.g., hitting a letter slightly before releasing Space) must pass through as normal typing, not trigger a shortcut.

### The App Interface (Dashboard)
- The application must have a visual settings interface (Dashboard) accessible via the system tray or a dedicated shortcut (e.g., `Space + ,`).
- The interface must allow users to map specific keys to specific apps or URLs across different profiles.

### Smart Cascade (Summon & Vanish)
- Pressing `Space + Key` must act intelligently:
  1. **If closed**: Launch the app.
  2. **If open but in background**: Restore and bring to the absolute foreground.
  3. **If open and in foreground**: Minimize the app instantly.
- **Cyclic Reliability**: This behavior must loop flawlessly over and over (Launch -> Minimize -> Restore -> Minimize -> Restore, etc.) using robust Window Handle (HWND) caching and native OS focus forcing.

### Boss Key (Audio & Visual Privacy)
- Triggered via `Space + Esc`.
- **First Press**: Must minimize all active windows using the native `Win+M` shortcut **AND** natively mute the system volume.
- **Second Press**: Must restore all minimized windows using the native `Win+Shift+M` shortcut **AND** natively unmute the system volume.

### Visual HUDs
- **Guide HUD**: Holding Space for >300ms must summon a native glassmorphism overlay showing the current profile's shortcuts. Releasing Space or triggering a combo must instantly hide it.
- **Toast Notifications**: Every action (Bypass toggled, Boss Key engaged, App summoned) must have a clean UI notification overlay.

### The Bypass Toggle
- Triggered via `Space + .`.
- Disables the entire hooking mechanism, allowing the Spacebar to revert to 100% vanilla OS behavior (useful for gaming).

## 3. The Prime Directive for AI
Any AI modifying this codebase **must** read this file. You are forbidden from "simplifying" or removing any of the features listed above. If a feature is broken, **fix it natively**; do not delete it. Parity with the original `install-v11.ps1` AutoHotkey logic is the absolute baseline.
