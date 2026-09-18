# Phase A, step 1 — the engine: actions + assignable specials (brief for the implementing agent)

Read first: `docs/PHASE-A-ACTIONS.md` (the decisions), `CLAUDE.md` §Architecture, §Keyboard-hook laws, §Testing laws, §Hard rules. The code map below was produced 2026-09-18 and is accurate to commit `8d96532`.

## Design (fixed — do not redesign; ask by leaving a `QUESTION:` line in your report if something cannot work)

### 1. Config (`src-tauri/src/config/schema.rs`, mirror in `src/types.ts`)

```rust
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Uri        { target: String },          // "ms-settings:display", "shell:Downloads", "control.exe /name Microsoft.PowerOptions", any URI
    Chord      { keys: Vec<u16> },          // virtual-key codes in press order, e.g. [0x5B, 0x10, 0x53] = Win+Shift+S
    Command    { line: String },            // a command line; Advanced only (UI concern — the engine just runs it)
    Brightness { delta: i32 },              // +10 / -10, WMI on the internal panel
    Special    { id: String },              // one of SPECIAL_IDS below
}
pub const SPECIAL_IDS: &[&str] = &["boss_key","pip","pip_fullscreen","force_close","cycle_profile","search","pause","voice_typing","screenshot","osk","scroll_top","scroll_bottom"];
```

- `KeyBinding` gains `#[serde(default)] pub action: Option<Action>`. When `None`, the legacy `app`/`web_url` fields ARE the action (an app or link) — every existing config deserialises unchanged. Follow the `browser_exe` doc-comment pattern (PROBLEM 159 is why).
- `KeyBinding::is_mapped()` → `self.action.is_some() || self.app.is_some() || self.web_url.is_some()`.
- **Non-letter keys live in `Profile.bindings` too**, keyed by the dashboard's key ids (`keyboard-matrix.ts`: `esc`, `backtick`, `tab`, `backspace`, `ralt`, `comma`, `period`, `semicolon`, `slash`, `quote`, `up`, `down`, and any other id the board has). Letters stay exactly as they are.
- `Profile` gains `#[serde(default)] pub specials_seeded: bool`. On config load (`config/mod.rs` load path, once, idempotent), for every profile with `specials_seeded == false`: insert today's default specials into `bindings` for keys NOT already present, then set `specials_seeded = true`. The default table is exactly today's: esc→boss_key, backtick→pip, tab→pip_fullscreen, backspace→force_close, ralt→cycle_profile, comma→search, period→pause, semicolon→voice_typing, slash→screenshot, quote→osk, up→scroll_top, down→scroll_bottom. (Opacity on Space+scroll is a gesture, not a key; leave it as it is.) A user REMOVING a special = deleting that key from `bindings`; the seed never re-adds it because `specials_seeded` is true.
- `AppConfig.special_keys` (Enter / F1–F12 / Left / Right optional specials) stays as it is — do not migrate it, do not remove it. It keeps feeding the same hook table (see 2).
- `AppConfig` gains `#[serde(default)] pub advanced_mode: bool` (UI only; the engine ignores it).
- Update the golden-bytes test `a_fresh_config_serialises_to_identical_bytes_every_time` and add: an old config (no `action`, no `specials_seeded`) loads, gets seeded, and a profile that already had `esc` bound keeps its own binding; `Action` serde round-trip for every variant; `is_mapped` truth table.

### 2. Hook (`src-tauri/src/hook/mod.rs`) — ONE precomputed table, no config in the callback

- Replace the 12 fixed `KeyCombo` variants (`Escape, Backtick, Comma, RightAlt, UpArrow, DownArrow, Period, Semicolon, Slash, Quote, Backspace, Tab`) with **`KeyCombo::Vk(u16)`**. Keep `Alpha(char)` and `Special(String)` as they are (Special is the existing `special_keys` path for Enter/F-keys/Left/Right — leave it working).
- New `pub fn key_id_for_vk(vk: u16) -> Option<&'static str>` and `pub fn vk_for_key_id(id: &str) -> Option<u16>` — one static table, both directions, covering every non-letter key id `keyboard-matrix.ts` knows (read that file for the ids; include the number row, punctuation, F-keys, nav cluster, arrows, RAlt/RCtrl etc. where a VK exists). Letters are NOT in this table (they stay `Alpha`). Unit test: every id in the table round-trips, and every id present in `keyboard-matrix.ts`'s `KEYS`/layout that is not a letter is in the table (read the TS file in the test via `include_str!` and a simple regex, or list them explicitly and add a comment naming the source).
- New `BOUND_VKS: [AtomicU64; 4]` — a 256-bit bitmap "Space owns this VK": published by `publish_bound_vks(cfg)` from (a) the ACTIVE profile's `bindings` keys that map through `vk_for_key_id`, and (b) `special_keys` (keep `BOUND_SPECIALS`/`special_bit` working for the `Special(String)` path — do not break it; you may implement it on top of the bitmap if it simplifies things). Call `publish_bound_vks` from `config::save()` (where the other `publish_*` live, config/mod.rs ~363–386), from `lib.rs` boot (~1135–1149), and from `set_active_profile` (profile switch changes the table).
- The VK→combo match in the callback (~4556–4606): `Alpha` arm unchanged; `Special(...)` arms unchanged; every former fixed arm becomes `v if bound_vk(v) => Some(KeyCombo::Vk(v))`. **`VK_TAB` must go through the same table** (it is seeded, so nothing changes for users). No heap allocation in the callback (`Vk(u16)` is Copy).
- `own_window_combo_for_vk` (~5371): map through the bitmap too, but KEEP the exclusion the test pins (Escape/Enter/Tab/Backspace/arrows/RAlt are never injected from the page) — express the exclusion as a small const set, update the two tests to read from it, keep their intent.
- `middle_ring::special_combo_for(c)` → returns `KeyCombo::Vk(vk_for_key_id(id))`; see 4.

### 3. Engine (`src-tauri/src/engine/mod.rs` + `engine/actions/`)

- `run_combo`: `Alpha(c)` → `run_binding(key_id = c.to_string())`; `Vk(vk)` → `run_binding(key_id_for_vk(vk))`; `Special(name)` → unchanged `handle_special`.
- New `run_binding(key_id, state_arc)`: active profile → `bindings.get(key_id)`; if absent → log + return; match `binding.action`:
  - `None` → today's `smart_cascade` path exactly as `handle_alpha` does now (keep `handle_alpha`'s body, just reached through here).
  - `Some(Uri{target})` → `actions::uri::open(target, app_handle)` — for `ms-settings:`/`shell:`/`*:` use the existing `shell_launch`/protocol path in smart_cascade (expose a small pub fn there rather than duplicating); for a `control.exe … `/exe-with-args line treat it as a command. Toast: the binding's `label` or the target.
  - `Some(Chord{keys})` → `actions::chord::send(&keys)`: build the INPUT array (down in order, up in reverse) with `dwExtraInfo 0x7A7A7A7A` and call `hook::send_keys_checked` ONCE (PROBLEM 227). Mirror `osk.rs`'s shape: `toast_text()` pure + tested, `send` never called from tests.
  - `Some(Command{line})` → `actions::command::run(line)`: `std::process::Command` via `cmd.exe /C <line>` with `CREATE_NO_WINDOW`, detached, never waited on; log the line; toast "Ran: …". No elevation ever.
  - `Some(Brightness{delta})` → `actions::brightness::adjust(delta)`: spawn hidden `powershell -NoProfile -NonInteractive -Command` with `Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness` → current, then `WmiMonitorBrightnessMethods … WmiSetBrightness(0, clamp(cur+delta))`. Toast "Brightness NN%". If no internal panel (the class is empty) toast "No built-in display to adjust".
  - `Some(Special{id})` → the existing handler for that id (`handle_boss_key`, `handle_pip`, `handle_fullscreen_pip`, `handle_force_close`, `handle_profile_cycle`, `handle_focus`, `handle_bypass_toggle`, `handle_voice_typing`, `handle_screenshot`, `handle_osk`, and the scroll-top/bottom double-tap handlers). The double-tap detection for scroll_top/scroll_bottom must keep working when those specials are on `up`/`down` — and also when the user moves them to other keys (the double-tap state keys on the key id, not on the VK).
- `HUD_SPECIALS` (static 8-list) → `pub fn hud_specials_for(cfg) -> Vec<(String, String)>`: the active profile's NON-letter bindings that `is_mapped()`, as (key label, name) — key label from a small `key_label(id)` table ("Esc", "`", "Tab", "⌫", "RAlt", ",", ".", ";", "/", "'", "↑", "↓", …), name = `binding.label` if set, else the special's display name (`boss_key`→"Boss Key (Hide All + Mute)", … keep today's strings), else for Uri/Chord/Command a short derived name. Keep the two gesture rows ("Scroll" → "Layer Opacity", and the Up/Dn ×2 row ONLY when both scroll specials are bound) so the ring reads as today by default. `specials_for_hud` keeps its on/off/band-count truth table but over this list. Rewrite the `band_gate_tests` against a fixture config that yields today's 8 rows (so the assertions stay meaningful) plus one test with a special removed.
- `middle_ring::RING_SPECIALS` → `ring_specials_for(cfg)`: same source; the PUA code for a key id is `'\u{E000}' + index in `key_id_for_vk`'s table` (stable, collision-free); `special_combo_for(code)` inverts that. Update the three middle_ring tests to fixtures.
- `hud_icons_for` / `hud_apps_for` / `bound_letters` / favourites: letters only, as today — but a LETTER whose binding has `action: Some(_)` is still "bound" (it shows its label with a generic glyph: Uri → "⚙", Chord → "⌨", Command → ">_", Special → the special's glyph). `middle_ring::bound_letters` therefore keeps working unchanged (it filters on `is_mapped`).
- Chord recorder for the UI: `hook::RECORDING: AtomicBool` + a `Mutex<Vec<u16>>` ring buffer written from the ENGINE thread (the hook forwards a `HookEvent::RawKey(vk, down)` only while RECORDING, keys pass through untouched, Space included). Commands: `chord_record_start()`, `chord_record_poll() -> Vec<u16>` (the keys currently held, in press order — the chord is what was held when the LAST key went down), `chord_record_stop()`. Keep it simple and safe: recording auto-stops after 15 s.

### 4. Frontend (vanilla TS — NO React, NO Tailwind)

- `src/types.ts`: `Action` union mirroring the enum (`{ kind: "uri"; target: string } | { kind: "chord"; keys: number[] } | …`), `KeyBinding.action?: Action`, `Profile.specials_seeded?: boolean`, `AppConfig.advanced_mode?: boolean`.
- `keyboard-matrix.ts`: every key with an id in the VK table becomes `.bindable` with `attachKeyListeners`, exactly like letters; `SPECIAL_ON_KEY` static labels go away — the label under a non-letter key is derived from its binding (special display name short form: "Boss", "PiP", "Snip", "Dictate", "Keys", … keep today's short words), empty when unbound; `updateMatrix()` handles non-letter keys.
- `key-detail-panel.ts`: an **action kind** segmented control at the top of the editor: *App or link* (today's UI, unchanged) / *Windows setting* (search box over `src/data/windows-catalogue.json`, results as rows "name — path", pick one → `Uri`/`Chord`/`Brightness` from the item's kind; hide items with `"unsure": true`) / *Send keys* (a pill "Press the keys…" that calls `chord_record_start`, polls every 100 ms, shows kbd caps for the held keys, a Done button stops) / *Run command* (one input + "Try it" that invokes a `run_command_once(line)` command; shown ONLY when `appConfig.advanced_mode`) / *Spaceadom special* (a list of the 12 with their description from `special-cards.ts`). Save produces a complete `KeyBinding` through the same `onSave` contract (full replace, re-null the browser fields as main.ts warns). Refuse assigning a `Special{id}` already used by another key in this profile — show "Boss key is on Esc — move it?" with a Move button that clears the other key.
- `special-cards.ts`: `combo` and `how` derived from the active profile's binding for that special (find the key whose action is `Special{id}`); a special bound nowhere reads "Not on any key — assign it from any key's editor".
- `settings-panel.ts`: one new switch "Advanced mode" (desc: "Shows Run command and the full catalogue in the key editor.") persisted to `advanced_mode`; use the existing `toggleRow` pattern.
- `preview.ts` stub: give it a fixture profile with the 12 seeded specials so the preview still renders.
- No Shortcuts page in this step (it waits for the owner's design).

### 5. Proof, docs, version

- `cd src-tauri && cargo test --release` 0 failures (doctest failures in `display_watch.rs`/`pip.rs` etc. are pre-existing prose blocks — ignore those specifically, do not "fix" them); `npx tsc --noEmit -p .` clean; `npm run build` clean; `cargo clippy` no new warnings.
- Bump to **1.0.116** in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `scripts/install-real.cmd`'s setup path. Do NOT build the installer, do NOT install, do NOT commit — the lead does the build + install proof + commit.
- Write `V14_FIXES_AND_CODE.md` §"PHASE A — ACTIONS AND ASSIGNABLE SPECIALS" (symptom/decision → design → exact files → key code → how verified), and a PROJECT_STATUS.md entry at the top (after the recovery notice block, before the newest entry) signed "Claude (Phase A implementing agent, Fable)". Update `CLAUDE.md`'s Architecture paragraph that lists the fixed specials (search "Space + ; and the ring's") to describe the trigger map in ≤6 lines. Keep the owner's voice out of it; plain engineering prose.
- Report: what changed (file list), test counts before/after, anything left as `QUESTION:`.

## Hard rules for this task
- Never read config inside the hook callback; every table is an atomic published on save/boot/profile switch.
- `send_keys_checked` is engine-thread only; one batch per chord.
- Do not delete or rename files you did not create. Do not touch `to-publish-in-microsoft-store/`, `.github/`, `docs/` other than the two docs named above.
- Keep every existing test's INTENT; rewrite assertions against fixtures rather than deleting tests.
