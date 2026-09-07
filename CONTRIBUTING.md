# Contributing to Spaceadom

Spaceadom is [source-available, not open source](LICENSE) — read that file
before you start, and see the licensing note at the bottom of this document.
With that said, bug reports, small fixes, and well-scoped pull requests are
genuinely welcome.

## Before you write any code

Read, in this order:

1. [`CORE_AIM.md`](CORE_AIM.md) — the non-negotiable feature contract. A pull
   request that removes or simplifies a listed feature will be rejected, even
   if it "cleans things up." Fix a broken feature natively; don't cut it.
2. [`NATIVE_SAFETY.md`](NATIVE_SAFETY.md) — a do-not-touch table for Win32
   calls. This app once broke a user's touchpad by calling the wrong window
   API on the wrong window.
3. [`CLAUDE.md`](CLAUDE.md) — the full engineering guide: architecture, the
   hard-won window/hook rules, the build steps, and the testing laws. It's
   long because most of what's in it was learned by shipping a bug, not by
   reading documentation elsewhere — skipping it tends to re-discover the
   same bug.

## Build environment

```powershell
$env:CARGO_HOME="D:\RUST-DOWNLOADED-HERE\cargo"      # wherever your Rust lives
$env:RUSTUP_HOME="D:\RUST-DOWNLOADED-HERE\rustup"
$env:PATH="$env:CARGO_HOME\bin;$env:PATH"

npm install
npm run build        # tsc + vite — MUST run before any cargo command
npm run tauri dev    # run it
npm run tauri build  # installers -> src-tauri/target/release/bundle/
```

Requirements: [Rust](https://rustup.rs/) (edition 2021, currently built and
tested against 1.97.1), Node 20+, and the WebView2 runtime (already present
on Windows 11). Windows only — there is no cross-platform target.

Frontend is vanilla TypeScript + Vite. **No React, no Tailwind** — this is a
deliberate, standing decision, not an oversight; a PR that introduces either
will not be merged.

## The gates a change has to pass

Run these from `src-tauri` (Rust) and the repo root (frontend) before opening
a pull request. CI runs the same checks; this just means you find out sooner.

```powershell
cargo test --lib      # from src-tauri — currently several hundred tests
cargo clippy           # from src-tauri — keep it at 0 warnings
npm run build          # tsc + vite, from the repo root — keep it at 0 errors
```

A change that touches the keyboard hook, an overlay window, or anything
described in `CLAUDE.md`'s "Testing laws" needs to be **hand-tested on a real
Windows machine**, not just proven by an automated test. That file explains
why an automated harness cannot exercise this app's core loop end to end, and
what "verified" is required to mean here — read it before claiming something
works.

## Documenting a fix

If you fix something non-obvious — anything where the bug's cause wasn't
where the symptom appeared — this project's rule is that it gets written
down, in two places, every time:

1. **`PROJECT_STATUS.md`** — a dated, append-only dev log. Add an entry; never
   delete one, even a wrong one (a corrected mistake is worth more than a
   tidy history).
2. **`V14_FIXES_AND_CODE.md`** — the technical record, in the shape:
   **Symptom → Root cause → Exact file → The actual code → How it was
   verified.** The test for a good entry: could someone apply the fix from
   this file alone, without re-reading the surrounding code to find it?

This isn't bureaucracy for its own sake — the project's own experience is
that undocumented fixes get silently re-broken and re-diagnosed from
scratch, which costs far more than writing two paragraphs.

## Files you didn't create

**Never delete, rename, or overwrite a file you didn't create as part of your
own change** — including something that looks like a stale temp file, a
`.bak`, or an obvious leftover. If something looks like it shouldn't be
there, say so in your PR description and let the maintainer decide. This
project has been burned by exactly that judgment call before.

## Pull requests

- Keep them scoped to one change. A PR that fixes a bug and also reformats
  unrelated files is harder to review and more likely to be asked to split.
- Fill in the PR template, including how you tested it.
- By submitting a pull request, you agree that your contribution is licensed
  to the project under the terms in [LICENSE](LICENSE), so that official
  builds — the ones distributed from this project's GitHub Releases and the
  Microsoft Store — can include it. This project is not OSI open source; see
  LICENSE for exactly what that means and doesn't mean.

## Reporting bugs and requesting features

Use the [issue templates](.github/ISSUE_TEMPLATE/) — the bug report template
asks for exactly the information (version, installer, Windows build,
`debug.log`) that actually gets a bug fixed quickly here. For questions or
loose ideas, use [Discussions](https://github.com/nur-arpon/Spaceadom/discussions)
instead of an issue.

Security issues have their own process — see [SECURITY.md](SECURITY.md) and
do not open a public issue for one.
