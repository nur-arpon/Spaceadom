/**
 * js-error-reporter.ts — the two global JavaScript error handlers, for BOTH
 * webviews (PROBLEM 217).
 *
 * WHY THIS EXISTS. A shipped WebView2 has no console. Until now the only bridge
 * out of a webview was `frontend_log` (INFO) and `overlay_log` (WARN), and the
 * crash reporter's floor is ERROR — so every JavaScript failure in the dashboard
 * or the overlay, which is the whole UI layer, stayed on the user's machine. A
 * wedged frontend or a dead HUD is exactly what a friend reports as "it looks
 * broken", and it was the one class of failure that could never be seen.
 *
 * A LEAF MODULE, on purpose: it imports nothing but Tauri's `invoke`, so the
 * overlay can use it without dragging the dashboard in (PROBLEM 148).
 *
 * THE THREE RULES THIS FILE EXISTS TO OBEY. Telemetry must never become a crash
 * source, so:
 *
 *   1. IT NEVER THROWS. Every line of the handler is inside a `try`, and the
 *      `invoke` is `.catch()`ed. A reporter that throws inside `window.onerror`
 *      turns one bug into an unrecoverable page.
 *   2. IT NEVER RECURSES. `reporting` is held for the whole synchronous body,
 *      so an error raised *inside* the handler cannot re-enter it. Without this
 *      a single fault in the reporter is an infinite loop that ends as a hung
 *      webview.
 *   3. IT NEVER SPAMS. A page that errors every frame must produce a handful of
 *      reports and then go quiet — see the three limits below. Rust rate-limits
 *      again on its side; this half exists so the invoke traffic and the log
 *      lines are bounded too, not only what reaches the reporter.
 */

import { invoke } from "@tauri-apps/api/core";

/** The same message+location at most this often. A render loop that throws on
 *  every frame is one report a minute, not sixty a second. */
const COOLDOWN_MS = 60_000;

/** Distinct signatures tracked. Bounds memory when the messages themselves
 *  vary (a loop that throws with a changing index, say). */
const MAX_SIGNATURES = 24;

/** Reports per page load, whatever the mix of signatures. After this the page
 *  says so once and then stays silent for the rest of its life. */
const MAX_REPORTS = 20;

/** A stack is the diagnosis; the first frames are all of it. */
const MAX_STACK = 1_500;

/** Re-entrancy guard — rule 2 above. */
let reporting = false;
let reports = 0;
let silenced = false;
const lastSeen = new Map<string, number>();

/** Which Rust command this webview reports through. Both take `{ msg }` and
 *  both log at ERROR; the prefix is applied on the Rust side so the existing
 *  `dashboard-js:` / `overlay-js:` log convention cannot drift. */
export type ErrorSink = "frontend_error" | "overlay_error";

/**
 * PII. A rejected `invoke` rejects with the Rust command's error STRING, and
 * several of those quote something the user named: `Profile 'Work' not found`,
 * `Profile 'Work' already exists`, `Unknown compositing mode 'x'`. A profile
 * name is user-derived and must not leave the machine, so every quoted run in a
 * non-`Error` rejection reason is redacted here.
 *
 * Deliberately NOT applied to real `Error` objects: `Cannot read properties of
 * undefined (reading 'offsetWidth')` quotes a PROPERTY name, which is the whole
 * diagnosis and contains nothing about the user. Nothing in `src/` throws with
 * config-derived text (there is not one `throw new Error` in the tree), so an
 * Error's message is always the engine's or the browser's own words.
 *
 * Rust scrubs paths and non-local URLs again on its side; this is the half only
 * the frontend has the type information to do.
 */
function redactQuoted(s: string): string {
  return s.replace(/'[^']{0,200}'/g, "'<redacted>'").replace(/"[^"]{0,200}"/g, '"<redacted>"');
}

function clip(value: unknown, max: number): string {
  let s: string;
  try {
    s = typeof value === "string" ? value : String(value);
  } catch {
    // A thrown object whose toString() itself throws. It happens.
    s = "<unstringifiable>";
  }
  return s.length > max ? `${s.slice(0, max)}…` : s;
}

/**
 * Send one report, subject to the three limits. Returns nothing and throws
 * nothing, ever.
 */
function report(sink: ErrorSink, msg: string, signature: string): void {
  if (silenced) return;
  const now = Date.now();

  const seen = lastSeen.get(signature);
  if (seen !== undefined && now - seen < COOLDOWN_MS) return;
  if (seen === undefined && lastSeen.size >= MAX_SIGNATURES) return;
  lastSeen.set(signature, now);

  reports += 1;
  if (reports > MAX_REPORTS) {
    silenced = true;
    void invoke(sink, {
      msg: `too many JS errors this session (${MAX_REPORTS}) — further reports suppressed`,
    }).catch(() => {});
    return;
  }

  void invoke(sink, { msg }).catch(() => {});
}

/**
 * Wire `error` and `unhandledrejection` for THIS webview.
 *
 * `addEventListener("error")` rather than assigning `window.onerror`: it is
 * what this project already used, it carries the same message/filename/line/
 * column, and it does not stomp on any handler a library may have installed.
 *
 * Call once, as early as possible — an exception during bootstrap is exactly
 * the one worth having.
 */
export function installJsErrorReporter(sink: ErrorSink): void {
  window.addEventListener("error", (e: ErrorEvent) => {
    if (reporting) return; // rule 2: an error inside the handler stops here
    reporting = true;
    try {
      // Everything that makes a JS error diagnosable: what, where, and how it
      // got there. `e.error?.stack` is absent for cross-origin scripts and for
      // some synthetic events, hence the fallback to the filename triple.
      const where = `${clip(e.filename, 300)}:${e.lineno}:${e.colno}`;
      const stack = e.error && typeof e.error.stack === "string"
        ? `\n${clip(e.error.stack, MAX_STACK)}`
        : "";
      report(sink, `error: ${clip(e.message, 500)} @ ${where}${stack}`, `error|${clip(e.message, 200)}|${where}`);
    } catch {
      // Deliberately empty — rule 1. There is nowhere left to report to.
    } finally {
      reporting = false;
    }
  });

  window.addEventListener("unhandledrejection", (e: PromiseRejectionEvent) => {
    if (reporting) return;
    reporting = true;
    try {
      const reason: unknown = e.reason;
      // A rejected Promise carries no filename/line of its own; the stack on
      // the rejection value is the only location information there is.
      const isError = reason instanceof Error;
      const stack = isError && typeof reason.stack === "string"
        ? `\n${clip(reason.stack, MAX_STACK)}`
        : "";
      // See redactQuoted: anything that is not a real Error is most likely a
      // Rust command's rejection string, and those quote user-named things.
      const text = isError ? clip(reason.message, 500) : redactQuoted(clip(reason, 500));
      report(sink, `unhandled rejection: ${text}${stack}`, `rejection|${clip(text, 200)}`);
    } catch {
      // Rule 1.
    } finally {
      reporting = false;
    }
  });
}
