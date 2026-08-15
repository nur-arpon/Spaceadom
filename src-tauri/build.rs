fn main() {
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
