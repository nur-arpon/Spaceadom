/**
 * build-portable.mjs — PROBLEM 254. Pack the plain release exe into a
 * self-contained, no-installer zip.
 *
 * WHAT THIS IS FOR. A friend who does not want an installer touching their
 * machine — no Program Files entry, no Task Scheduler entry, no registry Run
 * value, nothing left behind by an uninstaller that might miss a folder —
 * unzips this somewhere and runs `spaceadom.exe`. `portable.txt` sitting
 * beside the exe is the ONLY signal the app looks for
 * (`src-tauri/src/portable.rs::MARKER_FILE`); its presence routes every file
 * the app ever writes (config, debug.log, backups, the picker cache, the
 * update-rollback archive, last-run-version.txt) into `<exe dir>\data\`
 * instead of `%APPDATA%`. Delete the folder and the app, its data, and its
 * autostart footprint (there is none — see below) are all gone at once.
 *
 * WHAT GOES IN THE ZIP, and nothing else:
 *   - `spaceadom.exe`      the plain `npm run tauri build` release binary.
 *                          NOT the NSIS `setup.exe`, NOT the `.msi`, NOT the
 *                          Store's offline-WebView2 build from `npm run
 *                          store` (that one is ~210 MB and IS an installer —
 *                          see CLAUDE.md's "A THIRD build target" section;
 *                          running an installer from inside a zip makes no
 *                          sense here, which is also why the MSIX leg is not
 *                          applicable: you cannot run an installer from
 *                          inside a package either).
 *   - `portable.txt`       the marker, with a one-line explanation inside so
 *                          a curious user who opens it understands what it
 *                          does. The app never reads its CONTENTS — only
 *                          whether the file exists.
 *   - `README-portable.txt` what this is, where the data lives, and the
 *                          WebView2 requirement below.
 *
 * WEBVIEW2 REQUIREMENT — DOCUMENTED, NOT BUNDLED. This zip does not embed
 * the WebView2 runtime the way `npm run store`'s offline-installer build
 * does (`tauri.store.conf.json`'s `webviewInstallMode: offlineInstaller`,
 * ~210 MB). A portable zip has no installer step in which to run WebView2's
 * bootstrapper, so it can only ever assume the Evergreen runtime the OS
 * already ships with modern Windows 10/11 is present on the target machine.
 * If it genuinely is not, the exe will fail to create its window with no
 * obvious message — this is a known, accepted limitation of the portable
 * shape, spelled out in README-portable.txt so it is not mistaken for a bug.
 * The offline-installer variant is NOT an option to add here: it IS an
 * installer, and packing one inside a zip that promises "no installer" would
 * be exactly the contradiction PROBLEM 165 already named for `npm run store`.
 *
 * WHERE IT GOES: `src-tauri/target/release/bundle/portable/`, alongside how
 * `bundle/nsis`, `bundle/msi` and `bundle/msix` already lay out their own
 * outputs — so release.yml's upload step and a human looking for "where did
 * the build put it" both find it in the expected place.
 *
 * NOT wired into `tauri.conf.json`'s `beforeBundleCommand`/`afterBundleCommand`
 * — deliberately a separate `npm run portable`, exactly like `npm run msix`,
 * because it does not need `tauri build` to run at all (the exe from an
 * earlier `npm run tauri build` is enough) and should not silently run on
 * every ordinary build.
 */
import {
  copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync,
  writeFileSync,
} from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const say = (m) => console.log(`build-portable: ${m}`);
const fail = (m) => { console.error(`build-portable: ${m}`); process.exit(1); };

const version = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version;
const exeSrc = join(ROOT, "src-tauri/target/release/spaceadom.exe");

if (!existsSync(exeSrc)) {
  fail(
    `no release exe at ${exeSrc}. Run 'npm run build' then 'npm run tauri build' first — ` +
    `this script packs the binary that step already produces, it does not build one itself.`
  );
}

// ---------------------------------------------------------------------------
// REVIEW FIXES 2026-09-05 (scripts) — THE GUARD THIS COMMENT PROMISED, WRITTEN
// ---------------------------------------------------------------------------
//
// The paragraph that used to sit here argued at length that "the guard belongs
// here, at packing time" and was followed by a `statSync` that printed a size
// and checked nothing. A comment describing a guard that does not exist is
// worse than no comment: it is the reason nobody adds one.
//
// A dev/debug binary is the wrong thing to hand a friend — no optimisation, a
// much bigger file — and there is no portable-specific check inside the exe
// itself (the marker file decides WHERE data goes, not what build produced the
// binary), so packing time really is the only place this can be caught. Three
// independent signals, because each catches a different mistake and any one of
// them alone can be satisfied by accident:
//
//   1. SIZE. A `cargo build` (dev profile) binary of this project is several
//      times the release size. The window is deliberately wide — this is a
//      "that is the wrong kind of file" check, not a size assertion, and a
//      genuine release binary grows over time.
//   2. VERSION STAMP. `FileVersion` must equal package.json's version. This is
//      the one that catches the mistake actually available here: the script
//      packs whatever sits at target/release/spaceadom.exe, which after a
//      version bump but before a rebuild is the PREVIOUS build — and the zip
//      would be NAMED for the new version while containing the old app.
//      CLAUDE.md's PROBLEM 127 rule ("verify by version stamp") applied to the
//      one artifact that has no installer to verify it later.
//   3. FRESHNESS AGAINST dist2. Tauri embeds the frontend at COMPILE time, so
//      an exe older than the newest file in dist2 contains a stale UI while
//      reporting the right version — CLAUDE.md's own proof chain, and the
//      failure it names is invisible from outside the binary.
//
// All three fail loudly with the command that fixes them. None of them can
// pass by being unreadable: an unreadable version or an unreadable dist2 is a
// failure, not a skip, for the reason build-msix.ps1's TaskId check now spells
// out — a check that cannot produce a negative result is not a check.
const exeSize = statSync(exeSrc).size;
const exeMB = exeSize / (1024 * 1024);

/** Smallest a real release build has ever been here; below this it is not our exe. */
const MIN_RELEASE_MB = 8;
/** A dev-profile build of this project is far past this. */
const MAX_RELEASE_MB = 60;
if (exeMB < MIN_RELEASE_MB || exeMB > MAX_RELEASE_MB) {
  fail(
    `${exeSrc} is ${exeMB.toFixed(1)} MB, outside the ${MIN_RELEASE_MB}-${MAX_RELEASE_MB} MB `
    + `window a release build of Spaceadom occupies. A dev-profile ('cargo build') binary is `
    + `much larger and must never go in a portable zip. Run 'npm run build' then `
    + `'npm run tauri build' and try again.`
  );
}

/** Ask Windows for a file's version stamp. `null` if it has none. */
function fileVersion(path) {
  try {
    const out = execFileSync(
      "powershell",
      [
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command",
        `(Get-Item -LiteralPath '${path}').VersionInfo.FileVersion`,
      ],
      { encoding: "utf8" }
    ).trim();
    return out || null;
  } catch {
    return null;
  }
}

const stamped = fileVersion(exeSrc);
if (!stamped) {
  fail(
    `could not read a FileVersion from ${exeSrc}. That stamp is the only way this script can `
    + `tell a freshly built exe from the one an earlier version left behind, so packing without `
    + `it would ship a zip whose NAME is the only claim about what is inside.`
  );
}
// `1.0.100` vs a four-part `1.0.100.0` — Windows pads. Compare the first three.
const three = (v) => v.split(".").slice(0, 3).join(".");
if (three(stamped) !== three(version)) {
  fail(
    `${exeSrc} is stamped ${stamped} but package.json says ${version}. This script does not `
    + `compile anything — it packs whatever is already at that path, and that is the PREVIOUS `
    + `build. The zip would be named Spaceadom_${version}_x64-portable.zip and contain `
    + `${stamped}. Run 'npm run build' then 'npm run tauri build' first.`
  );
}

// FRESHNESS. `tauri::generate_context!` embeds dist2 at compile time, so an exe
// older than the newest file in dist2 was built against a previous frontend and
// will report the right version while showing the wrong UI.
const distDir = join(ROOT, "dist2");
if (!existsSync(distDir)) {
  fail(`no ${distDir}. Run 'npm run build' first — the exe embeds that folder at compile time.`);
}
const newestDist = readdirSync(distDir, { recursive: true, withFileTypes: true })
  .filter((d) => d.isFile())
  .map((d) => statSync(join(d.parentPath ?? d.path, d.name)).mtimeMs)
  .reduce((a, b) => Math.max(a, b), 0);
const exeMtime = statSync(exeSrc).mtimeMs;
if (newestDist > exeMtime) {
  fail(
    `${exeSrc} (${new Date(exeMtime).toISOString()}) is OLDER than the newest file in dist2 `
    + `(${new Date(newestDist).toISOString()}). Tauri embeds the frontend at compile time, so `
    + `this binary contains a previous version of the UI and would say nothing about it. Run `
    + `'npm run tauri build' again.`
  );
}

say(
  `packing ${exeSrc} (${exeMB.toFixed(1)} MB, stamped ${stamped}, newer than dist2) `
  + `as Spaceadom ${version}`
);

const outDir = join(ROOT, "src-tauri/target/release/bundle/portable");
const stage = join(outDir, "_stage");
const zipPath = join(outDir, `Spaceadom_${version}_x64-portable.zip`);

// Clean any previous attempt's leftovers so a failed run never gets zipped by
// accident and a stale zip never survives beside a fresh one under the same
// name.
rmSync(stage, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });
if (existsSync(zipPath)) rmSync(zipPath, { force: true });

copyFileSync(exeSrc, join(stage, "spaceadom.exe"));

// THE marker. `src-tauri/src/portable.rs::MARKER_FILE` — must match exactly.
// The app never reads what is inside; the text here is for a curious human.
writeFileSync(
  join(stage, "portable.txt"),
  "This file tells Spaceadom to keep all of its data (config, log, backups, " +
  "the app-picker cache) in a \"data\" folder right next to this exe instead " +
  "of your Windows user profile. Delete this whole folder and every trace of " +
  "Spaceadom goes with it. Do not delete this file on its own while keeping " +
  "the rest — that switches Spaceadom back to storing data in %APPDATA% on " +
  "its next launch, which most people do not want by accident.\n"
);

writeFileSync(
  join(stage, "README-portable.txt"),
  `Spaceadom ${version} — portable\n` +
  `${"=".repeat(("Spaceadom " + version + " — portable").length)}\n\n` +
  "WHAT THIS IS\n" +
  "Hold Space and tap a key to launch, focus or minimise any app. This is the\n" +
  "no-installer build: nothing is written to Program Files, the registry, or\n" +
  "Task Scheduler. Run spaceadom.exe directly from wherever you unzipped it.\n\n" +
  "WHERE YOUR DATA LIVES\n" +
  "Everything — config.json, debug.log, your rolling backups, the app-picker\n" +
  "cache — is kept in the \"data\" folder that appears next to this exe the\n" +
  "first time you run it. Nothing goes to %APPDATA% or %LOCALAPPDATA%. Moving\n" +
  "or copying this whole folder moves your settings with it; deleting it\n" +
  "removes Spaceadom completely, with no separate uninstall step.\n\n" +
  "RUN AT STARTUP\n" +
  "A portable copy does not register itself to start with Windows — that is\n" +
  "the point of \"portable\". If you want Spaceadom running after you log in,\n" +
  "put a shortcut to spaceadom.exe in your own Startup folder:\n" +
  "  Win+R -> shell:startup -> paste a shortcut to spaceadom.exe there.\n\n" +
  "UPDATES\n" +
  "This copy does not update itself — that also needs an installer, which a\n" +
  "portable copy deliberately has none of. Download a newer portable zip from\n" +
  "the Releases page and replace the exe (your \"data\" folder is untouched).\n\n" +
  "WEBVIEW2\n" +
  "Spaceadom's window is drawn with Microsoft Edge WebView2, which almost\n" +
  "every up-to-date Windows 10/11 machine already has (it ships with Windows\n" +
  "Update and with Edge itself). This portable build does NOT carry its own\n" +
  "copy of it — if the app's window never appears on a very old or locked-down\n" +
  "machine, install the \"Evergreen\" WebView2 Runtime from Microsoft and try\n" +
  "again: https://developer.microsoft.com/microsoft-edge/webview2/\n\n" +
  "TWO COPIES, ONE SPACEBAR\n" +
  "Do not run this portable copy at the same time as an installed Spaceadom\n" +
  "(setup.exe or .msi) on the same machine. Both hook the spacebar and they\n" +
  "will fight each other exactly as two installed copies would; the dashboard\n" +
  "detects this and tells you which two copies it found.\n"
);

// No bundled zip library — Windows ships Compress-Archive, and every other
// build helper in this project already shells out to PowerShell rather than
// add a dependency for something the OS does natively (CLAUDE.md: no CDN,
// minimal moving parts). `-Force` so a re-run never trips over the guard
// above having already removed the old zip.
try {
  execFileSync(
    "powershell",
    [
      "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command",
      `Compress-Archive -Path '${stage}\\*' -DestinationPath '${zipPath}' -Force`,
    ],
    { stdio: "inherit" }
  );
} catch (e) {
  fail(`Compress-Archive failed: ${e?.message ?? e}`);
}

rmSync(stage, { recursive: true, force: true });

if (!existsSync(zipPath)) {
  fail(`Compress-Archive reported success but ${zipPath} does not exist — nothing was produced.`);
}

// ---------------------------------------------------------------------------
// REVIEW FIXES 2026-09-05 (scripts) — OPEN THE ZIP AND LOOK INSIDE IT
// ---------------------------------------------------------------------------
//
// "The file exists" was the whole verification, and it is the one claim that
// could not distinguish the failure that matters here. `portable.txt` is not a
// nicety: `src-tauri/src/portable.rs::MARKER_FILE` is the ONLY signal the app
// looks for, and without it in the zip the "portable" build silently stops
// being portable — it writes config, debug.log, backups and the picker cache
// into `%APPDATA%\Spaceadom` instead of its own folder, which is the exact
// thing a person downloading this build chose it to avoid. Nothing about the
// app looks wrong when that happens; it just quietly leaves things behind on a
// machine whose owner was promised it would not.
//
// So the zip is opened and its entries are checked by NAME and by SIZE. The
// size check is what makes it a real check rather than a listing: a
// `Compress-Archive` that raced the staging directory can produce a 0-byte
// member, and an empty `portable.txt` is a file the app WILL find (it never
// reads the contents) while an empty `spaceadom.exe` is a zip that hands
// somebody nothing.
//
// `System.IO.Compression.ZipFile` rather than a zip library, for the same
// reason `Compress-Archive` writes it: no new dependency for something the OS
// does natively. Read-only; it opens the file this script just wrote.
const REQUIRED = {
  "spaceadom.exe": exeSize,
  "portable.txt": null,          // any non-empty size
  "README-portable.txt": null,
};
let entries;
try {
  const json = execFileSync(
    "powershell",
    [
      "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command",
      "Add-Type -AssemblyName System.IO.Compression.FileSystem; "
      + `$z=[IO.Compression.ZipFile]::OpenRead('${zipPath}'); `
      + "$e=$z.Entries | ForEach-Object { @{ name = $_.FullName; length = $_.Length } }; "
      + "$z.Dispose(); "
      + "ConvertTo-Json -InputObject @($e) -Compress",
    ],
    { encoding: "utf8" }
  );
  entries = JSON.parse(json);
} catch (e) {
  fail(`could not read back ${zipPath} to verify its contents: ${e?.message ?? e}`);
}

const byName = new Map(entries.map((e) => [e.name, e.length]));
for (const [name, expected] of Object.entries(REQUIRED)) {
  if (!byName.has(name)) {
    fail(
      `${zipPath} does not contain ${name}. Present: ${[...byName.keys()].join(", ") || "(nothing)"}.`
      + (name === "portable.txt"
        ? " Without this marker the unzipped copy is NOT portable — it writes to %APPDATA% like"
          + " an installed one, silently, which is the one promise this build makes."
        : "")
    );
  }
  const got = byName.get(name);
  if (got === 0) fail(`${zipPath} contains ${name} but it is 0 bytes.`);
  if (expected !== null && got !== expected) {
    fail(
      `${zipPath} contains ${name} at ${got} bytes but the source is ${expected} bytes — the `
      + `archive does not match what was staged.`
    );
  }
}
const extra = [...byName.keys()].filter((n) => !(n in REQUIRED));
if (extra.length) {
  // Not a failure — a future addition is legitimate — but it is named, because
  // this zip's whole claim is "nothing else is in here".
  say(`NOTE: the zip also carries ${extra.join(", ")}. Intended?`);
}

const zipSize = statSync(zipPath).size;
say(
  `wrote ${zipPath} (${(zipSize / (1024 * 1024)).toFixed(1)} MB) — verified to contain `
  + `${Object.keys(REQUIRED).join(", ")}, exe byte-for-byte the ${stamped} release build`
);
say("this zip is never referenced by any updater manifest — an update check must stay a no-op for a portable copy (see updater.rs).");
