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
- The application must have a visual settings interface (Dashboard) accessible via the system tray. (This line used to say "or a dedicated
  shortcut, e.g. `Space + ,`" — that combo is Smart Search and has been since
  V13, and NO combo opens the dashboard. Corrected 2026-08-20 rather than
  implemented: a shortcut that opens a window would have to steal a key from
  the twenty-six the user owns.)
- The interface must allow users to map specific keys to specific apps or URLs across different profiles.
- **First-run tour (added 2026-09-04, PROBLEM 242).** The app must introduce
  itself once. On the first launch after an install the dashboard shows a
  guided first bind — the two-line promise ("Hold Space, tap any app's
  initial letter — boom ! it opens."), then pick a letter → bind it → use it
  → done. It is shown exactly once (`config.tour_done`), Skip means never
  again, and Settings' header carries "Show me the walkthrough" as the way
  back in. **This is a layman-friendliness aim, not a nicety:** section 1
  says the interface must be usable without writing code or scripts, and the
  one action the whole app rests on — HOLDING Space — cannot be discovered by
  pressing things, because the hook swallows Space system-wide. A user who
  never guesses it never fires a single shortcut. Removing the tour puts that
  discovery back on luck.
  - Its last step advances only on a REAL launch, reported by the engine
    (`st-launched`, `engine/mod.rs::handle_alpha`, gated on the cascade not
    having Failed). Do not re-implement it as a keyboard listener in the
    page: the page cannot see Space.
  - Step 3 deliberately un-dims the dashboard so the Guide HUD's ring appears
    naturally under the user's own hand. **That ring is the discovery moment
    — do not suppress the HUD during the tour** (owner's decision).

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
- **Middle-button ring (added 2026-09-10, PROBLEM 263)**: Holding the MIDDLE MOUSE BUTTON must raise the same Guide HUD in the same centred place as Space, with aim-and-release, click-a-chip and tap-a-letter all launching exactly as under a Space hold; a quick middle click must still reach the app as a real middle click (one replayed `SendInput` batch, our `0x7A7A7A7A` cookie), and inside 3D/CAD/design programs (built-in list in `hook/orbit_apps.rs`, the middle button only, never Space) the button must be untouched. Never two rings for one gesture: the Space hold and the middle hold refuse each other in one arbitration block in `hook/mod.rs`. Settings row "Middle button opens the ring", ON by default. UNPROVEN ON HARDWARE as of 2026-09-12.

### The Bypass Toggle
- Triggered via `Space + .`.
- Disables the entire hooking mechanism, allowing the Spacebar to revert to 100% vanilla OS behavior (useful for gaming).

## 3. The Prime Directive for AI
Any AI modifying this codebase **must** read this file. You are forbidden from "simplifying" or removing any of the features listed above. If a feature is broken, **fix it natively**; do not delete it. Parity with the original `install-v11.ps1` AutoHotkey logic is the absolute baseline.

---

## 4. Audit record

A contract nobody re-checks is a wish list. Each row says when the aim was last
verified, **how**, and what was found — because "it worked once" and "it works
now" are different claims, and this file is read as making the second one.

### 2026-08-24 — full audit against 1.0.72, at the owner's request

*"recheck if each and every single aim is fully functional, because all of this
is ai and so im worried if ai halucinated and skipped or made some unstable
stuff to the core aim."* Fair question, and two of the aims were not fully
functional. Method: every contract traced to the code that implements it, then
cross-checked against `%APPDATA%\Spaceadom\debug.log`.

| Aim | Verdict | Where it lives |
| --- | --- | --- |
| Tapping Space inserts a space | ✅ intact | `hook/mod.rs` `inject_space()`, cookie-tagged |
| Holding Space never leaks repeats | ✅ intact | Space-down always returns `LRESULT(1)` |
| Rollover protection | ✅ intact | `in_rollover` + one ordered `SendInput` batch |
| Dashboard reachable from the tray | ✅ intact | `tray.rs` |
| Key → app/URL mapping across profiles | ✅ intact | `key-detail-panel.ts`, `config/` |
| Smart Cascade — **launch** | ❌ **BROKEN** → fixed | PROBLEM 170 |
| Smart Cascade — focus / minimise cycling | ✅ intact | 4-step `force_foreground` ladder |
| Smart Cascade — cyclic reliability | ✅ intact | HWND cache with `IsWindow` validation |
| Boss Key (Win+M / Win+Shift+M + COM mute) | ✅ intact | `boss_key.rs` |
| Guide HUD on hold, hides on release | ⚠️ intact but **unreliable** → fixed | PROBLEMS 168, 169, 173, 175 |
| Toast on every action | ⚠️ intact but **blockable** → fixed | PROBLEM 175 |
| Bypass on `Space + .` | ✅ intact | incl. the escape hatch to turn it back off |

**The two real failures, and why they had gone unnoticed:**

1. **"If closed: Launch the app" was half-implemented.** The process started;
   nothing ever brought its window to the front. `force_foreground` existed and
   was called from four places, all of them focus-an-existing-window paths. It
   read as covered because the function was plainly there. PROBLEM 170.
2. **The Guide HUD's reliability, not its existence.** Four independent causes,
   each capable of the same symptom — a latched frontend flag, a topmost
   re-assert that was a tao no-op, primary-monitor-only placement, and Windows
   evicting the hook 17 times in one day. PROBLEMS 168/169/173/175.

**Nothing had been removed or simplified away.** Every aim was still present in
the code. What had happened is subtler and worth recording: features were
*surrounded* by later machinery — the HUD→toast flight, the compositing
self-test, the PiP topmost flag — and that machinery could block them while
every component still reported itself healthy. The failure mode of this codebase
is not deletion. It is a working feature made unreachable by something added
beside it.

**Verified how:** `cargo test --lib` (23 pass), a clean `cargo check`, the
1.0.73 installer verified on the real machine by version stamp and
bundle-freshness chain, and the owner's live `debug.log` re-read afterwards.
Behaviour that needs hands — a real cold launch landing in front, the HUD over a
second display — is listed as owner-verified in `PROJECT_STATUS.md`, not claimed
here.
