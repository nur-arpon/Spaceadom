# Spaceadom — what changed in each version

Every installer ever built lives in this folder. Both formats are kept for each
version: `Spaceadom_<v>_x64_en-US.msi` and `Spaceadom_<v>_x64-setup.exe`.

**To go back to any version:** run its `.msi`. Uninstall the current one first
from Add/Remove Programs, or the installer may exit successfully while leaving
the old exe in place (Tauri regenerates the product code every build).

**Your settings are not touched** by switching versions — profiles and bindings
live in `%APPDATA%\Spaceadom\config.json`, separate from the app. Rolling back
is safe.

**Missing: 1.0.28.** Deleted at your request on 2026-08-14 — it was the first
attempt at the toast/HUD transition and it made the HUD feel delayed. Every
other version is here.

---

## 2026-08-15 — the transition attempts

| Version | What changed |
| --- | --- |
| **1.0.32** | **Third transition attempt — the real fix.** Every animated property is now `transform`/`opacity` only. The previous versions animated width, height, background and border colour, which forces the browser to re-lay-out and repaint every frame — impossible to make smooth, especially on your laptop where the overlay runs in software rendering. Morph is now two faces cross-fading instead of one box resizing. |
| **1.0.31** | Three fixes after your feedback: toasts stopped appearing after a hold (a flag that blocked window updates was never cleared — the log showed 4 shortcuts fire with zero window fits); flights sometimes started off to the left (same root cause — the stale window rect); and the SPACE→toast animation, which existed but could never run because the engine cancels the HUD *before* the toast arrives. |
| **1.0.30** | Cleared the stuck flag that hid toasts after a hold. Superseded within minutes by 1.0.31. |
| **1.0.29** | **First warp implementation** — your patch, applied as written. Toast pills fly into the SPACE key and back out. Looked right on paper; in practice toasts stopped appearing after a hold. |
| **1.0.27** | **The clean baseline with no warp at all.** Rounded app icon, brand mark, undo system, everything else from 08-14. If the transition work ever needs abandoning, this is the version to come back to. |

## 2026-08-14 — icon, branding, undo

| Version | What changed |
| --- | --- |
| **1.0.27** | App icon corners rounded to a squircle (15.8%, matched to the icon's own inner frame), regenerated natively at 10 sizes so small icons stay sharp. Topbar mark nudged 2px down and closer to the wordmark. |
| **1.0.26** | Brand mark in the topbar is now your actual app icon, recoloured to the theme — warm brown on cream, soft blue in Nocturne. It had been a placeholder character (a circle with a dot) transcribed literally from the design file. |
| **1.0.25** | "Restore preset profiles" button in Settings — deleting a preset used to be a one-way door. Gamers and Professionals restored from backup. Log noise from the diagnostic counter throttled to once a minute. |
| **1.0.24** | **Undo is a stack.** Deleting two profiles in a row used to destroy the first undo silently. Undo windows tiered: 10s for profiles you create, 20s for stock, 30s for Founders. Red "Delete?" pill for ordinary profiles, full panel only for Founders. |
| **1.0.23** | Delete confirmation became a real in-app panel that waits for you. `window.confirm` doesn't render in this webview at all — the earlier warning was invisible. |
| **1.0.22** | Fallback-profile warning, second attempt (toast + tooltip). Still too brief to read. |
| **1.0.21** | Warning when deleting Founders — it's the fallback other profiles borrow bindings from. First attempt used a dialog that never appeared. |
| **1.0.20** | Restored your Founders profile from backup after you deleted it to test the fallback behaviour. |

## 2026-08-13/14 — the reliability pass

| Version | What changed |
| --- | --- |
| **1.0.19** | **Watchdog stopped crying wolf.** It had logged 260 errors in two days without catching a single real fault — it counted time you were away from the keyboard as "the hook is dead". Self-healing all intact; only the trigger changed. |
| **1.0.18** | Added logging to the profile-cycle path, which had been completely silent — a grep for it returned zero and read as "never worked". |
| **1.0.17** | 10-second undo for clear/reset/delete. "Reset to defaults" renamed and rescoped to the active profile only. |
| **1.0.16** | App picker: browse now always opens at the Start Menu, installers (`setup.exe` and friends) are refused, and the app list no longer silently stops at 60 entries. Conflicts "Details" button fixed — it was opening a panel and instantly closing it. |
| **1.0.15** | **Typing speed fixed.** The rollover window was narrower than the gap between your own keystrokes at every setting — measured: a 180ms spacebar hold turned 18 of 18 words into commands. Widened to 1.4× the interval; default is now 60wpm/280ms. **This is the version your friends tested.** |
| **1.0.14** | Config backups: 10 rolling copies, kept outside any folder an uninstaller owns. |
| **1.0.13** | "Reset to defaults" was a full factory reset that also erased the app's measurement of your graphics driver — the cause of the invisible HUD. Now resets the active profile only. Added the "Software overlay" toggle. |
| **1.0.12** | Pixel self-test made honest: it was sampling the whole desktop, so any window repainting behind the invisible overlay counted as "working". |
| **1.0.11** | Resilience pass: hook thread supervisor, lock-poison recovery, off-screen window guard, tiny-screen support, fatal-error dialog, and 5 other latent failures — none of which had happened to you yet. |

## Before 2026-08-13

`1.0.0`–`1.0.10` and `14.0.0` are kept here for completeness. Their details are
in `PROJECT_STATUS.md` (dated entries, newest first) and `V14_FIXES_AND_CODE.md`
(every problem with its root cause and the exact code that fixed it).

---

## Notes for future entries

Add a row when a version ships. Keep it to one or two plain sentences — what
changed and, where it matters, *why* or *who asked for it*. The point of this
file is to be readable months later without opening the code.

Worth recording when it applies:
- who reported the problem and roughly when
- whether a version was a failed attempt (so it isn't retried)
- which version is the safe fallback if something goes wrong
