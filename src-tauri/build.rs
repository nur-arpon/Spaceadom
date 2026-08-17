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
        return;
    }

    #[cfg(not(target_os = "windows"))]
    tauri_build::build()
}
