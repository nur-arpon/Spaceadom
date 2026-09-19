# Phase A brief 3 — 1.0.118: the prune, the rule, two fixes (2026-09-19)

Owner verdict after using 1.0.117 for an hour: the settings catalogue and
most toggles are "a shortcut app, not a utility"; he tried Screen off and
could not get the screen back (every Space+U woke the panel and turned it
off again — six times, 10:55:21–10:55:58 in debug.log); and the mouse ring
lost its specials. Read `docs/PHASE-A-BRIEF-1.md`, `-2.md`, and the code
they touched before starting. Rules: no installer, no install, no commit, no
input injection, never touch `%APPDATA%\Spaceadom\config.json`, never delete
or rename files you did not create, no elevation from the app's own process.

## 1. THE RULE (CLAUDE.md, near the top, its own heading)

> **Reversible, or it does not ship.** A bound action must be undoable by
> the same key or by an obvious next move, and must never leave the machine
> in a state the user has to recover from (screen off, sleep, muted mic,
> kept awake). Screen off shipped in 1.0.117 and the owner restarted his
> laptop to escape it. Any new action goes through this test in its brief.

## 2. Hide, do not delete — one code switch

`src/config/features.ts` (new) exporting `const FEATURES = { windowsCatalogue: false, hazardousToggles: false } as const;` and the Rust mirror `src-tauri/src/features.rs` with the same two `pub const bool`s (a test asserts the TS and Rust files agree — read the TS file with `include_str!` and check the literal). Everything below stays compiled, tested and reachable by flipping the constants; NOTHING is deleted.

With `windowsCatalogue: false`:
- The key editor's "Windows" tab shows ONLY the control rows (below). No
  "Open a settings page" heading, no catalogue rows, no `ms-settings:` /
  `shell:` items, no search box if fewer than 8 rows remain.
- The engine still executes `Action::Uri` from an existing config (a
  binding made in 1.0.116/117 keeps working) — only the UI hides them.

With `hazardousToggles: false`: `screen_off`, `sleep`, `bluetooth`,
`wifi`, `dark_mode`, `night_light` never appear in the editor. Existing
bindings to `screen_off` and `sleep` are **neutralised**, not just hidden:
`run_action` logs a warn + toast "Screen off / Sleep were removed in 1.0.118
— rebind this key" and does nothing. (Bluetooth/Wi‑Fi/dark/night still
execute if already bound; they are slow, not dangerous.)

## 3. What stays on the keyboard side — Advanced only, a few clicks deep

The "Windows" tab (rename to **"Controls"**) is shown ONLY in Advanced mode
and lists exactly: Lock, Taskbar auto-hide, Brightness up, Brightness down,
Volume up, Volume down, Mute. `show_desktop` stays in the catalogue JSON but
is not listed (it returns with the mouse-ring palette). Non-advanced users
see three kinds: **App or link · Send keys · Spaceadom special**.

## 4. Two new default specials: Space+← / Space+→ move the window

`SPECIAL_IDS` += `move_window_left`, `move_window_right`. Each sends the
chord Win+Shift+←/→ through `actions::chord::send` (one `send_keys_checked`
batch, PROBLEM 227) — Windows' own "move to the other monitor". Toast
"Window → other screen". Seeding: profiles already carry
`specials_seeded: true`, so add a second idempotent pass: `seed_specials`
now also fills `left`/`right` when BOTH are absent from `bindings` and the
profile has `specials_seeded` (log it once). A profile where the user
already bound `left` or `right` is left alone. Tests: fresh profile gets
14; a 1.0.117 profile gets the two added; a profile with `left` bound to an
app is untouched. HUD/ring: they appear wherever the other specials do.

## 5. Run command → PowerShell, PowerToys-Run parity

`Action::Command { line, elevated: bool }` (`#[serde(default)]` on
`elevated`). Execution: `powershell.exe -NoProfile -ExecutionPolicy Bypass
-Command <line>` with `CREATE_NO_WINDOW`, stdin null, killed after 60 s.
`elevated: true` → `ShellExecuteW(verb "runas", "powershell.exe", args)` —
Windows shows its own UAC prompt every time; the app's process never
elevates. The editor page: a multi-line box, a checkbox "Ask for
administrator rights (UAC prompt each time)", the full line rendered in a
monospace block under "This key will run:", and the hint "Runs as you, in
PowerShell, with no window. Paste any PowerShell one-liner. Advanced mode
only." Import (`ProfileExport`): when an imported profile carries any
`command` action, the import dialog lists every line before the user
confirms — never run on import. Test: serde default, script string, the
import listing.

## 6. Auto-repeat: fire once per press for everything but chords

Holding Space+U at 10:47:33 fired `Vol+` 30 times in 200 ms — fine for a
volume chord, wrong for a toggle/uri/command/special. In the hook's
callback the key-down carries the repeat bit (`LLKHF`… no: WH_KEYBOARD_LL
has no repeat flag — track it: a per-VK "down already seen since the last
up" bitmap, `AtomicU64 x4`, set on the first down, cleared on up; a down
with the bit already set is a repeat). Repeats reach the engine as
`HookEvent::KeyCombo` with `repeat: true`; `run_binding` drops repeats for
every action kind except `Chord`. Test the bitmap logic and the drop.

## 7. FIX: the mouse ring lost its specials

Evidence (debug.log 10:57:23): `hud-pointer: icon ring published — 13
tile(s), 13 code(s)` for the owner's profile "Arpon's Profile" — the 13 are
letters; the specials ring (ring 4, scope All) was empty, so aiming there
armed the nearest letter chip (`v`, then `c` → Brave). Before Phase A the
ring carried the static 8 `RING_SPECIALS`; now `ring_specials_for(cfg)`
derives them. Find why it returned nothing for a real, seeded profile —
suspects: it reads a profile other than `active_profile`; it filters on a
field the seeded bindings lack (`is_mapped()` false for `Special`
bindings?); the frontend drops tiles whose `icon` is `None`; the scope was
not `All`. Copy the LIVE config out via an explorer-launched cmd (the agent
shell sees a stale shadow — CLAUDE.md) to `scratchpad\live-config.json`,
write a test that loads it (redact nothing, do not commit it — keep the
test's fixture as a trimmed synthetic profile with the same shape) and
asserts ≥ 8 specials come back. Fix the cause. Document the cause in
PROJECT_STATUS as PROBLEM (next number).

## 8. Docs

- `FUTURE_IDEAS.md` §11 "Parked on 2026-09-19 with reasons": keyboard
  detection (exact legends via GetKeyboardLayout/GetKeyNameText; shape is
  unknowable to Windows — ISO/ANSI heuristic + a "show me your keyboard"
  press-through pass), mic mute everywhere (needs a persistent indicator +
  auto-unmute on exit), stay awake (same rule), see-through window (already
  exists as middle-hold + scroll), auto profile by context (owner: never —
  a wrong context ruins the whole experience), Windows settings catalogue +
  slow toggles (behind `FEATURES`), Show desktop (returns on the mouse ring
  palette). One line each with the reason.
- `PROJECT_STATUS.md` dated entry: the verdict, the screen-off incident and
  the rule, the ring bug, what 1.0.118 hides. `V14_FIXES_AND_CODE.md`
  §PHASE A — STEP 3. `all-versions/WHAT-CHANGED.md` row for 1.0.118 in the
  owner's voice (see the 1.0.117 row for tone).
- Version → **1.0.118** (package.json, tauri.conf.json, Cargo.toml,
  scripts/install-real.cmd).

## Gates

`cargo test --release --lib` (all green, new tests for §2 switch parity,
§4 seeding, §5 serde/import, §6 repeat, §7 ring), `cargo clippy --release
--lib` 0 warnings, `npx tsc --noEmit -p .`, `npm run build`. Report: files,
gate output, the §7 root cause in two sentences, any QUESTION one line each.
