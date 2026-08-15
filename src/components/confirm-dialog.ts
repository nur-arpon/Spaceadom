/**
 * confirm-dialog.ts — an in-app confirmation panel.
 *
 * PROBLEM 106. This exists because `window.confirm` DOES NOT RENDER in this
 * webview: it returns instantly, so a delete guarded by it went straight
 * through and the user saw no warning at all. A warning that cannot appear is
 * worse than none, because the code looks like it is protecting someone.
 *
 * Deliberately built from the app's own DOM:
 *   * it renders, because it is just elements on the page;
 *   * it STAYS until answered, unlike a toast, so a three-line explanation can
 *     actually be read;
 *   * the text wraps, unlike the single-line toast pill.
 *
 * Escape cancels, Enter confirms, and the first click outside cancels — all
 * the ways a person expects to back out of something destructive.
 */

export interface ConfirmOptions {
  title: string;
  /** Body text. "\n\n" becomes a paragraph break. */
  body: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** Style the confirm button as destructive. */
  danger?: boolean;
}

export function askConfirm(opts: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    const back = document.createElement("div");
    back.className = "confirm-back";

    const box = document.createElement("div");
    box.className = "confirm-box";
    box.setAttribute("role", "alertdialog");
    box.setAttribute("aria-modal", "true");

    const h = document.createElement("div");
    h.className = "confirm-title";
    h.textContent = opts.title;              // textContent — profile names are user data
    box.appendChild(h);

    for (const para of opts.body.split("\n\n")) {
      const p = document.createElement("p");
      p.className = "confirm-body";
      p.textContent = para;
      box.appendChild(p);
    }

    const row = document.createElement("div");
    row.className = "confirm-actions";

    const cancel = document.createElement("button");
    cancel.className = "btn";
    cancel.textContent = opts.cancelLabel ?? "Cancel";

    const go = document.createElement("button");
    go.className = "btn" + (opts.danger ? " btn-danger" : "");
    go.textContent = opts.confirmLabel ?? "Confirm";

    row.append(cancel, go);
    box.appendChild(row);
    back.appendChild(box);
    document.body.appendChild(back);

    const close = (result: boolean) => {
      document.removeEventListener("keydown", onKey, true);
      back.remove();
      resolve(result);
    };
    const onKey = (e: KeyboardEvent) => {
      // Capture phase + stopPropagation: the dashboard has document-level
      // handlers for Escape (close panels) and Space (blur buttons), and this
      // panel must own the keyboard while it is up.
      if (e.key === "Escape") { e.stopPropagation(); e.preventDefault(); close(false); }
      if (e.key === "Enter")  { e.stopPropagation(); e.preventDefault(); close(true); }
    };

    cancel.addEventListener("click", (e) => { e.stopPropagation(); close(false); });
    go.addEventListener("click", (e) => { e.stopPropagation(); close(true); });
    // Clicking the backdrop cancels; clicking INSIDE the box must not, or the
    // panel would vanish while the user is reading it.
    back.addEventListener("click", (e) => { if (e.target === back) close(false); });
    box.addEventListener("click", (e) => e.stopPropagation());
    document.addEventListener("keydown", onKey, true);

    // Focus Cancel, not Confirm: a stray Enter should never destroy anything.
    cancel.focus();
  });
}
