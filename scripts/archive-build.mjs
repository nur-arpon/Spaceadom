/**
 * archive-build.mjs — copy the installers just built into `all-versions/`,
 * and refresh `share-spaceadom/` to the same version.
 *
 * WHY THIS EXISTS. Both of those were manual commands I ran after each build,
 * and on 2026-08-20 they were skipped for five consecutive versions — because
 * the build cycle sped up during the storm iterations, which is exactly when
 * an easily-forgotten step gets forgotten. The result: `all-versions/` stopped
 * at 1.0.65 while its own header promised "every installer ever built lives
 * here", and friends were being handed a share folder five versions behind.
 *
 * A step that must happen after every build belongs in the build.
 *
 * Wired as `afterBundleCommand` in tauri.conf.json, so it runs on
 * `npm run tauri build` and cannot be forgotten. Never fails the build: a
 * broken archive step must not cost a working installer, so every problem is
 * reported and swallowed.
 */
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, unlinkSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const say = (m) => console.log(`archive-build: ${m}`);

try {
  const version = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version;
  const nsis = join(ROOT, "src-tauri/target/release/bundle/nsis", `Spaceadom_${version}_x64-setup.exe`);
  const msi  = join(ROOT, "src-tauri/target/release/bundle/msi",  `Spaceadom_${version}_x64_en-US.msi`);

  const archive = join(ROOT, "all-versions");
  const share = join(ROOT, "share-spaceadom");
  mkdirSync(archive, { recursive: true });
  mkdirSync(share, { recursive: true });

  // No installers for this version means this was `tauri dev`, or a build that
  // failed. Either way there is nothing to archive and nothing to publish —
  // and touching the share folder in that state is how it ends up empty.
  if (![nsis, msi].some(existsSync)) {
    say(`no installers for ${version} — nothing to do`);
    process.exit(0);
  }

  let kept = 0;
  for (const src of [nsis, msi]) {
    if (!existsSync(src)) { say(`MISSING, not archived: ${src}`); continue; }
    copyFileSync(src, join(archive, src.split(/[\\/]/).pop()));
    kept++;
  }
  say(`archived ${kept} installer(s) for ${version}`);

  // The share folder holds exactly ONE version — the current one. Older
  // installers left behind are how someone ends up sending a friend a build
  // from five versions ago.
  for (const f of readdirSync(share)) {
    if (/\.(exe|msi)$/i.test(f) && !f.includes(version)) {
      unlinkSync(join(share, f));
      say(`removed stale ${f} from share-spaceadom`);
    }
  }
  for (const src of [nsis, msi]) {
    if (existsSync(src)) copyFileSync(src, join(share, src.split(/[\\/]/).pop()));
  }

  // PRIVACY.md is referenced by the share README, so it has to travel with it.
  const privacy = join(ROOT, "PRIVACY.md");
  if (existsSync(privacy)) copyFileSync(privacy, join(share, "PRIVACY.md"));

  // Loud, checkable warnings — never a build failure.
  const readme = join(share, "READ-ME-FIRST.txt");
  if (existsSync(readme) && !readFileSync(readme, "utf8").includes(version)) {
    say(`WARNING: share-spaceadom/READ-ME-FIRST.txt does not mention ${version} — update it before sharing.`);
  }
  const changelog = join(ROOT, "all-versions/WHAT-CHANGED.md");
  if (existsSync(changelog) && !readFileSync(changelog, "utf8").includes(`**${version}**`)) {
    say(`WARNING: all-versions/WHAT-CHANGED.md has no row for ${version} — add one before sharing.`);
  }
  say(`share-spaceadom is now ${version}`);
} catch (e) {
  say(`FAILED (build is unaffected): ${e?.message ?? e}`);
}
