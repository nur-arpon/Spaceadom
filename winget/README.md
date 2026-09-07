# winget manifests (prepared, not submitted)

This folder holds a **winget package manifest**, laid out exactly the way
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) expects it
(`manifests/<first-letter>/<Publisher>/<Package>/<version>/`), for
`NurArpon.Spaceadom`. `winget validate` on this folder passes (checked
2026-09-05, winget v1.29.290, schema 1.9.0).

**Nothing here has been submitted to winget-pkgs.** That is a deliberate,
separate, owner-approved step — see below.

## What's in `1.0.100/`

- `NurArpon.Spaceadom.yaml` — the version manifest.
- `NurArpon.Spaceadom.installer.yaml` — installer type `nullsoft` (NSIS),
  `Scope: user`, silent switch `/S`, pointed at the `setup.exe` asset on this
  project's own GitHub Releases.
- `NurArpon.Spaceadom.locale.en-US.yaml` — the listing text, licence and
  privacy URLs, tags.

The installer manifest's `InstallerSha256` is a **placeholder** (all zeros).
It is not valid and must be replaced with the real hash of a real published
`setup.exe` before this manifest means anything.

## Generating a manifest for a new release

```bash
node scripts/winget-manifest.mjs v1.0.101
```

This downloads that release's `setup.exe` via `gh release download`,
computes its real SHA-256, and writes a new versioned folder under
`winget/manifests/n/NurArpon/Spaceadom/<version>/`, using the `1.0.100`
manifests here as the template (so hand-written description/tag edits carry
forward). It requires `gh` to be authenticated and the release to already be
published with its `setup.exe` asset — this is a **post-release** step, not
part of the release itself.

Validate what it wrote:

```bash
winget validate "winget/manifests/n/NurArpon/Spaceadom/<version>"
```

## Submitting to winget-pkgs (when the owner decides to)

This is **not done by anything in this repo**. When ready:

1. Fork [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs).
2. Either copy the generated folder into that fork at the same path
   (`manifests/n/NurArpon/Spaceadom/<version>/`) and open a PR by hand, or
   use [`wingetcreate`](https://github.com/microsoft/winget-create)
   (`wingetcreate update NurArpon.Spaceadom --submit --version <version> --urls <setup.exe URL>`)
   which does the fork/branch/PR for you against a real, already-published
   release asset.
3. winget-pkgs runs its own automated validation and a moderator review
   before merging — expect requested changes are normal, not a rejection.

Only ever submit a manifest pointing at a release that is actually public.
The auto-updater in this app (`src-tauri/src/updater.rs`) and winget are
independent distribution channels; neither depends on the other.
