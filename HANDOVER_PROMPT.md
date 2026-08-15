# MISSION: ARCHITECTURAL FINALIZATION (V12 DEPLOYMENT)

**[CRITICAL INSTRUCTION]:** You are taking over a high-stakes Rust/Tauri development project. Do not start by guessing; start by "downloading" the current reality from the project logs.

## 1. THE KNOWLEDGE BASE (Read in this order)

1. **`install-v11.ps1` (Attached):** This is the **Functional Bible**. It defines the "V11 Parity" behavior we are recreating. Use this as your reference for how the user expects the app to feel, animate, and behave.
2. **`PROJECT_STATUS.md`:** This is the **State of the Union**. It lists what is currently working, what is buggy, and what has been done today.
3. **`walkthrough.md`:** This is the **Dev Diary**. It contains the context of how we solved the `npm`/`fnm` path issues and the keyboard hook threading.

## 2. YOUR RESPONSIBILITY

* **NO DELETIONS:** Never prune the history in `PROJECT_STATUS.md`. Always append new updates to the top.
* **ENVIRONMENT AWARENESS:** The local machine uses `fnm` (Fast Node Manager). If you get a "command not found" for `npm` or `node`, run the path-injection script (saved in `walkthrough.md`) before attempting any builds.
* **THE "GHOST" RULE:** If you hit port conflicts (e.g., Port 1420), assume a zombie process exists. Always check for and terminate rogue `node` or `vite` processes before declaring an error.

## 3. IMMEDIATE OBJECTIVE: FINAL PACKAGING

1. **Audit & Cleanup:** Run `cargo check`. If there are `dead_code` or `unused_must_use` warnings, fix them to ensure a clean build.
2. **Verify & Test:** Ensure the `SmartCascade` (App summoning/banishing) and `HUD` trigger (300ms hold) work exactly as they did in the `install-v11.ps1` script.
3. **Production Build:**
   * Execute `cargo tauri build`.
   * Verify the successful generation of the MSI installer.
   * Provide the user with the final absolute path to the `.msi` file.
   * Generate a `FINAL_RELEASE_README.md` summarizing the V12 parity features.

## 4. MISSION SUCCESS CRITERIA

* The user should be able to run the generated MSI and experience the "V11 Parity" (Modifier Gate, HUD, Audio Gating, PiP) without the source code terminal open.
* Do not leave the user with a "dev" environment; leave them with a finished, shippable installer.
