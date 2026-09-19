/**
 * rival-banner.ts — the WORDS and the BUTTON of the second-install banner,
 * chosen from the backend's verdict. Pure: no DOM, no `invoke`, no imports
 * from main.ts, so `scripts/rival-banner.test.ts` can pin every variant with
 * `node` alone (the same arrangement as `own-window-keys.ts`).
 *
 * PROBLEM 272 (2026-09-20). This used to be a nested ternary inside
 * `main.ts::checkRivalInstall`, one arm per PROBLEM (129/141, 238, 250, 250
 * follow-up, 254). The owner's friend on the Store copy installed the
 * setup.exe on top and got two copies with no one-click fix — and when the
 * fix was added, the banner's choice of words and button became the thing
 * that decides whether a person is offered a removal that cannot work (a
 * packaged process removing files outside its package) or denied one that
 * can (an unpackaged process asking the deployment service to remove a
 * package for its own user). A decision like that is tested, not eyeballed.
 *
 * The rule the whole banner is built on: **a control's presence is a
 * promise.** `button: null` means no control at all — not a disabled one, not
 * a hidden one — and the text has to be complete on its own for that case.
 *
 * The five kinds `rival_install::status_kind()` can return, and what each
 * gets:
 *
 * | kind             | who we are           | button                    | backend call         |
 * | ---------------- | -------------------- | ------------------------- | -------------------- |
 * | `second_copy`    | unpackaged, per-user | Remove the old copy       | repair_rival_install (UAC once, `plan_removal`) |
 * | `orphaned_entry` | unpackaged           | Remove the leftover entry | repair_rival_install (registry only, PROBLEM 244) |
 * | `packaged_host`  | THE STORE COPY       | Open Installed apps       | open_installed_apps (directions; never a removal) |
 * | `store_copy`     | unpackaged (EXE or MSI), a Store copy beside us | Remove the Store copy | repair_rival_install → `remove_store_package`, no UAC |
 * | anything else    | —                    | —                         | — (not shown) |
 */

/** What the button does when pressed. `null` button = directions only. */
export type RivalBannerAction = "repair" | "open_installed_apps" | "remove_store";

export interface RivalBannerCopy {
  /** The banner sentence(s). Complete on its own when `button` is null. */
  text: string;
  /** The one control, or none. */
  button: { label: string; action: RivalBannerAction; busyLabel: string } | null;
  /** Toast after a successful `repair` / `remove_store`. */
  okToast: string;
  /** Toast after a failed one. */
  failToast: string;
  /**
   * PROBLEM 272 — the confirmation shown BEFORE `remove_store`, and only for
   * it. One confirm, in the app's own dialog (`askConfirm`; `window.confirm`
   * does not render here — PROBLEM 106). Null for every other action: the
   * elevated ones already get a UAC prompt, and "open a window" needs none.
   */
  confirm: { title: string; body: string; confirmLabel: string } | null;
}

/** The inputs, exactly as `get_rival_install` + `is_portable_install` hand them over. */
export interface RivalBannerInputs {
  kind: string;
  /** PROBLEM 254 — WE are an unzipped portable copy beside an installed one. */
  portable: boolean;
  /** The OTHER copy's version, for the arms that name it. */
  version: string;
  /** The OTHER copy's exe path — or, for `store_copy`, a SENTENCE (never interpolated). */
  path: string;
}

/**
 * The directions a `store_copy` banner falls back to when the removal fails,
 * and the whole of what the packaged side says. Kept as one string so the
 * text the user is left with after a failed button press is the text the old
 * banner always showed.
 */
export const STORE_COPY_DIRECTIONS =
  "Remove it from Settings > Apps > Installed apps, or keep it and uninstall this copy instead.";

export function rivalBannerCopy(i: RivalBannerInputs): RivalBannerCopy | null {
  const orphan = i.kind === "orphaned_entry";

  if (i.kind === "store_copy") {
    // PROBLEM 272 — WE are unpackaged (the setup.exe copy, or the .msi copy;
    // both reach this kind) and a Microsoft Store copy is registered for this
    // user. The removal is the deployment service's own per-user request —
    // no elevation, no files touched by us — so the button is offered, once
    // confirmed. `path` is a sentence naming the package, so neither it nor
    // `version` is interpolated: see rival_install.rs's test
    // `a_store_copy_finding_can_never_yield_a_deletable_directory`.
    return {
      text:
        "A Microsoft Store copy of Spaceadom is also installed — remove it? " +
        "Both start with Windows and fight over the spacebar, so one has to go. " +
        "Removing the Store copy keeps this one and your settings; no permission prompt.",
      button: { label: "Remove the Store copy", action: "remove_store", busyLabel: "Removing…" },
      okToast: "✅ Store copy removed — one Spaceadom left, no more spacebar conflict",
      failToast: "⚠️ Could not remove the Store copy — " + STORE_COPY_DIRECTIONS,
      confirm: {
        title: "Remove the Microsoft Store copy?",
        body:
          "Windows will uninstall the Store version of Spaceadom for your account. " +
          "This copy, and your profiles and settings, stay exactly as they are.\n\n" +
          "If you would rather keep the Store version, cancel and uninstall this copy instead.",
        confirmLabel: "Remove the Store copy",
      },
    };
  }

  if (i.kind === "packaged_host") {
    // PROBLEM 250 — WE are the Microsoft Store copy. The fault is the same
    // (two copies, both at logon, both hooking the spacebar); the remedy is
    // not. A packaged process must never touch files outside its package and
    // must never elevate to do so (CLAUDE.md, MSIX section; PROBLEM 244 is
    // what that looks like when it goes wrong), so this side gives directions
    // and a door — the button OPENS Installed apps, it does not remove
    // anything. The wording says so, in the owner's words from the brief.
    return {
      text:
        `This is the Microsoft Store version of Spaceadom, and another copy ` +
        `(v${i.version}) is also installed at ${i.path}. Both start with Windows and ` +
        "fight over the spacebar, so one has to go. The Store version cannot remove " +
        "the other one for you — remove this copy from Installed apps, or keep it and " +
        "uninstall the other. Your settings stay where they are.",
      button: { label: "Open Installed apps", action: "open_installed_apps", busyLabel: "Open Installed apps" },
      okToast: "",
      failToast: "⚠️ Could not open Settings — it is under Apps ▸ Installed apps",
      confirm: null,
    };
  }

  if (orphan) {
    // PROBLEM 238 / 244 — nothing is running twice; the backend deletes the
    // registration only, never files, and the text has to say so.
    return {
      text:
        "An old installer entry is left over. Nothing is running twice, but " +
        "Programs and Features lists Spaceadom twice. " +
        "This only removes the leftover entry from Programs and Features. " +
        "Your app and settings are not touched.",
      button: { label: "Remove the leftover entry", action: "repair", busyLabel: "Removing…" },
      okToast: "✅ Leftover entry removed — Programs and Features now lists one Spaceadom",
      failToast: "⚠️ Not removed — the permission prompt was declined",
      confirm: null,
    };
  }

  if (i.kind === "second_copy" || i.kind === "") {
    const okToast = "✅ Old copy removed — one Spaceadom left, no more spacebar conflict";
    const failToast = "⚠️ Not removed — the permission prompt was declined";
    if (i.portable) {
      // PROBLEM 254 — the portable shape: it is the OTHER copy that is
      // installed, and closing this one is a complete remedy too.
      return {
        text:
          `You're running the PORTABLE copy of Spaceadom (unzipped, nothing ` +
          `installed), and an installed copy (v${i.version}) is also on this PC at ` +
          `${i.path}. Both put a keyboard hook on the spacebar, so only one can run ` +
          "at a time. Closing this portable copy — or deleting its folder — settles " +
          "it with no uninstaller. Or remove the installed one below (Windows will " +
          "ask for permission once).",
        button: { label: "Remove the old copy", action: "repair", busyLabel: "Removing…" },
        okToast,
        failToast,
        confirm: null,
      };
    }
    // PROBLEM 129 / 141 — the original: a per-machine copy beside us.
    return {
      text:
        `Another copy of Spaceadom (v${i.version}) is installed at ${i.path}. ` +
        "Both start with Windows and fight over the spacebar. " +
        "One click removes the old one (Windows will ask for permission once).",
      button: { label: "Remove the old copy", action: "repair", busyLabel: "Removing…" },
      okToast,
      failToast,
      confirm: null,
    };
  }

  return null;
}
