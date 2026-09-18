# Phase A — actions, remappable specials, the Shortcuts page (started 2026-09-18)

Owner decisions, 2026-09-18 (Friday, Max plan, "keep going until done"):

- Order: **(a) actions + remappable specials + Shortcuts page** → (b) mouse
  buttons → (c) touchpad edge bands. Each its own release.
- Space stays the DEFAULT modifier; the modifier becomes a setting later
  (not in (a)). Mouse/touchpad triggers fire without Space (later phases).
- Brightness = Windows' own (WMI `WmiMonitorBrightnessMethods`), internal
  panel only, the same path the keyboard's brightness keys take. No
  Twinkle Tray, no DDC/CI.
- "Run command" is **Advanced only**. The keyboard dashboard stays the
  HOME screen; Shortcuts is a new page/tab, reached from a button.
- Touchpad (c): two-finger scroll inside an edge band, off by default —
  never one-finger edge slides (no driver = cannot stop the cursor moving).
- Keyboard detection: layout detection (GetKeyboardLayout / GetKeyNameText)
  + a "show me your keyboard" pass that records the scan codes the user
  actually presses; any recorded key is bindable. Windows cannot
  enumerate physical keys; Fn never reaches the OS.
- Lazy-senior rule: glue over engines. Windows already opens
  `ms-settings:` URIs, sends chords, runs commands; we catalogue them.
- Models: Fable/Opus for hook/engine/config (systems code), Sonnet for
  catalogue + markup, Opus leads and reviews.

## The model

```
Action = App { app, params, web_url, browser_exe, browser_profile_dir, site_icon }   // today's KeyBinding, unchanged
       | Uri { target }                       // ms-settings:… / shell:… / any URI, via ShellExecute (TargetShape::ProtocolUri / ShellVerb)
       | Chord { keys: Vec<u16> }             // virtual keys in press order; sent as ONE send_keys_checked batch (PROBLEM 227)
       | Command { line }                     // exe + args or a PowerShell line; Advanced only; shown on import
       | Brightness { delta: i32 }            // WMI, internal panel
       | Special { id }                       // boss_key | pip | pip_fullscreen | force_close | cycle_profile | search
                                              // | pause | voice_typing | screenshot | osk | scroll_top | scroll_bottom
```

- A `KeyBinding` gains `action: Option<Action>`; when `None`, the legacy
  app/web_url fields ARE the action (App). Nothing existing changes shape.
- **Triggers**: today the specials are matched by `KeyCombo` variant in
  `engine::run_combo`. After (a), a profile owns a `triggers` map
  `key-id → Action` where key-id is the dashboard's key id ("esc",
  "backtick", "tab", "backspace", "ralt", "comma", "period", "semicolon",
  "slash", "quote", "up", "down", any letter, F1–F12, …). The defaults
  are exactly today's table, seeded on migration, so nobody notices.
- The hook's per-key "does Space own this key" table (`BOUND_SPECIALS`
  and friends) is rebuilt on every config save from the trigger map —
  precomputed, never read from config in the callback (law: the LL hook
  is microseconds).
- Conflicts: one key, one action, per profile. The UI refuses a second.
- Migration: `config_version` bump; old specials → `triggers` with the
  default table; existing letter bindings untouched.

## Files (expected)

- `src-tauri/src/config/schema.rs` — Action, triggers, migration.
- `src-tauri/src/engine/mod.rs` — `run_combo` looks up the trigger map
  instead of matching fixed variants; `HUD_SPECIALS` / ring specials read
  the map (label + glyph from the special's id).
- `src-tauri/src/engine/actions/{chord.rs, brightness.rs, command.rs}` —
  new, each ~60 lines, same shape as `osk.rs`.
- `src-tauri/src/hook/mod.rs` — the key table fed from the map.
- `src/data/windows-catalogue.json` — the catalogue (Sonnet agent).
- `src/components/shortcuts-page.ts` + `styles/shortcuts.css` — the
  page (after the owner's Claude Design pass; transcribed literally).
- `src/components/key-detail-panel.ts` — action kinds in the key editor.

## What (a) does NOT do

Mouse buttons, touchpad, a changeable modifier key, global (no-Space)
hotkeys, external-monitor brightness.
