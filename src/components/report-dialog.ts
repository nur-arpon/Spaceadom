/**
 * report-dialog.ts — PROBLEM 253. "Report a problem".
 *
 * One small modal: a box to say what went wrong, a button that builds a zip,
 * and then the path to that zip with a link to the issue tracker.
 *
 * WHAT IT DOES NOT DO, and must never do: upload anything. Rust writes the
 * archive to `%APPDATA%\Spaceadom\reports\` and opens Explorer with it
 * selected; the user decides whether it leaves the machine. Every sentence in
 * here is written to make that obvious rather than to reassure — a dialog that
 * says "sending…" about a file that is not being sent is worse than no dialog.
 *
 * A LEAF MODULE (CLAUDE.md, PROBLEM 148): it imports nothing from `main.ts`, so
 * `preview.ts` can render it and the settings panel can import it. That is the
 * whole reason it is its own file — the safe-mode banner in `main.ts` and the
 * About section's "Report a problem" link are owned by two different places and
 * both need this one dialog. `openReportDialog()` is the entire public surface.
 *
 * Its CSS is injected from here rather than living in `styles.css`, for the
 * same reason: a component another file imports should not need a second edit
 * somewhere else to be visible. Theme handling is free — every colour is a
 * design-system token, and `body.nocturne` redefines the tokens.
 */

import { invoke } from "@tauri-apps/api/core";

/** The one dialog. Reopening while it is up focuses the existing one. */
let host: HTMLDivElement | null = null;

const STYLE_ID = "st-report-dialog-styles";

const CSS = `
#st-report-scrim {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(32, 22, 12, .38);
  animation: st-report-fade 180ms var(--ease-out) both;
}
#st-report-card {
  width: min(560px, calc(100vw - 64px));
  max-height: calc(100vh - 96px);
  overflow: auto;
  box-sizing: border-box;
  padding: 20px 22px 18px;
  border-radius: var(--radius-lg, 16px);
  background: var(--st-card, #fdf6e9);
  border: 1px solid var(--st-border, #d8c9ab);
  color: var(--st-text, #201e1d);
  box-shadow: var(--shadow-lg, 0 18px 50px rgba(90, 60, 30, .2));
  font-family: var(--st-font-body, system-ui, sans-serif);
  /* 420ms spring in; exits run at ~65% with --ease-in (CLAUDE.md design rules)
     and are handled by removing the node, which needs no keyframe. */
  animation: st-report-pop 420ms var(--ease-spring, cubic-bezier(.34,1.3,.4,1)) both;
}
#st-report-card h2 {
  margin: 0 0 6px;
  font-family: var(--st-font-heading, Georgia, serif);
  font-size: 19px;
  font-weight: 400;
}
#st-report-card p { margin: 0 0 12px; font-size: 12.5px; line-height: 1.45; color: var(--st-text-soft, #5c554d); }
#st-report-card textarea {
  width: 100%;
  box-sizing: border-box;
  min-height: 104px;
  resize: vertical;
  padding: 10px 12px;
  border-radius: var(--radius-md, 13px);
  border: 1px solid var(--st-border, #d8c9ab);
  background: var(--st-surface, #faf1de);
  color: inherit;
  font: inherit;
  font-size: 13px;
}
#st-report-card textarea:focus { outline: 2px solid var(--st-accent-brd, #e0ac80); outline-offset: 1px; }
.st-report-row { display: flex; gap: 8px; justify-content: flex-end; margin-top: 14px; flex-wrap: wrap; }
.st-report-path {
  display: block;
  margin: 10px 0 0;
  padding: 9px 11px;
  border-radius: var(--radius-sm, 9px);
  background: var(--st-surface, #faf1de);
  border: 1px solid var(--st-hairline, #e8dcc2);
  font-size: 11.5px;
  font-family: ui-monospace, Consolas, monospace;
  word-break: break-all;
  color: var(--st-text-soft, #5c554d);
}
.st-report-note { font-size: 11.5px; color: var(--st-text-dim, #736a5f); margin-top: 10px; }
@keyframes st-report-fade { from { opacity: 0 } to { opacity: 1 } }
@keyframes st-report-pop {
  from { opacity: 0; transform: translateY(10px) scale(.97) }
  to   { opacity: 1; transform: none }
}
@media (prefers-reduced-motion: reduce) {
  /* CLAUDE.md: reduced motion renders FINAL states, it does not just shorten. */
  #st-report-scrim, #st-report-card { animation: none }
}
`;

function ensureStyles(): void {
  if (document.getElementById(STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = CSS;
  document.head.appendChild(style);
}

function close(): void {
  host?.remove();
  host = null;
  document.removeEventListener("keydown", onKeydown, true);
}

function onKeydown(e: KeyboardEvent): void {
  if (e.key === "Escape") {
    e.stopPropagation();
    close();
  }
}

/**
 * Open the report dialog. Idempotent: a second call while it is open just
 * focuses the box instead of stacking two scrims.
 *
 * Exported for `main.ts`'s safe-mode banner and for the settings panel's About
 * section, which are owned separately and must not each grow their own copy.
 */
export function openReportDialog(): void {
  if (host) {
    host.querySelector<HTMLTextAreaElement>("#st-report-text")?.focus();
    return;
  }
  ensureStyles();

  host = document.createElement("div");
  host.id = "st-report-scrim";
  // Clicking the scrim closes; clicking the card must not. Checked on
  // `e.target` rather than by stopping propagation inside the card, so a future
  // control in the card cannot silently break dismissal.
  host.addEventListener("click", (e) => {
    if (e.target === host) close();
  });

  const card = document.createElement("div");
  card.id = "st-report-card";
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-modal", "true");
  card.setAttribute("aria-label", "Report a problem");

  const title = document.createElement("h2");
  title.textContent = "Report a problem";

  const blurb = document.createElement("p");
  blurb.textContent =
    "Describe what went wrong. Spaceadom will save a report file on this " +
    "computer — the recent log, your settings with personal details removed, " +
    "and what kind of Windows you are on. Nothing is sent anywhere; you " +
    "choose whether to attach it to a GitHub issue.";

  const box = document.createElement("textarea");
  box.id = "st-report-text";
  box.placeholder =
    "For example: the app closed by itself a few seconds after I signed in, three times in a row.";

  const row = document.createElement("div");
  row.className = "st-report-row";

  const cancel = document.createElement("button");
  cancel.className = "btn btn-sm";
  cancel.textContent = "Cancel";
  cancel.addEventListener("click", close);

  const create = document.createElement("button");
  create.className = "btn btn-sm";
  create.textContent = "Create the report";
  create.addEventListener("click", () => {
    void buildReport(card, create, box.value);
  });

  row.append(cancel, create);
  card.append(title, blurb, box, row);
  host.appendChild(card);
  document.body.appendChild(host);
  // Capture phase: the dashboard has its own document-level Escape handler for
  // popovers and the key-detail panel, and a modal has to win.
  document.addEventListener("keydown", onKeydown, true);
  box.focus();
}

/**
 * Ask Rust for the bundle and rewrite the card with the result.
 *
 * The button is disabled and relabelled for the duration, because building the
 * zip reads two logs and deflates a few megabytes — it takes a visible moment,
 * and a button that looks idle while it works gets pressed twice.
 */
async function buildReport(
  card: HTMLElement,
  button: HTMLButtonElement,
  description: string,
): Promise<void> {
  button.disabled = true;
  const original = button.textContent ?? "Create the report";
  button.textContent = "Saving…";

  let path = "";
  let failure = "";
  try {
    path = await invoke<string>("build_diagnostics_bundle", { description });
  } catch (err) {
    failure = String(err);
  }

  if (failure || !path) {
    button.disabled = false;
    button.textContent = original;
    let note = card.querySelector<HTMLElement>(".st-report-note");
    if (!note) {
      note = document.createElement("p");
      note.className = "st-report-note";
      card.appendChild(note);
    }
    // The real reason, not "something went wrong": the person reading it is
    // already trying to report a fault, and a second unexplained fault on top
    // of the first is the end of the report.
    note.textContent = `The report could not be saved. ${failure || "No path came back."}`;
    return;
  }

  card.innerHTML = "";

  const title = document.createElement("h2");
  title.textContent = "Report saved";

  const line = document.createElement("p");
  line.textContent = "Your report is at";

  const where = document.createElement("code");
  where.className = "st-report-path";
  where.textContent = path;

  const after = document.createElement("p");
  after.style.marginTop = "12px";
  after.textContent =
    "— attach it to a GitHub issue. The folder is already open with the file " +
    "selected. Nothing was uploaded.";

  const row = document.createElement("div");
  row.className = "st-report-row";

  const done = document.createElement("button");
  done.className = "btn btn-sm";
  done.textContent = "Done";
  done.addEventListener("click", close);

  const issues = document.createElement("button");
  issues.className = "btn btn-sm";
  issues.textContent = "Open GitHub Issues";
  issues.addEventListener("click", () => {
    void invoke<boolean>("open_issues_page").catch(() => {
      after.textContent =
        "— attach it to a GitHub issue at github.com/nur-arpon/Spaceadom/issues. " +
        "The browser could not be opened from here.";
    });
  });

  row.append(done, issues);
  card.append(title, line, where, after, row);
  issues.focus();
}
