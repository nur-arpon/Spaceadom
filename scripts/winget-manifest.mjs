#!/usr/bin/env node
/**
 * winget-manifest.mjs — build a winget manifest folder for one release tag.
 *
 * Usage:
 *   node scripts/winget-manifest.mjs v1.0.101
 *   node scripts/winget-manifest.mjs 1.0.101      (the leading "v" is optional)
 *
 * What it does, in order:
 *   1. `gh release download <tag>` — pulls the NSIS setup.exe asset for that
 *      tag into a scratch directory (requires `gh` to be authenticated;
 *      this repo's CLAUDE.md and release.yml already assume that).
 *   2. Computes its SHA-256.
 *   3. Writes winget/manifests/n/NurArpon/Spaceadom/<version>/ with the three
 *      manifests (version, installer, defaultLocale en-US), using the
 *      existing 1.0.100 manifests in this repo as the template so hand-edits
 *      to description/tags/etc. carry forward version to version.
 *
 * This does NOT submit anything. Submitting a new/updated package to winget
 * is a pull request against https://github.com/microsoft/winget-pkgs — see
 * the README this script sits beside, or `wingetcreate submit` /
 * `winget-pkgs`'s own CONTRIBUTING.md — and that step is only ever done
 * after a real GitHub release exists with a real, signed setup.exe attached.
 * Owner decision; not automated here on purpose.
 */

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const REPO = "nur-arpon/Spaceadom";
const PACKAGE_ID = "NurArpon.Spaceadom";
const PUBLISHER_SEGMENT = "n"; // winget-pkgs layout: first letter, lowercased, of the publisher segment

function fail(msg) {
  console.error(`error: ${msg}`);
  process.exit(1);
}

const rawArg = process.argv[2];
if (!rawArg) {
  fail("usage: node scripts/winget-manifest.mjs <tag-or-version>  (e.g. v1.0.101 or 1.0.101)");
}
const tag = rawArg.startsWith("v") ? rawArg : `v${rawArg}`;
const version = rawArg.startsWith("v") ? rawArg.slice(1) : rawArg;
const assetName = `Spaceadom_${version}_x64-setup.exe`;

console.log(`Building winget manifest for ${PACKAGE_ID} ${version} (release ${tag})`);

// --- 1. Download the release asset via gh ---------------------------------
const scratch = mkdtempSync(join(tmpdir(), "spaceadom-winget-"));
try {
  console.log(`Downloading ${assetName} from release ${tag}...`);
  execFileSync(
    "gh",
    ["release", "download", tag, "--repo", REPO, "--pattern", assetName, "--dir", scratch, "--clobber"],
    { stdio: "inherit" }
  );
} catch (e) {
  rmSync(scratch, { recursive: true, force: true });
  fail(`gh release download failed — is release ${tag} published with asset ${assetName}? (${e.message})`);
}

const assetPath = join(scratch, assetName);
if (!existsSync(assetPath)) {
  rmSync(scratch, { recursive: true, force: true });
  fail(`expected asset not found after download: ${assetPath}`);
}

// --- 2. Compute SHA-256 -----------------------------------------------------
const hash = createHash("sha256");
hash.update(readFileSync(assetPath));
const sha256 = hash.digest("hex");
console.log(`SHA-256: ${sha256}`);
rmSync(scratch, { recursive: true, force: true });

// --- 3. Write the versioned manifest folder --------------------------------
const repoRoot = join(import.meta.dirname, "..");
const manifestsRoot = join(repoRoot, "winget", "manifests", PUBLISHER_SEGMENT, "NurArpon", "Spaceadom");
const templateDir = join(manifestsRoot, "1.0.100"); // the hand-authored template
const outDir = join(manifestsRoot, version);

if (!existsSync(templateDir)) {
  fail(`template manifest folder missing: ${templateDir} — expected the 1.0.100 manifests to exist as a template`);
}

mkdirSync(outDir, { recursive: true });

function rewriteVersion(text) {
  return text
    .replaceAll("PackageVersion: 1.0.100", `PackageVersion: ${version}`)
    .replaceAll("/v1.0.100/", `/${tag}/`)
    .replaceAll("Spaceadom_1.0.100_x64-setup.exe", assetName);
}

for (const file of readdirSync(templateDir)) {
  if (!file.endsWith(".yaml")) continue;
  const src = readFileSync(join(templateDir, file), "utf8");
  let out = rewriteVersion(src);
  if (file.endsWith(".installer.yaml")) {
    out = out.replace(
      /InstallerSha256: "0+"/,
      `InstallerSha256: "${sha256}"`
    );
    // In case the template's placeholder sha differs from the all-zero one
    // above (e.g. it's already been filled from a previous run), replace any
    // 64-hex-char value on the InstallerSha256 line unconditionally.
    out = out.replace(
      /InstallerSha256: "[0-9a-fA-F]{64}"/,
      `InstallerSha256: "${sha256}"`
    );
  }
  writeFileSync(join(outDir, file.replace("1.0.100", version).replace(/^NurArpon\.Spaceadom/, "NurArpon.Spaceadom")), out);
}

console.log(`Wrote manifests to ${outDir}`);
console.log("Next steps (manual, owner decision):");
console.log("  1. Review the generated YAML — especially ReleaseNotesUrl / description drift from the template.");
console.log("  2. If winget is installed locally: winget validate \"" + outDir + "\"");
console.log("  3. Submit via a PR to https://github.com/microsoft/winget-pkgs (wingetcreate or a manual fork+PR),");
console.log("     only once this exact release is public and this SHA-256 is final.");
