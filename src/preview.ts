/**
 * preview.ts — DEV-ONLY visual harness for the dashboard.
 *
 * Renders the real keyboard-matrix component and the real stylesheet against
 * a stub config so the design can be compared to Dashboard Earthy v2.dc.html
 * without the Rust backend. Not a Vite build input; it never ships.
 *
 * Query flags: ?dark  ?profiles  ?gear  ?specials  ?editor
 */
import { initKeyboardMatrix, DESIGN_W, DESIGN_H } from "./components/keyboard-matrix";
import type { AppConfig, KeyBinding } from "./types";

const q = new URLSearchParams(location.search);

const DEMO: Record<string, string> = {
  c: "Chrome", s: "Slack", n: "Notion", f: "Figma", m: "Mail", t: "Terminal",
  g: "GitHub", d: "Discord", w: "Word", e: "Excel", p: "Photos", o: "Obsidian",
  v: "VS Code", z: "Zoom",
};

const bindings: Record<string, KeyBinding> = {};
"abcdefghijklmnopqrstuvwxyz".split("").forEach((k) => {
  bindings[k] = DEMO[k]
    ? { app: `C:\\Apps\\${DEMO[k]}.exe`, web_url: null, label: DEMO[k] }
    : { app: null, web_url: null, label: null };
});

const config: AppConfig = {
  version: 1,
  active_profile: "Founders",
  rollover_ms: 50,
  guide_hud_delay_ms: 300,
  opacity_floor_pct: 30,
  browser_path: null,
  fullscreen_allowlist: [],
  dark_mode: q.has("dark"),
  sound_enabled: false,
  profiles: [
    { name: "Founders", bindings },
    { name: "Gamers", bindings: {} },
    { name: "Professionals", bindings: {} },
  ],
};

document.body.classList.toggle("nocturne", !!config.dark_mode);

// ---- the real component ----
initKeyboardMatrix(
  document.getElementById("keyboard-matrix")!,
  config,
  () => {},
  () => {},
);

// ---- fit (same maths as main.ts) ----
const outer = document.getElementById("keyboard-outer")!;
const scale = document.getElementById("keyboard-scale")!;
const fit = () => {
  const r = outer.getBoundingClientRect();
  const s = Math.min(1, (r.width - 12) / DESIGN_W, (r.height - 12) / DESIGN_H);
  scale.style.transform = `scale(${s.toFixed(4)})`;
};
fit();
new ResizeObserver(fit).observe(outer);

// ---- specials tray ----
const SPECIALS: [string, string][] = [
  ["␣ Esc", "Boss Key"], ["␣ `", "PiP Cycle"], ["␣ ⌫", "Force Close"],
  ["␣ ↑↑", "Scroll Top"], ["␣ ,", "Smart Search"], ["␣ .", "Pause"],
  ["␣ RAlt", "Cycle Profile"], ["␣ Scroll", "Opacity"],
];
const tray = document.getElementById("specials-tray")!;
SPECIALS.forEach(([combo, what], i) => {
  const item = document.createElement("span");
  item.className = "special-item";
  item.style.animationDelay = `${60 + i * 30}ms`;
  const k = document.createElement("kbd"); k.textContent = combo;
  const t = document.createElement("span"); t.textContent = what;
  item.append(k, t);
  tray.appendChild(item);
});

// ---- profile rows ----
const list = document.getElementById("profile-list")!;
config.profiles.forEach((p, i) => {
  const count = Object.values(p.bindings).filter((b) => b.app || b.web_url).length;
  const row = document.createElement("div");
  row.className = "profile-row" + (i === 0 ? " active" : "");
  row.style.animationDelay = `${i * 55}ms`;
  row.innerHTML =
    `<span class="profile-row-icon">${p.name[0]}</span>` +
    `<span class="profile-row-text"><span class="profile-row-name">${p.name}</span>` +
    `<span class="profile-row-count">${count} keys</span></span>` +
    `<button class="profile-row-del">✕</button>`;
  list.appendChild(row);
});

// ---- settings panel ----
document.getElementById("settings-panel")!.innerHTML = `
  <div class="set-title">Settings</div>
  <div class="set-rows">
    ${["Engine active", "Dark mode", "Sound ticks"].map((l, i) => `
      <label class="set-row" style="animation-delay:${60 + i * 45}ms">
        <span class="set-row-label">${l}</span>
        <span class="toggle-switch"><input type="checkbox" ${i === 0 ? "checked" : ""}/>
        <span class="toggle-track"><span class="toggle-thumb"></span></span></span>
      </label>`).join("")}
  </div>
  <div class="divider" style="margin:14px 0 10px;"></div>
  <div class="set-row" style="flex-direction:column; align-items:stretch; gap:4px; margin-bottom:10px;">
    <div style="display:flex; align-items:baseline; gap:8px;">
      <span class="set-row-label">Rollover window</span>
      <span style="font-size:11px; font-weight:700; color:var(--st-accent-deep);">50ms</span>
    </div>
    <input type="range" min="10" max="150" value="50" style="width:100%; accent-color:var(--st-accent);" />
  </div>
  <div class="set-actions">
    <button class="btn">Reset to defaults</button>
    <button class="btn btn-danger">Clear all</button>
  </div>`;

// ---- key editor ----
document.getElementById("key-detail-panel")!.innerHTML = `
  <div class="ed-head">
    <span class="ed-cap">C</span>
    <span class="ed-title-wrap">
      <span class="ed-title">Space + C</span>
      <span class="ed-sub">Bound to Chrome</span>
    </span>
    <button class="ed-close">✕</button>
  </div>
  <input class="input" id="ed-search" placeholder="Search apps…" />
  <div class="ed-section">Apps on this device</div>
  <div id="ed-grid-scroll"><div id="ed-grid">
    ${["Chrome","Spotify","Figma","Terminal","VS Code","Discord","Notion","Mail","Slack","Steam","Excel","Word"]
      .map((n, i) => `
      <div class="ed-tile${i === 0 ? " current" : ""}" style="animation-delay:${100 + i * 22}ms">
        <span class="ed-tile-disc" style="background:${
          ["#c67139","#b08a3e","#a8552f","#8a6c4a","#c2884e","#6e3a15","#7a8a5e","#5f7052"][i % 8]
        }">${n[0]}</span>
        <span class="ed-tile-name">${n}</span>
      </div>`).join("")}
  </div></div>
  <div class="ed-row">
    <button class="btn ed-browse">Browse files…</button>
    <div class="ed-drop-hint">or drag an .exe / URL onto the key</div>
  </div>
  <div class="ed-row-tight">
    <input class="input" id="ed-path" placeholder="…or paste a file path / URL" />
    <button class="btn btn-primary" id="ed-assign" disabled>Assign</button>
  </div>
  <div class="ed-foot">
    <button class="btn btn-danger">Remove binding</button>
    <button class="btn btn-primary">Done</button>
  </div>`;

// ---- open whichever surface was asked for ----
const show = (id: string, expandBtn?: string) => {
  (document.getElementById(id) as HTMLElement).hidden = false;
  if (expandBtn) document.getElementById(expandBtn)!.setAttribute("aria-expanded", "true");
};
if (q.has("profiles")) show("profile-popover", "profile-pill");
if (q.has("gear")) show("settings-panel", "gear-btn");
if (q.has("specials")) show("specials-tray", "specials-btn");
if (q.has("editor")) {
  const p = document.getElementById("key-detail-panel")!;
  p.style.setProperty("--fx", "-300px");
  p.style.setProperty("--fy", "40px");
  p.hidden = false;
  p.classList.add("open");
  const b = document.getElementById("editor-backdrop")!;
  b.hidden = false;
  b.classList.add("shown");
  document.getElementById("stage")!.classList.add("editing");
}

// ---- cursor glow ----
const stage = document.getElementById("stage")!;
const glow = document.getElementById("cursor-glow")!;
let tx = stage.clientWidth / 2, ty = stage.clientHeight / 2, gx = tx, gy = ty;
stage.addEventListener("mousemove", (e) => {
  const r = stage.getBoundingClientRect();
  tx = e.clientX - r.left; ty = e.clientY - r.top;
  glow.style.opacity = "1";
});
const loop = () => {
  gx += (tx - gx) * 0.09; gy += (ty - gy) * 0.09;
  glow.style.transform = `translate(${gx - 190}px, ${gy - 190}px)`;
  requestAnimationFrame(loop);
};
requestAnimationFrame(loop);
