/**
 * whats-new-sheet.ts — PROBLEM 249. "What changed?", answered with the actual
 * release notes instead of a version number.
 *
 * PROBLEM 245 shipped a one-time toast reading `Updated to 1.0.X`. It says
 * that something happened without saying what, which is the shape of
 * notification a person learns to dismiss without reading. This is the other
 * half: when `release_notes.rs` managed to fetch the GitHub release body for
 * the version now running, the dashboard shows it in a compact sheet.
 *
 * A LEAF MODULE (CLAUDE.md, PROBLEM 148): it imports `invoke` and the opener
 * plugin, and nothing else — no `main.ts`, no sibling component. Same reason
 * `report-dialog.ts` is its own file: `main.ts` owns when this opens, this
 * file owns what it looks like, and `preview.ts` could render it without
 * dragging the dashboard in. Its CSS is injected from here for the same
 * reason — a component another file imports should not need a second edit
 * somewhere else to be visible. Theme handling is free: every colour is a
 * design-system token and `body.nocturne` redefines the tokens.
 *
 * ============================ THE RENDERER =============================
 *
 * **NOTHING FROM THE NETWORK IS EVER ASSIGNED TO `innerHTML`.** The body of a
 * GitHub release is text somebody typed into a web form, it arrives over the
 * network, and the dashboard webview holds `invoke` — i.e. the whole Rust
 * command surface. A markdown-to-HTML string concatenation here would be a
 * remote-input-to-script path in the one window that can ask Rust to do
 * things. So the renderer builds DOM NODES and every scrap of author text
 * reaches the page through `textContent`, which cannot become markup however
 * it is spelled.
 *
 * That also settles "which markdown library": none. A dependency would be a
 * dependency that emits an HTML STRING, which is the thing being avoided,
 * and it would be a second CDN-free bundle to audit for a panel that needs
 * four constructs. What is supported is exactly what release notes use:
 *
 *   - `#`, `##`, `###` headings
 *   - `-` / `*` bullets (one level; a nested list flattens rather than
 *     failing, which is the right trade for notes nobody proof-reads)
 *   - `**bold**`
 *   - `[label](url)` links
 *
 * Everything else — tables, images, code fences, blockquotes — renders as its
 * own plain text rather than disappearing. A line that this renderer does not
 * understand is still a line the user can read, and silently swallowing part
 * of a changelog is worse than showing its punctuation.
 */

import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

/** The one sheet. A second call while it is up re-focuses instead of stacking. */
let host: HTMLDivElement | null = null;

const STYLE_ID = "st-whats-new-styles";

const CSS = `
#st-wn-scrim {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(32, 22, 12, .38);
  animation: st-wn-fade 180ms var(--ease-out) both;
}
#st-wn-card {
  width: min(540px, calc(100vw - 64px));
  max-height: calc(100vh - 96px);
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  padding: 20px 22px 16px;
  border-radius: var(--radius-lg, 16px);
  background: var(--st-card, #fdf6e9);
  border: 1px solid var(--st-border, #d8c9ab);
  color: var(--st-text, #201e1d);
  box-shadow: var(--shadow-lg, 0 18px 50px rgba(90, 60, 30, .2));
  font-family: var(--st-font-body, system-ui, sans-serif);
  animation: st-wn-pop 420ms var(--ease-spring, cubic-bezier(.34,1.3,.4,1)) both;
}
#st-wn-card h2 {
  margin: 0 0 10px;
  font-family: var(--st-font-heading, Georgia, serif);
  font-size: 19px;
  font-weight: 400;
}
#st-wn-body {
  overflow: auto;
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--st-text-soft, #5c554d);
  /* The notes are the only part of this sheet whose length is not ours to
     decide, so the scroll lives here and the title and button stay put. */
  padding-right: 4px;
}
#st-wn-body h3 {
  margin: 14px 0 4px;
  font-family: var(--st-font-heading, Georgia, serif);
  font-size: 14px;
  font-weight: 400;
  color: var(--st-text, #201e1d);
}
#st-wn-body h3:first-child { margin-top: 0; }
#st-wn-body h4 {
  margin: 12px 0 4px;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--st-text, #201e1d);
}
#st-wn-body p { margin: 0 0 8px; }
#st-wn-body ul { margin: 0 0 8px; padding-left: 18px; }
#st-wn-body li { margin: 0 0 3px; }
#st-wn-body a {
  color: var(--st-accent, #b8763c);
  text-decoration: underline;
  text-underline-offset: 2px;
  cursor: pointer;
}
#st-wn-body a:focus-visible { outline: 2px solid var(--st-accent-brd, #e0ac80); outline-offset: 2px; }
.st-wn-row { display: flex; gap: 8px; justify-content: flex-end; margin-top: 14px; }
@keyframes st-wn-fade { from { opacity: 0 } to { opacity: 1 } }
@keyframes st-wn-pop {
  from { opacity: 0; transform: translateY(10px) scale(.97) }
  to   { opacity: 1; transform: none }
}
@media (prefers-reduced-motion: reduce) {
  /* CLAUDE.md: reduced motion renders FINAL states, it does not just shorten. */
  #st-wn-scrim, #st-wn-card { animation: none }
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

// ---------------------------------------------------------------------------
// The minimal, injection-proof markdown renderer
// ---------------------------------------------------------------------------

/**
 * Only `http:` and `https:` links are ever wired up. Anything else — and the
 * one that matters is `javascript:` — is rendered as its LABEL with no link
 * at all, so a hostile release body cannot get a scheme past this into
 * `openUrl`. The check is on the parsed URL's protocol, never on the string's
 * prefix: `JaVaScRiPt:`, leading whitespace and percent-encoding all defeat a
 * prefix test and none of them defeat `new URL()`.
 */
function safeHref(raw: string): string | null {
  try {
    const u = new URL(raw.trim());
    return u.protocol === "http:" || u.protocol === "https:" ? u.href : null;
  } catch {
    return null;
  }
}

/**
 * Inline markdown → DOM nodes appended to `into`. Handles `**bold**` and
 * `[label](url)`; everything else is literal text.
 *
 * Every text run goes through `document.createTextNode`, so no combination of
 * angle brackets, entities or quotes in the author's text can become markup.
 */
function renderInline(into: HTMLElement, text: string): void {
  // One pass, one regex, alternating between the two constructs so a link
  // inside bold and bold inside a link label both degrade to plain text
  // rather than to a half-open element.
  // The URL group allows ONE level of balanced parentheses —
  // `(?:[^()\s]|\([^()\s]*\))+`. Two shapes need it and both showed up the
  // first time this was exercised: a legitimate `…/wiki/Foo_(bar)` link, and
  // a hostile `[x](javascript:alert(document.cookie))`. With a naive
  // `[^)\s]+` the match stops at the first `)`, so the good link breaks and
  // the bad one leaves a stray `)` sitting in the sentence after its label is
  // refused. Neither is a security hole — the scheme check below still
  // refuses the second — but a renderer that visibly mangles its input is one
  // nobody trusts the sanitising half of.
  const pattern = /\*\*([^*]+)\*\*|\[([^\]\n]+)\]\(((?:[^()\s]|\([^()\s]*\))+)\)/g;
  let last = 0;
  for (let m = pattern.exec(text); m !== null; m = pattern.exec(text)) {
    if (m.index > last) into.appendChild(document.createTextNode(text.slice(last, m.index)));
    if (m[1] !== undefined) {
      const b = document.createElement("strong");
      b.textContent = m[1];
      into.appendChild(b);
    } else {
      const label = m[2] ?? "";
      const href = safeHref(m[3] ?? "");
      if (href) {
        const a = document.createElement("a");
        a.textContent = label;
        // `href` is set so the link reads as one — hover cursor, context
        // menu, "Copy link address" — but every activation is intercepted:
        // the webview must never navigate away from the dashboard, and the
        // system browser is where a release link belongs.
        a.href = href;
        // REVIEW FIXES 2026-09-05 (LOW) — `auxclick` TOO, not just `click`.
        //
        // The comment here used to say the href was kept "for the status bar
        // and for middle-click", which described the hole as if it were a
        // feature. A middle-click (or a Ctrl+click, which WebView2 also
        // reports as a non-primary activation) does not fire `click`; it fires
        // `auxclick`, and the browser then follows the href ITSELF. In a
        // Tauri webview there is no tab to open one in, so what actually
        // happens is the DASHBOARD NAVIGATES TO GITHUB: the whole app is
        // replaced by a web page, with no back button, no chrome, and no way
        // out but restarting the app. The one surface where this can happen is
        // release notes, whose links come from a remote release body.
        //
        // Both listeners are the same handler for the same reason: `href` has
        // already been through `safeHref`, so `openUrl` is the right
        // destination for either kind of press — the only thing that differs
        // is which event the webview chose to send.
        const openInBrowser = (ev: Event) => {
          ev.preventDefault();
          // `openUrl` (@tauri-apps/plugin-opener) rather than a Rust command
          // with a compile-time constant URL, which is PROBLEM 164's rule for
          // this repo. That rule cannot apply here and the reason is worth
          // stating: a release-notes link is an ARBITRARY url that arrives at
          // runtime, so there is no constant to bake in, and a Rust command
          // taking a url parameter would grant the webview exactly the
          // "choose where this goes" power the rule exists to withhold. The
          // guard is `safeHref` above instead — scheme-checked before the
          // element is even created. `opener:default` is already granted,
          // unrestricted, in capabilities/default.json.
          void openUrl(href).catch(() => {});
        };
        a.addEventListener("click", openInBrowser);
        a.addEventListener("auxclick", openInBrowser);
        into.appendChild(a);
      } else {
        // A refused scheme still shows its label — losing the words entirely
        // would make a changelog look truncated rather than sanitised.
        into.appendChild(document.createTextNode(label));
      }
    }
    last = m.index + m[0].length;
  }
  if (last < text.length) into.appendChild(document.createTextNode(text.slice(last)));
}

/**
 * Block-level markdown → DOM, appended to `into`.
 *
 * Deliberately line-based rather than a real parser: release notes are a flat
 * list of headings and bullets, and every construct this does not know about
 * falls through to a paragraph of its own literal text. Nothing is dropped.
 */
export function renderNotes(into: HTMLElement, markdown: string): void {
  const lines = markdown.replace(/\r\n?/g, "\n").split("\n");
  let list: HTMLUListElement | null = null;
  let para: HTMLParagraphElement | null = null;

  const endList = () => { list = null; };
  const endPara = () => { para = null; };

  for (const raw of lines) {
    const line = raw.trimEnd();
    const trimmed = line.trim();

    if (trimmed === "") { endList(); endPara(); continue; }

    const heading = /^(#{1,6})\s+(.*)$/.exec(trimmed);
    if (heading) {
      endList(); endPara();
      // Two levels of visual weight, not six: the sheet is 540px wide and a
      // six-step scale inside it reads as noise. h1/h2/h3 → h3, the rest → h4.
      // They are `h3`/`h4` in the DOM (never `h1`) because this sheet lives
      // inside the dashboard's own heading outline.
      const el = document.createElement(heading[1].length <= 3 ? "h3" : "h4");
      renderInline(el, heading[2]);
      into.appendChild(el);
      continue;
    }

    const bullet = /^[-*]\s+(.*)$/.exec(trimmed);
    if (bullet) {
      endPara();
      if (!list) { list = document.createElement("ul"); into.appendChild(list); }
      const li = document.createElement("li");
      renderInline(li, bullet[1]);
      list.appendChild(li);
      continue;
    }

    endList();
    if (!para) { para = document.createElement("p"); into.appendChild(para); }
    else para.appendChild(document.createTextNode(" "));
    renderInline(para, trimmed);
  }
}

// ---------------------------------------------------------------------------
// The sheet
// ---------------------------------------------------------------------------

/**
 * Show the What's New sheet for `version`.
 *
 * `rolledBack` changes the wording and nothing else: after a rollback the
 * version went BACKWARDS, so "What's new in 1.0.X" would be the wrong
 * sentence — the honest one is "You're back on 1.0.X".
 *
 * The notes are fetched AFTER the sheet is on screen, not before. Rust has
 * already told us `has_notes` is true, so there is something to show; opening
 * first means the sheet appears at the moment of the click rather than after
 * a cache read or a network round trip, and a fetch that then fails (the
 * cache file was removed between the two calls) degrades to one honest line
 * instead of a sheet that never appeared and never said why.
 */
export function openWhatsNew(version: string, rolledBack = false): void {
  if (host) {
    host.querySelector<HTMLButtonElement>("#st-wn-ok")?.focus();
    return;
  }
  ensureStyles();

  host = document.createElement("div");
  host.id = "st-wn-scrim";
  host.addEventListener("click", (e) => { if (e.target === host) close(); });

  const card = document.createElement("div");
  card.id = "st-wn-card";
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-modal", "true");

  const title = document.createElement("h2");
  title.textContent = rolledBack
    ? `You're back on ${version}`
    : `What's new in ${version}`;
  card.setAttribute("aria-label", title.textContent);

  const body = document.createElement("div");
  body.id = "st-wn-body";
  const loading = document.createElement("p");
  loading.textContent = "Reading the release notes…";
  body.appendChild(loading);

  const row = document.createElement("div");
  row.className = "st-wn-row";
  const ok = document.createElement("button");
  ok.id = "st-wn-ok";
  ok.className = "btn btn-sm";
  ok.textContent = "Got it";
  ok.addEventListener("click", close);
  row.appendChild(ok);

  card.append(title, body, row);
  host.appendChild(card);
  document.body.appendChild(host);
  // Capture phase: the dashboard has its own document-level Escape handler for
  // popovers and the key-detail panel, and a modal has to win.
  document.addEventListener("keydown", onKeydown, true);
  ok.focus();

  void invoke<string | null>("get_release_notes", { version })
    .then((notes) => {
      // The sheet may have been dismissed while the fetch was in flight.
      if (!host || !body.isConnected) return;
      body.textContent = "";
      if (notes && notes.trim()) {
        renderNotes(body, notes);
      } else {
        const p = document.createElement("p");
        p.textContent =
          "The release notes for this version couldn't be read just now. " +
          "They're on the GitHub releases page.";
        body.appendChild(p);
      }
    })
    .catch(() => {
      if (!host || !body.isConnected) return;
      body.textContent = "";
      const p = document.createElement("p");
      p.textContent =
        "The release notes for this version couldn't be read just now. " +
        "They're on the GitHub releases page.";
      body.appendChild(p);
    });
}
