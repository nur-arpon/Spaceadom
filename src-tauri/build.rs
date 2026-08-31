fn main() {
    // PROBLEM 131 — guarantee `symbols/spaceadom.pdb` exists before
    // tauri-build validates `bundle.resources`, which it does BELOW, during
    // compilation — long before the linker has produced the real pdb.
    //
    // This lives in build.rs rather than in `beforeBuildCommand` for a reason
    // found by breaking it: Tauri's before-hooks run only for `tauri build`,
    // so with the staging in the config, a plain `cargo test` or `cargo build`
    // died with `resource path symbols\spaceadom.pdb doesn't exist`. build.rs
    // runs for every cargo invocation, so the path is always there.
    //
    // It writes a STUB and never a pdb. Staging a previous build's symbols
    // would also satisfy the check and is far worse than shipping none:
    // mismatched symbols do not fail, they resolve to confidently wrong
    // function names and line numbers, and someone will act on that. The real
    // pdb is copied in by `beforeBundleCommand` after linking and immediately
    // before packaging — see scripts/stage-symbols.mjs.
    #[cfg(target_os = "windows")]
    {
        let staged = std::path::Path::new("symbols/spaceadom.pdb");
        if !staged.exists() {
            let _ = std::fs::create_dir_all("symbols");
            let _ = std::fs::write(
                staged,
                "NOT A REAL PDB - placeholder written by build.rs so Tauri's resource \
                 check passes during compilation. scripts/stage-symbols.mjs --real \
                 replaces it after linking. If you are reading this inside an INSTALLED \
                 copy of Spaceadom, the bundle step did not run and crash backtraces \
                 will be unsymbolised.\r\n",
            );
        }
    }

    // Link GDI32 and User32 for Win32 icon extraction APIs
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-lib=dylib=user32");
        println!("cargo:rustc-link-lib=dylib=gdi32");
        println!("cargo:rustc-link-lib=dylib=dwmapi");
        println!("cargo:rustc-link-lib=dylib=shell32");
    }

    // PROBLEMS 61 + 62 — ship an explicit application manifest.
    // Without it the process is DPI-unaware (Windows feeds it virtualised
    // coordinates on scaled displays) and inherits whatever execution level
    // the launcher had. See windows-app-manifest.xml for the reasoning.
    #[cfg(target_os = "windows")]
    {
        let windows = tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("windows-app-manifest.xml"));
        tauri_build::try_build(
            tauri_build::Attributes::new().windows_attributes(windows),
        )
        .expect("failed to run tauri-build with the app manifest");

        // PROBLEM 226 — `cargo test --lib` binaries link WITHOUT the manifest
        // above, so they load comctl32 v5, are missing `TaskDialogIndirect`,
        // and die `0xC0000139`/`0xC0000138` at PROCESS STARTUP (Windows
        // resolves the whole import table before `main` runs) — or, off this
        // machine, pop an Entry-Point-Not-Found modal that HANGS the test
        // run. Documented in PROJECT_STATUS.md 2026-08-29 (Opus 5, PiP
        // session) but left unfixed there ("Your call."). This is that call
        // — and the one-line fix that comment proposed (mirror `-bins` with
        // `cargo:rustc-link-arg-tests=`) turned out not to exist for this
        // crate. Recorded here so nobody re-tries it:
        //
        //   1. `cargo:rustc-link-arg-tests=` is REJECTED outright —
        //      `error: invalid instruction ... does not have a test target`
        //      — because this crate's 239 tests are all `#[cfg(test)]` unit
        //      tests run via `--lib`, and Cargo's "-tests" scoping targets
        //      only `Test`-kind targets (files under `tests/`), which this
        //      crate has none of. Proved both on this crate and on a
        //      from-scratch repro with only a `#[lib]`+`#[test]`: identical
        //      error either way.
        //   2. The bare, un-suffixed `cargo:rustc-link-arg=` DOES reach the
        //      lib's own `--test` harness (proved with `-vv`: the flag shows
        //      up on that link line) — but it ALSO reaches `bins`, on top of
        //      the manifest `-bins` above already supplies. Reproduced: two
        //      copies of the SAME resource file on ONE link line →
        //      `CVTRES fatal error CVT1100: duplicate resource, type:
        //      MANIFEST, name:1` → `LNK1123`. That is not a test-only
        //      failure, it is `spaceadom.exe` itself failing to link —
        //      unshippable, so this path is out.
        //
        // What actually works, proved end-to-end below: don't put a second
        // manifest anywhere. Delay-load comctl32.dll instead, so the loader
        // never resolves `TaskDialogIndirect` (or anything else from it) at
        // PROCESS STARTUP — only on first actual call. None of the 239 tests
        // call it, so the import is simply never touched and the harness
        // starts clean. `/DELAYLOAD` is a linker flag, not a resource, so —
        // unlike the manifest — applying it to `bins` too is harmless: a
        // fresh scratch repro built a bin carrying the real manifest (via
        // `-bins`, unmodified) PLUS this delay-load pair and linked clean,
        // then a lib built the same way ran its unit tests clean. Confirmed
        // even the pathological case is safe: forcing the delay-loaded call
        // to actually run (a test that is never otherwise exercised) exits
        // immediately via the delay-load failure handler (`0xC06D007F`) —
        // a clean process exit, never a hang, never a modal.
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
        println!("cargo:rustc-link-arg=delayimp.lib");
        // The cfg block below is the non-Windows path; on Windows this arm is
        // the whole function, so the early return is redundant (clippy).
    }

    #[cfg(not(target_os = "windows"))]
    tauri_build::build()
}
