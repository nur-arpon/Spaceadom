/**
 * label-store-build.mjs — rename the Store installer so it cannot be confused
 * with the one you hand to friends.
 *
 * WHY. `npm run store` writes to the SAME path as `npm run tauri build`:
 * `bundle/nsis/Spaceadom_<v>_x64-setup.exe`. After a Store build that file is
 * ~210 MB (it embeds the whole WebView2 runtime) instead of ~5.6 MB, with an
 * identical name. Two failure modes follow, and both are silent:
 *
 *   - `scripts/install-real.cmd` installs whatever is at that path, so a local
 *     install after a Store build quietly installs the 210 MB variant.
 *   - The 210 MB file gets copied into `share-spaceadom/` or attached to a
 *     GitHub release, and friends download 210 MB for no reason.
 *
 * So the Store output is renamed to `…-setup-STORE.exe` and the normal path is
 * left empty, which makes the next `tauri build` the only way to get a friend
 * installer back — an obvious failure instead of a silent substitution.
 *
 * Wired as npm's `poststore`.
 */
import { existsSync, readFileSync, renameSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const say = (m) => console.log(`label-store-build: ${m}`);

try {
  const version = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version;
  const dir = join(ROOT, "src-tauri/target/release/bundle/nsis");
  const from = join(dir, `Spaceadom_${version}_x64-setup.exe`);
  const to = join(dir, `Spaceadom_${version}_x64-setup-STORE.exe`);

  if (!existsSync(from)) { say(`no installer at ${from} — nothing to label`); process.exit(0); }

  const mb = statSync(from).size / (1024 * 1024);
  // A Store build must be big, because the runtime is inside it. If this is
  // small, the offline config did not apply and the file would be REJECTED by
  // the Store as a downloader stub — say so rather than labelling it anyway.
  if (mb < 100) {
    say(`REFUSING to label: ${mb.toFixed(1)} MB is far too small for an offline-installer build.`);
    say(`webviewInstallMode did not apply — check src-tauri/tauri.store.conf.json.`);
    process.exit(0);
  }

  renameSync(from, to);
  say(`Store installer is ${to} (${mb.toFixed(1)} MB, WebView2 embedded).`);
  say(`The normal path is now EMPTY — run 'npm run tauri build' before installing locally.`);
} catch (e) {
  say(`FAILED (the installer is still there, just unlabelled): ${e?.message ?? e}`);
}
