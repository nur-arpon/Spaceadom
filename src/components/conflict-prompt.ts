/**
 * conflict-prompt.ts — the top-centre offer to close a conflicting program.
 *
 * PROBLEM 157. The first build (1.0.63) put two permanent buttons under every
 * detected conflict in Settings. The owner's verdict: *"this thing always
 * staying there in the settings isn't worth it. This thing can just pop up
 * when someone presses the thing that is conflicting."* He is right — it is a
 * once-in-a-lifetime action sitting permanently in a panel you open often.
 *
 * So the conflict ROW is the trigger, and this is what it raises: one prompt,
 * where every other transient message in this app appears (top centre, beside
 * `#conflict-banner`), with the two choices and nothing else.
 *
 * WHY IT IS ITS OWN MODULE: `settings-panel.ts` already imports `main.ts`, and
 * this needs `invoke` + `sfx` only. Keeping it a leaf means the preview
 * harness can raise it without dragging main's bootstrap in (the mistake that
 * blanked the harness in PROBLEM 148).
 */
import { invoke } from "@tauri-apps/api/core";
import { sfx } from "../sfx";

export interface ConflictLike {
  process: string;
  product: string;
  detail: string;
}

interface CloseOutcome {
  closed: boolean;
  needsPermission: boolean;
  message: string;
}

let _host: HTMLElement | null = null;

function close(): void {
  if (!_host) return;
  const el = _host;
  _host = null;
  el.classList.add("is-leaving");
  el.addEventListener("transitionend", () => el.remove(), { once: true });
  window.setTimeout(() => el.remove(), 400);   // hidden documents never fire it
}

/** Raise the offer for one conflict. Replaces any prompt already up. */
export function openConflictPrompt(c: ConflictLike, onChanged?: () => void): void {
  close();

  const host = document.createElement("div");
  host.className = "conflict-prompt";
  host.setAttribute("role", "dialog");
  host.setAttribute("aria-label", `Close ${c.product}`);
  // It sits over the stage, which closes popovers on any click — and closing
  // the settings panel out from under this prompt would strand it.
  host.addEventListener("click", (e) => e.stopPropagation());

  const title = document.createElement("div");
  title.className = "conflict-prompt-title";
  title.textContent = `Close ${c.product}?`;

  const body = document.createElement("div");
  body.className = "conflict-prompt-body";
  body.textContent =
    `${c.product} is holding your keyboard, and only one program can. ` +
    `Windows may ask your permission — choose Yes if it does.`;

  const row = document.createElement("div");
  row.className = "conflict-prompt-row";

  const btnTemp = document.createElement("button");
  btnTemp.className = "btn btn-sm";
  btnTemp.textContent = "Close it now";
  const btnPerm = document.createElement("button");
  btnPerm.className = "btn btn-sm btn-danger";
  // The owner's wording, 2026-08-20 ("close it and obstruct restart" / "refrain
  // it from starting"). The button has to say what it changes on the machine,
  // because it is the one that edits another program's settings.
  btnPerm.textContent = "Close it and stop it from restarting";
  const btnNo = document.createElement("button");
  btnNo.className = "btn btn-sm";
  btnNo.textContent = "Not now";

  const run = async (permanent: boolean): Promise<void> => {
    btnTemp.disabled = btnPerm.disabled = true;
    body.textContent = `Closing ${c.product}…`;
    row.hidden = true;
    try {
      const r = await invoke<CloseOutcome>("close_conflict", {
        process: c.process, permanent, elevate: false,
      });
      body.textContent = r.message;
      if (r.closed) {
        sfx.confirm();
        onChanged?.();
        window.setTimeout(close, 3400);
        return;
      }
      // It could not be closed. Rather than leave the user with a refusal,
      // hand them the place they CAN turn it off — the owner's instruction:
      // "direct the user to startup menu of task manager… guide the user".
      row.hidden = false;
      btnTemp.hidden = btnPerm.hidden = true;
      const tm = document.createElement("button");
      tm.className = "btn btn-sm";
      tm.textContent = "Open Windows Start-up settings";
      tm.addEventListener("click", () => {
        void invoke("open_startup_manager");
        body.textContent =
          `Task Manager is opening on its Start-up apps tab. Find ${c.product} in the ` +
          `list, select it, and choose Disable — then restart your PC.`;
        tm.disabled = true;
      });
      row.insertBefore(tm, btnNo);
      btnNo.textContent = "Close this";
    } catch (e) {
      console.error("close_conflict failed:", e);
      body.textContent = "Spaceadom could not reach the part of itself that closes programs.";
      row.hidden = false;
      btnTemp.hidden = btnPerm.hidden = true;
      btnNo.textContent = "Close this";
    }
  };

  btnTemp.addEventListener("click", () => void run(false));
  btnPerm.addEventListener("click", () => void run(true));
  btnNo.addEventListener("click", close);

  row.append(btnTemp, btnPerm, btnNo);
  host.append(title, body, row);
  document.body.appendChild(host);
  _host = host;
  sfx.arm();

  // Escape always backs out of a dialog. Capture phase, because sky mode and
  // the special cards also listen for Escape and the topmost thing wins.
  const esc = (e: KeyboardEvent) => {
    if (e.key !== "Escape" || !_host) return;
    e.stopPropagation();
    close();
    document.removeEventListener("keydown", esc, true);
  };
  document.addEventListener("keydown", esc, true);

  // A forced reflow, NOT requestAnimationFrame. rAF does not fire in a window
  // that is not compositing — and if the class never lands, the prompt stays
  // at opacity 0 forever, i.e. the feature silently does not exist. Reading
  // offsetWidth flushes layout synchronously, which is all the transition
  // needs to have a "from" state.
  void host.offsetWidth;
  host.classList.add("is-in");
}
