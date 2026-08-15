# Rust Crate Catalog

Versions are `max_stable_version` from crates.io, verified **2026-08-10**. Re-check with `cargo search <crate>` or `cargo add <crate>` (which resolves the latest itself).

**Prefer `cargo add`** over hand-editing `Cargo.toml` — it writes the current version and correct feature syntax.

---

## Core

| Crate | Version | Notes |
|---|---|---|
| `tauri` | 2.11.5 | Framework. |
| `tauri-build` | 2.6.3 | Build script dependency. |
| `serde` | 1.0.229 | Needs `features = ["derive"]`. |
| `serde_json` | 1.0.151 | |
| `tokio` | 1.53.1 | Async runtime. Tauri already pulls it in — match features rather than adding a second runtime. |
| `anyhow` | 1.0.104 | Error handling in application code. |
| `thiserror` | 2.0.20 | Typed errors for library boundaries and IPC. Note v2 — v1 is a different API. |

**Error handling split:** `thiserror` for anything crossing into a Tauri command (the frontend needs a stable, serializable error shape), `anyhow` for internal plumbing where you only need context.

```rust
#[derive(Debug, thiserror::Error, serde::Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    #[error("shortcut {0} is already bound")]
    ShortcutTaken(String),
    #[error("target application not found: {0}")]
    TargetMissing(String),
}
```

---

## Tauri plugins

| Crate | Version | JS package | Use for |
|---|---|---|---|
| `tauri-plugin-window-state` | 2.4.1 | `@tauri-apps/plugin-window-state` | Remembers window size and position. Add this to every app. |
| `tauri-plugin-store` | 2.4.4 | `@tauri-apps/plugin-store` | Persistent key-value JSON on disk. Correct home for user settings. |
| `tauri-plugin-single-instance` | 2.4.3 | — | Focus the running window instead of launching a second copy. |
| `tauri-plugin-autostart` | 2.5.1 | `@tauri-apps/plugin-autostart` | Launch at login. |
| `tauri-plugin-global-shortcut` | 2.3.2 | `@tauri-apps/plugin-global-shortcut` | System-wide hotkeys. |
| `tauri-plugin-notification` | 2.3.3 | `@tauri-apps/plugin-notification` | Native toast notifications. |
| `tauri-plugin-dialog` | 2.7.2 | `@tauri-apps/plugin-dialog` | Native file and message dialogs. |
| `tauri-plugin-fs` | 2.5.1 | `@tauri-apps/plugin-fs` | Scoped filesystem access. |
| `tauri-plugin-shell` | 2.3.5 | `@tauri-apps/plugin-shell` | Spawn processes. |
| `tauri-plugin-opener` | 2.5.4 | `@tauri-apps/plugin-opener` | Open URLs and files in the default handler. Replaces the old shell-open API. |
| `tauri-plugin-updater` | 2.10.1 | `@tauri-apps/plugin-updater` | Signed auto-update. |
| `tauri-plugin-log` | 2.9.0 | `@tauri-apps/plugin-log` | Logging to file, stdout, and the webview console. |
| `tauri-plugin-sql` | 2.4.0 | `@tauri-apps/plugin-sql` | SQLite/Postgres/MySQL from the frontend. |
| `tauri-plugin-deep-link` | 2.4.9 | `@tauri-apps/plugin-deep-link` | Custom URL scheme handling. |

Plugins need the Rust crate *and* usually the JS package, plus a capability entry in `src-tauri/capabilities/`. Missing the capability is the most common "the plugin does nothing" cause — the call fails with a permission error that is easy to miss.

**Prefer `tauri-plugin-log` over `log4rs`.** It integrates with Tauri's lifecycle, forwards frontend `console` output into the same file, and handles rotation, at a fraction of the configuration.

---

## Windows / Win32

| Crate | Version | Notes |
|---|---|---|
| `windows` | 0.62.2 | Official Microsoft bindings. Enable only the feature modules you use — each adds compile time and binary size. |
| `windows-sys` | (tracks `windows`) | Raw FFI, no wrappers. Smaller and faster to compile if you do not need the ergonomics. |
| `winreg` | 0.56.0 | Registry access. |
| `window-vibrancy` | 0.8.0 | Mica, acrylic, and blur on Tauri windows. |
| `global-hotkey` | 0.8.0 | Standalone hotkey registration (what the Tauri plugin wraps). |
| `tray-icon` | 0.24.2 | Standalone tray icon. |
| `muda` | 0.19.3 | Native menus. |
| `raw-window-handle` | 0.6.2 | Get an `HWND` from a Tauri window for Win32 calls. |

The `windows` crate moves fast and is not semver-stable across minor versions — 0.58 to 0.62 has breaking changes. Pin it and upgrade deliberately.

```rust
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;

let handle = window.window_handle()?;
let hwnd = match handle.as_raw() {
    RawWindowHandle::Win32(h) => HWND(h.hwnd.get() as *mut _),
    _ => return Err(AppError::NotWindows),
};
```

---

## Data & storage

| Crate | Version | Notes |
|---|---|---|
| `rusqlite` | 0.40.2 | Synchronous SQLite. Use `features = ["bundled"]` so users need no system SQLite. |
| `sqlx` | 0.9.0 | Async, compile-time-checked queries. Heavier; worth it for a real schema. |
| `serde_json` | 1.0.151 | |
| `toml` | — | Human-editable config files. |
| `directories` | — | Correct per-OS config, data, and cache paths. Do not build these paths by hand. |

For an app with settings and a handful of tables, `tauri-plugin-store` plus `rusqlite` bundled is the light path. Reach for `sqlx` when migrations and query checking start paying for themselves.

---

## Concurrency

| Crate | Version | Notes |
|---|---|---|
| `crossbeam-channel` | 0.5.16 | Fast MPMC channels. Right choice for OS-thread to OS-thread (a Win32 hook thread talking to the app). |
| `parking_lot` | 0.12.5 | Faster `Mutex`/`RwLock`, no poisoning. |
| `dashmap` | 6.2.1 | Concurrent hashmap without a global lock. |
| `rayon` | 1.12.0 | Data parallelism for CPU-bound loops. |
| `once_cell` | 1.21.4 | Lazy statics. Much of this is now in std as `OnceLock`/`LazyLock` — prefer std for new code. |

**Channel choice:** `tokio::sync::mpsc` when both ends are async tasks; `crossbeam-channel` when a real OS thread is involved. A low-level keyboard hook runs on its own thread with a message pump and must not block, so it should push into a crossbeam channel and return immediately.

---

## Type-safe IPC

| Crate | Version | Notes |
|---|---|---|
| `specta` | 1.0.5 | Derives TypeScript types from Rust types. |
| `tauri-specta` | 1.0.2 | Generates a typed client for your Tauri commands. |

This removes an entire class of bug. Commands, arguments, and return types become TypeScript automatically, so a renamed Rust field breaks the build instead of producing `undefined` at runtime. Worth adding early; retrofitting is tedious.

---

## Observability

| Crate | Version | Notes |
|---|---|---|
| `tracing` | 0.1.44 | Structured, span-based logging. |
| `tracing-subscriber` | 0.3.23 | Output configuration. |
| `log` | 0.4.x | Simple facade. Fine for small apps. |

`tracing` spans let you measure how long a command actually took, which is how you find that a "slow UI" is really a 400 ms blocking registry read.

---

## Networking

| Crate | Version | Notes |
|---|---|---|
| `reqwest` | 0.13.4 | HTTP client. Note the 0.13 jump from the long-lived 0.12 line. |
| `tokio-tungstenite` | — | WebSockets. |

Do network work in Rust rather than the webview: no CSP exceptions, no CORS, credentials stay out of JS, and the request survives a webview reload.

---

## Utilities

| Crate | Version |
|---|---|
| `chrono` | 0.4.45 |
| `uuid` | 1.24.0 |
| `base64` | 0.23.1 |
| `regex` | — |
| `itertools` | — |

`std::time` covers monotonic timing; `chrono` is for calendar dates and formatting.

---

## Build profile

Cuts binary size substantially over the default release profile:

```toml
[profile.release]
codegen-units = 1
lto = true
opt-level = "s"     # or "z" for smallest; "3" if hot loops matter more than size
panic = "abort"
strip = true
```

`panic = "abort"` removes unwinding machinery. Do not use it if any code relies on `catch_unwind` — some Win32 callback wrappers do.

For a keyboard hook or other latency-sensitive hot path, keep `opt-level = 3` and take the larger binary.

---

## Version drift check

If a `Cargo.toml` uses bare major-version constraints (`tauri = "2"`, `windows = "0.58"`), `cargo update` will not cross a breaking boundary and the project can sit on old code indefinitely. Check with:

```bash
cargo install cargo-outdated   # once
cargo outdated --root-deps-only
```

For the `windows` crate specifically, `"0.58"` will never resolve to 0.62 — that upgrade is a deliberate, breaking edit.
