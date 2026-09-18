/**
 * vk-names.ts — PHASE A (2026-09-18). Windows virtual-key codes by their
 * `VK_*` names (the vocabulary `src/data/windows-catalogue.json` writes its
 * chords in) and the short human label for a code (what the key editor's
 * chord caps and the board's sub-label show — the same words Rust's
 * `engine::specials::vk_name` uses for the toast).
 *
 * A LEAF MODULE: no imports, so the preview harness and every component can
 * read it.
 */

const NAMED: Record<string, number> = {
  VK_BACK: 0x08, VK_TAB: 0x09, VK_RETURN: 0x0d, VK_SHIFT: 0x10, VK_CONTROL: 0x11,
  VK_MENU: 0x12, VK_PAUSE: 0x13, VK_CAPITAL: 0x14, VK_ESCAPE: 0x1b, VK_SPACE: 0x20,
  VK_PRIOR: 0x21, VK_NEXT: 0x22, VK_END: 0x23, VK_HOME: 0x24, VK_LEFT: 0x25,
  VK_UP: 0x26, VK_RIGHT: 0x27, VK_DOWN: 0x28, VK_SNAPSHOT: 0x2c, VK_INSERT: 0x2d,
  VK_DELETE: 0x2e, VK_LWIN: 0x5b, VK_RWIN: 0x5c, VK_APPS: 0x5d,
  VK_LSHIFT: 0xa0, VK_RSHIFT: 0xa1, VK_LCONTROL: 0xa2, VK_RCONTROL: 0xa3,
  VK_LMENU: 0xa4, VK_RMENU: 0xa5,
  VK_VOLUME_MUTE: 0xad, VK_VOLUME_DOWN: 0xae, VK_VOLUME_UP: 0xaf,
  VK_MEDIA_NEXT_TRACK: 0xb0, VK_MEDIA_PREV_TRACK: 0xb1, VK_MEDIA_STOP: 0xb2,
  VK_MEDIA_PLAY_PAUSE: 0xb3,
  VK_OEM_1: 0xba, VK_OEM_PLUS: 0xbb, VK_OEM_COMMA: 0xbc, VK_OEM_MINUS: 0xbd,
  VK_OEM_PERIOD: 0xbe, VK_OEM_2: 0xbf, VK_OEM_3: 0xc0, VK_OEM_4: 0xdb,
  VK_OEM_5: 0xdc, VK_OEM_6: 0xdd, VK_OEM_7: 0xde,
};

/** `"VK_LWIN"` → 0x5B; `"VK_S"` → 0x53; `"VK_F5"` → 0x74; unknown → null. */
export function vkFromName(name: string): number | null {
  const n = name.trim().toUpperCase();
  if (n in NAMED) return NAMED[n]!;
  const m = /^VK_([A-Z0-9])$/.exec(n);
  if (m) return m[1]!.charCodeAt(0);
  const f = /^VK_F(\d{1,2})$/.exec(n);
  if (f) {
    const k = Number(f[1]);
    if (k >= 1 && k <= 24) return 0x70 + k - 1;
  }
  return null;
}

/** Every name in a catalogue chord, or null if any name is unknown. */
export function chordFromNames(names: string[]): number[] | null {
  const out: number[] = [];
  for (const n of names) {
    const vk = vkFromName(n);
    if (vk === null) return null;
    out.push(vk);
  }
  return out;
}

const LABELS: Record<number, string> = {
  0x08: "⌫", 0x09: "Tab", 0x0d: "Enter", 0x10: "Shift", 0x11: "Ctrl", 0x12: "Alt",
  0x13: "Pause", 0x14: "Caps", 0x1b: "Esc", 0x20: "Space", 0x21: "PgUp", 0x22: "PgDn",
  0x23: "End", 0x24: "Home", 0x25: "←", 0x26: "↑", 0x27: "→", 0x28: "↓",
  0x2c: "PrtSc", 0x2d: "Ins", 0x2e: "Del", 0x5b: "Win", 0x5c: "Win", 0x5d: "Menu",
  0xa0: "Shift", 0xa1: "Shift", 0xa2: "Ctrl", 0xa3: "Ctrl", 0xa4: "Alt", 0xa5: "RAlt",
  0xad: "Mute", 0xae: "Vol−", 0xaf: "Vol+", 0xb0: "Next", 0xb1: "Prev", 0xb2: "Stop",
  0xb3: "Play", 0xba: ";", 0xbb: "=", 0xbc: ",", 0xbd: "-", 0xbe: ".", 0xbf: "/",
  0xc0: "`", 0xdb: "[", 0xdc: "\\", 0xdd: "]", 0xde: "'",
};

/** The short label for a virtual key: `Win`, `Shift`, `S`, `F5`, `Vol+`. */
export function vkLabel(vk: number): string {
  if (vk in LABELS) return LABELS[vk]!;
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk);
  if (vk >= 0x30 && vk <= 0x39) return String.fromCharCode(vk);
  if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x70 + 1}`;
  return `VK ${vk.toString(16).toUpperCase().padStart(2, "0")}`;
}

/** `Win+Shift+S` — the same spelling Rust puts in the toast. */
export function chordLabel(keys: number[]): string {
  return keys.length ? keys.map(vkLabel).join("+") : "(no keys)";
}
