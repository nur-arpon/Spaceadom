// Put spaceadom.pdb into the installer, next to the exe (PROBLEM 131).
//
// WHY: all 14 recorded crashes printed `0: <unknown>` for every backtrace
// frame. The panic handler was fine; it simply had no symbols to resolve
// against, because the pdb is built into target/release and the installer
// shipped only the .exe. dbghelp looks for the pdb beside the executable, so
// putting it there makes those frames resolve on the user's own machine.
//
// WHY THIS NEEDS A SCRIPT AT ALL — an ordering trap worth understanding before
// "simplifying" it away:
//
//   * Tauri resolves `bundle.resources` while the RUST CRATE COMPILES
//     (generate_context!), which is BEFORE the linker has produced the pdb. So
//     a resource pointing straight at target/release/spaceadom.pdb breaks any
//     clean build — including every CI build, which is the one that matters.
//
//   * Staging the PREVIOUS build's pdb early would satisfy that check and is
//     far worse than shipping nothing: mismatched symbols do not fail, they
//     resolve to CONFIDENTLY WRONG function names and line numbers. A crash
//     report that lies is worse than one that says <unknown>, because someone
//     will act on it.
//
// So: --placeholder runs before the compile and guarantees the path exists
// (writing an obviously-invalid stub, never a stale pdb), and --real runs
// after the link and before bundling, replacing it with the genuine article.
// If the real pdb is missing at that point the build FAILS rather than
// quietly shipping the stub.

import { existsSync, mkdirSync, copyFileSync, writeFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const BUILT = join(ROOT, "src-tauri", "target", "release", "spaceadom.pdb");
const STAGED = join(ROOT, "src-tauri", "symbols", "spaceadom.pdb");

const STUB =
  "NOT A REAL PDB.\r\n" +
  "This placeholder only exists so Tauri's resource check passes during " +
  "compilation. scripts/stage-symbols.mjs --real replaces it with the genuine " +
  "spaceadom.pdb after linking and before bundling. If you are reading this " +
  "inside an INSTALLED copy of Spaceadom, the bundle step did not run and " +
  "crash backtraces will be unsymbolised.\r\n";

const mode = process.argv[2];
mkdirSync(dirname(STAGED), { recursive: true });

if (mode === "--placeholder") {
  // Never leave a previous build's pdb here: it would ship and mislead.
  writeFileSync(STAGED, STUB);
  console.log("stage-symbols: placeholder written (real pdb comes after linking)");
} else if (mode === "--real") {
  if (!existsSync(BUILT)) {
    console.error(
      `stage-symbols: FATAL — no pdb at ${BUILT}\n` +
        "The release profile must emit debug info. Refusing to bundle, because " +
        "the alternative is shipping the placeholder and silently losing every " +
        "future crash report."
    );
    process.exit(1);
  }
  copyFileSync(BUILT, STAGED);
  const mb = (statSync(STAGED).size / 1024 / 1024).toFixed(1);
  console.log(`stage-symbols: real pdb staged (${mb} MB) — backtraces will resolve`);
} else {
  console.error("usage: stage-symbols.mjs --placeholder | --real");
  process.exit(1);
}
