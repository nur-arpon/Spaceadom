# Spaceadom — what changed in each version

Every installer ever built lives in this folder.

**From 1.0.41 onward there is only one format: `Spaceadom_<v>_x64-setup.exe`.**
The `.msi` was dropped — see the 1.0.41 row for why. Versions up to 1.0.40 have
both files; use their `setup.exe` too, for the same reason.

**To go back to any version:** run its `setup.exe`. It installs into your own
user folder, needs no admin password, and replaces whatever is there — you do
not need to uninstall first.

**One warning about the old `.msi` files, if you ever run one:** an `.msi`
installs to `C:\Program Files` instead, which is a SEPARATE copy. Windows then
has two Spaceadoms that cannot see each other, and both try to start when you
log in — two programs fighting over your spacebar. If you ever do it by
accident, uninstall "Spaceadom" from Add/Remove Programs and reinstall from a
`setup.exe`.

**Your settings are not touched** by switching versions — profiles and bindings
live in `%APPDATA%\Spaceadom\config.json`, separate from the app. Rolling back
is safe.

**Missing: 1.0.28.** Deleted at your request on 2026-08-14 — it was the first
attempt at the toast/HUD transition and it made the HUD feel delayed. Every
other version is here.

---

## 2026-08-17 — one app, one install

| Version | What changed |
| --- | --- |
| **1.0.41** | **You had Spaceadom installed twice, and neither copy could see the other.** This was found by checking your machine, not because it crashed — but it was about to bite. One copy was v1.0.37 in `C:\Program Files` (put there by the `.msi`), the other v1.0.40 in your own user folder (put there by the `setup.exe`). Windows treats those as two different programs. Both were set to start when you log in, so at your next restart two Spaceadoms would have launched and both would have grabbed the spacebar. You would not have seen "two apps running" — you would have seen Space+D opening Discord twice, or your settings mysteriously reverting. Fixed three ways: the old copy was removed from your machine; the `.msi` is no longer built at all; and the `setup.exe` now says in writing that it installs to your user folder. **What this means for you day to day: updates never ask for an admin password again.** What it costs: there is no `.msi` for company IT departments that require one — say the word if you ever need that back. Also proven for the first time today: an update installed *while Spaceadom was running* actually replaced the program. Every "successful" update before this one had quietly left the old version in place. |
| **1.0.40** | **The keyboard at 0.75, as you specified.** You said "make the keyboard layout 0.75 times of what is running right now" — so instead of the keyboard growing to fill whatever space exists, it now takes three quarters of it and leaves the last quarter as breathing room. That ratio holds on every display: your laptop, both external monitors, and small laptops too. (1.0.39's version of this never actually reached your screen — the update bug above kept 1.0.37 on disk — which is why it still looked unchanged to you.) |

## 2026-08-16 — two bugs killed at the root

| Version | What changed |
| --- | --- |
| **1.0.39** | **The update bug you caught, and the scaling you asked for.** (1) You found it yourself: running an update while Spaceadom is open showed a "Files in Use" dialog — and in a silent install, where nobody can answer that dialog, the update reported success while actually installing nothing. Since Spaceadom starts with Windows, it is always open during an update, so every silent update failed this way. The installer now closes the app itself before replacing files, then restarts it. (2) 1.0.36's big-screen scaling went too far the other way — the keyboard filled the window edge to edge with a fixed sliver of margin. Now the keyboard and the space around it grow together: on your external monitor the board is about 1.3x with real breathing room instead of 1.6x wall-to-wall. Small laptops are untouched. (1.0.38 was the installer fix alone; it was never installed and is superseded by this.) |
| **1.0.37** | **Getting ready for strangers.** (1) The app could crash at start-up on a busy or low-memory machine — a background thread that failed to start took the whole program with it, before the keyboard was even hooked up. It now carries on without that one feature and says so in the log. (2) If the app ever does crash, it now writes down what happened and exactly where, so a problem you hit can actually be diagnosed. Before, a crash left nothing at all. (3) Uninstalling used to leave behind the entry that starts Spaceadom when you log in — so Windows kept trying to launch a program that was no longer there, forever. Uninstalling now removes it, including entries left by older versions under the app's previous names. Your settings are deliberately kept, so reinstalling does not cost you your bindings; PRIVACY.md says where they are if you want them gone. (4) Added a privacy policy — plain English about what is stored on your computer, including the fact that the log records which shortcuts you pressed and which apps were opened. |
| **1.0.36** | **The dashboard can finally fill a big screen, and the app can now notice a dead overlay by itself.** (1) You said the keyboard looked small on your larger monitor and the space was wasted. Two separate limits were stopping it: the window could never exceed 1220x880, and the keyboard could never draw larger than its design size, no matter how much room there was. Both were ceilings, in every version this app has ever had. The window now takes 92% of your usable screen and the keyboard grows with it. Small laptops behave exactly as before. **Not done yet:** the settings and profile popovers still stay one fixed size — scaling them safely needs a look at a screenshot first, so it is deliberately left for next time rather than guessed at. (2) The check that detects "the overlay is not drawing" used to switch itself off permanently the moment the app moved to software rendering — which is why nothing noticed your HUD was dead for seven hours. It now keeps checking in both modes, and rebuilds the overlay if it finds it dead, up to three times before giving up and saying so. **NOT INSTALLED on 2026-08-17 — the permission prompt was declined, so this build exists but is not running.** |
| **1.0.35** | **Three things you reported, all confirmed as real bugs.** (1) The "Opacity floor" slider did nothing at all — it saved a number to your settings that no code ever read, while the actual floor stayed fixed at 25%. It now works. (Space + scroll wheel fades the window under the cursor; the floor is how transparent it may go before it stops, so a window can never be faded until you cannot find it.) (2) Deleting two profiles in a row: the first delete's countdown kept running and hid the Undo button when IT ran out, taking the second, still-valid undo off screen with it. The undo itself was never lost — only the button. There is now one countdown instead of one per delete. (3) Brave and Discord occasionally stopping responding: to bring a window to the front the app briefly links itself to that app's input, which is the normal way to beat Windows' focus lock, but linking to an app that is ALREADY stuck can drag both down. It now checks whether the app is responding first and skips the link if not. This is a likely cause, not a proven one — if it still happens, say so. |
| **1.0.34** | **Fixes two mistakes in 1.0.33's own repair, found within the hour on your machine.** 1.0.33 correctly noticed when you plugged your second display in, then failed to rebuild the overlay ("a webview with label `overlay` already exists") because it asked the old window to close and did not wait for it to actually go. Worse, on that failure it switched the HUD and toasts off entirely — so the repair did more damage than the fault. It now destroys the old window properly, waits until it is really gone, and if anything still fails it leaves the working overlay alone instead of disabling it. Also: if the dashboard is open on a display you unplug, it is now moved back onto a screen you still have, instead of being stranded until you reopen it from the tray. |
| **1.0.33** | **Two permanent fixes, and the transition switched back off.** (1) The Guide HUD and toasts used to stop appearing after the app had been running a long time — sound still played, apps still launched, nothing was drawn. Cause: the display arrangement changed underneath the app and the overlay window never recovered, while every internal check still reported it healthy. Measured: at the moment the app said the HUD was shown, that part of the screen contained **zero** HUD pixels. The app now watches the display setup and rebuilds the overlay when it changes, so no restart is ever needed. **This is also the whole of the "self-healing" seen since 08-13 — it never healed, it got restarted.** (2) Space+D stopped opening Discord, because Discord updates itself into a new `app-<version>` folder and the saved path died with the old one. Paths like that now repair themselves, which fixes Slack, Teams, GitHub Desktop and Signal too. (3) The 1.0.29–1.0.32 toast/HUD flight animation is switched off at your request, so the motion is 1.0.27's again. The code is kept behind one `WARP` flag for when you want to work on it. |

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
