# Security Policy

## Scope

Spaceadom runs a system-wide low-level keyboard hook and installs a
self-updater that can silently replace its own binary. Those two things are
where a real vulnerability would matter most:

- **The keyboard hook** (`src-tauri/src/hook/`) — anything that would let
  another process read, inject, or interfere with keystrokes beyond what the
  documented feature set does; anything that would let the hook see or leak
  what you type (it is designed to keep no history at all — see PRIVACY.md).
- **The updater and its signing** (`src-tauri/src/updater.rs`) — anything
  that would let an attacker get Spaceadom to install something that isn't a
  genuine, signed release. The signing key itself is never in this
  repository; see CLAUDE.md for how it's handled in CI.
- **The conflict-closer** (`src-tauri/src/hook/conflict_close.rs`) — the one
  feature that can end another process or touch another program's
  autostart entries. It's built to act only on a fixed known-process list,
  only on explicit user confirmation, and never with silent elevation; a way
  around any of those constraints is a security bug.

General bugs, crashes, and UI issues are not security reports — please use
the normal [issue tracker](https://github.com/nur-arpon/Spaceadom/issues) for
those instead.

## Reporting a vulnerability

**Please do not open a public issue for a security problem.** Report it
privately through GitHub's own mechanism:

1. Go to the [Security tab](https://github.com/nur-arpon/Spaceadom/security)
   of this repository.
2. Click **"Report a vulnerability"** to open a private security advisory.

This reaches the maintainer directly and keeps the report private until a fix
is ready, rather than disclosing it (and any exploit details) to everyone at
once.

Include what you'd include in any good bug report: the version affected, the
installer used, Windows version, and clear reproduction steps. A minimal
proof of concept is welcome and speeds things up.

## What to expect

This is a one-person project, not a company with an SLA. There's no bug
bounty. What you can expect: an acknowledgement, an honest assessment of
whether it's in scope and how serious it is, and credit in the eventual fix's
release notes if you'd like it (or anonymity, if you'd prefer that instead —
say which when you report).
