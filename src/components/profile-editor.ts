/**
 * profile-editor.ts — profiles, now behind the top-right pill popover.
 *
 * V14: the sidebar and the New Profile modal are gone; the design puts
 * profiles in a popover with pill rows and an inline "＋ New profile" field
 * (Dashboard Earthy v2.dc.html). All backend calls are V13's, unchanged:
 * set_active_profile / create_profile / rename_profile / delete_profile.
 *
 * set_active_profile SAVES on the Rust side — never persistConfig() after it
 * (that was the double config-save bug).
 */
import { invoke } from "@tauri-apps/api/core";
import { showToast } from "./toast";
import { askConfirm } from "./confirm-dialog";
import { offerUndo } from "../main";
import type { AppConfig } from "../types.ts";

const PROFILE_NAME_RE = /^[a-zA-Z0-9_]{1,24}$/;

let _config: AppConfig | null = null;
let _onProfileSwitch: ((name: string) => void) | null = null;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export function initProfileEditor(
  config: AppConfig,
  onProfileSwitch: (name: string) => void,
): void {
  _config = config;
  _onProfileSwitch = onProfileSwitch;
  wireNewProfile();
  renderProfileList();
}

export function refreshProfileList(config: AppConfig): void {
  _config = config;
  renderProfileList();
  syncPill();
}

/** Keep the top-right pill in step with the active profile. */
export function syncPill(): void {
  if (!_config) return;
  const name = _config.active_profile;
  const nameEl = document.getElementById("profile-pill-name");
  const initEl = document.getElementById("profile-pill-initial");
  if (nameEl) nameEl.textContent = name;
  if (initEl) initEl.textContent = (name[0] ?? "·").toUpperCase();
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

function renderProfileList(): void {
  if (!_config) return;
  const list = document.getElementById("profile-list");
  if (!list) return;

  list.innerHTML = "";

  _config.profiles.forEach((profile, i) => {
    const isActive = profile.name === _config!.active_profile;
    const count = Object.values(profile.bindings).filter(
      (b) => b.app || b.web_url,
    ).length;

    const row = document.createElement("div");
    row.className = "profile-row" + (isActive ? " active" : "");
    row.dataset.profileName = profile.name;
    row.setAttribute("role", "listitem");
    row.tabIndex = 0;
    row.setAttribute("aria-current", isActive ? "true" : "false");
    row.style.animationDelay = `${i * 55}ms`;

    const icon = document.createElement("span");
    icon.className = "profile-row-icon";
    icon.textContent = (profile.name[0] ?? "?").toUpperCase();

    const text = document.createElement("span");
    text.className = "profile-row-text";
    const nameEl = document.createElement("span");
    nameEl.className = "profile-row-name";
    nameEl.textContent = profile.name;      // textContent — user data
    const countEl = document.createElement("span");
    countEl.className = "profile-row-count";
    countEl.textContent = `${count} ${count === 1 ? "key" : "keys"}`;
    text.append(nameEl, countEl);

    const del = document.createElement("button");
    // PROBLEM 105 — an armed row says so ON the control, not only in a toast
    // that may have already faded. The fallback profile also carries a warning
    // in its tooltip, so the consequence is discoverable BEFORE the first click.
    const armed = _armedDelete === profile.name;
    del.className = "profile-row-del" + (armed ? " armed" : "");
    del.textContent = armed ? "Delete?" : "✕";
    del.title = profile.name === FALLBACK_PROFILE
      ? `Delete ${profile.name} — WARNING: this is the fallback profile. Keys you have not assigned in your other profiles are rerouted here and will stop working.`
      : `Delete ${profile.name}`;
    del.setAttribute("aria-label", del.title);

    row.append(icon, text, del);

    row.addEventListener("click", (e) => {
      if ((e.target as HTMLElement).closest(".profile-row-del")) return;
      void switchProfile(profile.name);
    });
    row.addEventListener("keydown", (e) => {
      if (e.key === "Enter") void switchProfile(profile.name);
    });
    row.addEventListener("dblclick", () => startInlineRename(row, profile.name));
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      void confirmDeleteProfile(profile.name);
    });

    list.appendChild(row);
  });

  syncPill();
}

// ---------------------------------------------------------------------------
// Switch / rename / delete
// ---------------------------------------------------------------------------

async function switchProfile(name: string): Promise<void> {
  try {
    await invoke("set_active_profile", { name });
    if (_config) _config.active_profile = name;
    renderProfileList();
    closeProfilePopover();
    if (_onProfileSwitch) _onProfileSwitch(name);
    showToast(`👤 Profile: ${name}`);
  } catch (_) {
    showToast("⚠️ Failed to switch profile");
  }
}

function startInlineRename(row: HTMLElement, oldName: string): void {
  const nameEl = row.querySelector<HTMLElement>(".profile-row-name");
  if (!nameEl) return;

  const input = document.createElement("input");
  input.className = "input";
  input.value = oldName;
  input.maxLength = 24;
  input.style.cssText = "height:24px; padding:0 10px; font-size:12px; width:100%;";

  nameEl.replaceWith(input);
  input.focus();
  input.select();
  // Clicking into the field must not also switch profile.
  input.addEventListener("click", (e) => e.stopPropagation());

  let done = false;
  const commit = async () => {
    if (done) return;
    done = true;
    const newName = input.value.trim();
    if (!newName || newName === oldName) { renderProfileList(); return; }
    if (!PROFILE_NAME_RE.test(newName)) {
      showToast("⚠️ Name: 1–24 chars, letters/numbers/underscore");
      renderProfileList();
      return;
    }
    try {
      await invoke("rename_profile", { oldName, newName });
      if (_config) {
        const p = _config.profiles.find((x) => x.name === oldName);
        if (p) p.name = newName;
        if (_config.active_profile === oldName) _config.active_profile = newName;
      }
      renderProfileList();
      showToast(`✅ Renamed → ${newName}`);
    } catch (_) {
      showToast("⚠️ Rename failed");
      renderProfileList();
    }
  };

  input.addEventListener("blur", () => void commit());
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") { e.preventDefault(); input.blur(); }
    if (e.key === "Escape") { done = true; renderProfileList(); }
  });
}

/**
 * PROBLEM 105 — two-step delete, with the fallback warning shown IN THE APP.
 *
 * The first attempt used `window.confirm`. It never appeared: this webview
 * does not render native script dialogs, so the delete went straight through
 * and the user saw no warning at all — worse than none, because the code
 * looked like it was protecting them. Every other destructive control here
 * already uses a two-step "Confirm" button for exactly this reason; this now
 * matches them.
 *
 * MUST match FALLBACK_PROFILE in src-tauri/src/config/schema.rs.
 */
const FALLBACK_PROFILE = "Founders";
/** Which profile row is armed for deletion, and the timer that disarms it. */
let _armedDelete: string | null = null;
let _armedDeleteTimer: number | undefined;

/**
 * PROBLEM 108 — two levels of friction, matched to the consequence.
 *
 * Ordinary profiles get the red "Delete?" pill: a light second click, which is
 * how every other destructive control in this app already behaves and is
 * enough for something the user can rebuild.
 *
 * The FALLBACK profile gets the full panel, because its consequence is not
 * about this profile at all — it silently breaks unassigned keys in every
 * OTHER profile, and that needs sentences the user has time to read.
 */
async function confirmDeleteProfile(name: string): Promise<void> {
  if (!_config || _config.profiles.length <= 1) {
    showToast("⚠️ Cannot delete the last profile");
    return;
  }

  if (name === FALLBACK_PROFILE) {
    const ok = await askConfirm({
      title: `Delete "${name}"?`,
      body: `This is the FALLBACK profile.

Any key you have not assigned in your ` +
            `other profiles is currently rerouted here — those keys will stop working.` +
            `

You will have 30 seconds to undo.`,
      danger: true,
      confirmLabel: "Delete anyway",
    });
    if (!ok) return;
  } else {
    // First click arms the pill; second within 4s deletes.
    if (_armedDelete !== name) {
      _armedDelete = name;
      window.clearTimeout(_armedDeleteTimer);
      _armedDeleteTimer = window.setTimeout(() => {
        _armedDelete = null;
        renderProfileList();
      }, 4000);
      renderProfileList();
      return;
    }
    window.clearTimeout(_armedDeleteTimer);
    _armedDelete = null;
  }

  try {
    await invoke("delete_profile", { name });
    _config.profiles = _config.profiles.filter((p) => p.name !== name);
    if (_config.active_profile === name) {
      _config.active_profile = _config.profiles[0].name;
      if (_onProfileSwitch) _onProfileSwitch(_config.active_profile);
    }
    renderProfileList();
    showToast(`🗑️ Deleted: ${name}`);
    // PROBLEM 99 — deleting a profile the USER created destroys bindings and
    // custom icons that exist in no other copy. This was the one destructive
    // path that stashed an undo in Rust but never offered it in the UI.
    offerUndo();
  } catch (e) {
    showToast(`⚠️ ${e}`);
  }
}

// ---------------------------------------------------------------------------
// Inline "＋ New profile"
// ---------------------------------------------------------------------------

function wireNewProfile(): void {
  const openBtn = document.getElementById("new-profile-btn") as HTMLButtonElement | null;
  const row = document.getElementById("new-profile-row") as HTMLElement | null;
  const input = document.getElementById("new-profile-input") as HTMLInputElement | null;
  const addBtn = document.getElementById("new-profile-add") as HTMLButtonElement | null;
  if (!openBtn || !row || !input || !addBtn) return;

  const open = () => {
    row.hidden = false;
    openBtn.hidden = true;
    input.value = "";
    input.focus();
  };
  const close = () => {
    row.hidden = true;
    openBtn.hidden = false;
    input.value = "";
  };

  openBtn.addEventListener("click", open);
  addBtn.addEventListener("click", () => void create(input, close));
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") void create(input, close);
    if (e.key === "Escape") close();
  });
}

async function create(input: HTMLInputElement, close: () => void): Promise<void> {
  const name = input.value.trim();
  if (!PROFILE_NAME_RE.test(name)) {
    showToast("⚠️ Name: 1–24 chars, letters/numbers/underscore");
    input.classList.add("error");
    return;
  }
  input.classList.remove("error");

  try {
    await invoke("create_profile", { name });
    if (_config) {
      _config.profiles.push({
        name,
        bindings: Object.fromEntries(
          "abcdefghijklmnopqrstuvwxyz"
            .split("")
            .map((k) => [k, { app: null, web_url: null, label: null }]),
        ),
      });
    }
    close();
    renderProfileList();
    showToast(`✅ Profile created: ${name}`);
  } catch (e) {
    showToast(`⚠️ ${e}`);
  }
}

function closeProfilePopover(): void {
  const pop = document.getElementById("profile-popover");
  const pill = document.getElementById("profile-pill");
  if (pop) pop.hidden = true;
  pill?.setAttribute("aria-expanded", "false");
}
