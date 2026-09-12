/// hook/orbit_apps.rs — THE BUILT-IN MIDDLE-BUTTON EXCLUSION LIST.
///
/// PROBLEM 263. Holding the middle mouse button raises the Guide HUD ring, the
/// same ring holding Space raises. In a 3D or CAD program that gesture is
/// already spoken for: middle-drag ORBITS the model in SolidWorks, Fusion 360,
/// Blender, Maya, AutoCAD and every one of their neighbours, and middle-drag
/// PANS the canvas in the 2D design tools underneath them. Swallowing the
/// middle button there would not add a feature, it would delete somebody's
/// day's work-flow.
///
/// So this table exists, and three things about it are deliberate:
///
///   1. **IT IS SEPARATE FROM THE USER'S APP EXCEPTIONS, AND IT APPLIES TO THE
///      MIDDLE-BUTTON TRIGGER ONLY.** `hook/exclusions.rs` stands the WHOLE app
///      down inside an app the user listed — Space included. This one does
///      nothing of the kind: inside SolidWorks the Space shortcuts, the ring,
///      Space+letter and every special key keep working exactly as they do
///      everywhere else. All that changes is that the middle button is handed
///      straight back to Windows. Merging the two lists would silently delete
///      a user's Space shortcuts in twenty programs.
///   2. **IT IS EXACT-STEM MATCHING**, through `exclusions::normalize_stem`, so
///      the one normalisation rule in this codebase covers full paths, bare
///      file names and hand-written entries alike. No prefix matching, no
///      substring matching: `sldworks` is `sldworks`. A substring rule would
///      make `edge` match `msedge` and quietly kill the feature in a browser,
///      which is one of the two places the owner most wants it.
///   3. **IT IS READ ON A POLLER THREAD, NEVER IN THE CALLBACK.** The verdict
///      is published into one `AtomicBool` by `st-exclusion-watcher`, which
///      already resolves the foreground exe stem every 500 ms for the user's
///      own exception list. The hook callback reads that atomic and nothing
///      else (PROBLEM 58/134/184 — a foreground-window query is a win32k call).
///      The cost of that arrangement is up to 500 ms of latency after an
///      alt-tab INTO a listed app, which is exactly the latency the user's own
///      App exceptions have always had.
///
/// ### The 500 ms window, and why the feature fails CLOSED into it
///
/// Alt-tab into SolidWorks and press the middle button inside the first 500 ms
/// and the poller has not caught up yet. That window is unavoidable without
/// putting a win32k call on the callback, which is forbidden. What IS avoidable
/// is the much worse version of the same failure: the watcher thread failing to
/// spawn at all (PROBLEM 124 — `start_exclusion_watcher` is explicitly allowed
/// to fail and logs that shortcuts are unaffected). With no watcher, this
/// atomic would sit `false` for the whole session and the middle button would
/// be swallowed in every CAD program on the machine, forever, silently.
///
/// So `hook::middle_button_down_accepted` — the ONE gate, and the only place
/// the trigger is ever allowed to say yes — takes `watcher_alive` as its own
/// separate parameter and refuses without it. **No watcher, no middle-button
/// trigger.** A feature that can break somebody's CAD work does not get to run
/// blind. Generalise: *when a guard and the feature it guards can fail
/// independently, the feature must be the one that fails.*
///
/// (That sentence used to name a helper `middle_trigger_armed()`. It was
/// deleted on 2026-09-10: nothing ever called it, `#![allow(dead_code)]` hid
/// that, and folding this gate together with the on/off switch would have cost
/// the caller the ability to say which of the two refused. See the note on
/// `middle_button_down_accepted`.)
///
/// ### Known gaps, written down rather than pretended away
///
/// * **Onshape** is a WEB app. `onshape` below covers the Electron desktop
///   wrapper; Onshape in a browser tab is indistinguishable from any other tab
///   to a foreground-exe probe, and the middle button WILL raise the ring
///   there. The user's own App exceptions cannot fix that either. If it ever
///   matters, the answer is the toggle in Settings, not a browser-title probe.
/// * **Godot** ships as a VERSIONED exe (`Godot_v4.2-stable_win64.exe`), so an
///   exact-stem table cannot name it. Listed anyway as `godot` for the copies
///   people rename, and called out here so the next reader does not think it
///   was forgotten.
/// * Anything a user installs under a renamed exe is invisible to this table.
///   That is what the user's own App exceptions and the Settings switch are
///   for.
use std::sync::atomic::{AtomicBool, Ordering};

/// THE TABLE. Lowercase exe STEMS (no `.exe`), as `normalize_stem` produces.
///
/// Sorted by family, and every family says what the middle button does there,
/// because that — not the app's fame — is the reason a row belongs. If you add
/// a row, say which gesture you are protecting.
///
/// ── 3D / CAD / BIM: middle-drag ORBITS or PANS the model ───────────────────
/// ── Game engines: middle-drag pans the scene view ──────────────────────────
/// ── Slicers and mesh tools: middle-drag orbits the print bed ───────────────
/// ── 2D design canvases: middle-drag PANS the artboard ──────────────────────
///
/// The last group is the one the brief left to judgement ("any others you
/// judge to belong"). Middle-drag panning is as load-bearing in Photoshop,
/// Figma and Krita as orbiting is in Blender, and the cost of including them
/// is only that the ring does not appear there. **It is also the easiest group
/// to prune** if the owner decides the middle button should reach them: delete
/// the rows, nothing else changes.
pub const ORBIT_APPS: &[&str] = &[
    // ── 3D / CAD / BIM — middle-drag orbits or pans the model ──────────────
    "sldworks",      // SolidWorks
    "edrawings",     // SolidWorks eDrawings viewer
    "fusion360",     // Autodesk Fusion 360 (webdeploy\production\Fusion360.exe)
    "fusion",        // Autodesk Fusion, post-2024 rename. Generic-looking on
    // purpose: the cost of a false hit is one program without the ring.
    "inventor",      // Autodesk Inventor
    "acad",          // AutoCAD, and every vertical (Mechanical, Electrical,
    // Civil 3D, Plant 3D) — they all run acad.exe
    "acadlt",        // AutoCAD LT
    "revit",         // Autodesk Revit
    "roamer",        // Autodesk Navisworks (Manage/Simulate = Roamer.exe)
    "alias",         // Autodesk Alias
    "cnext",         // CATIA V5 (CNEXT.exe) — not a typo
    "catstart",      // CATIA launcher
    "3dexperience",  // Dassault 3DEXPERIENCE platform
    "parametric",    // PTC Creo Parametric
    "proe",          // PTC Pro/ENGINEER (pre-Creo, still in service)
    "ugraf",         // Siemens NX (historic and current main exe)
    "nx",            // Siemens NX, newer launcher naming
    "edge",          // Siemens Solid Edge. GENERIC-LOOKING AND KEPT ANYWAY:
    // Microsoft Edge is `msedge`, so the obvious collision does not exist, and
    // the cost of any other collision is a program without the ring, never a
    // program that breaks.
    "rhino",         // McNeel Rhino (Rhino.exe on 7/8)
    "sketchup",      // Trimble SketchUp
    "vectorworks",   // Vectorworks
    "archicad",      // Graphisoft Archicad
    "bricscad",      // BricsCAD
    "draftsight",    // DraftSight
    "librecad",      // LibreCAD
    "freecad",       // FreeCAD
    "openscad",      // OpenSCAD
    "onshape",       // Onshape desktop wrapper (see "Known gaps" above)
    "blender",       // Blender
    "maya",          // Autodesk Maya
    "3dsmax",        // Autodesk 3ds Max
    "cinema 4d",     // Maxon Cinema 4D (the stem really does carry spaces)
    "houdini",       // SideFX Houdini
    "houdinifx",     // SideFX Houdini FX
    "modo",          // Foundry Modo
    "zbrush",        // Maxon ZBrush
    "keyshot",       // Luxion KeyShot
    "toolbag",       // Marmoset Toolbag
    "substance painter",              // Allegorithmic-era naming
    "substance designer",             // Allegorithmic-era naming
    "adobe substance 3d painter",     // Adobe-era naming
    "adobe substance 3d designer",    // Adobe-era naming
    // ── PCB and electronics CAD — middle-drag pans the board ───────────────
    "kicad",         // KiCad 6+ (one process)
    "pcbnew",        // KiCad 5 and earlier, standalone
    "eeschema",      // KiCad 5 and earlier, standalone
    "gerbview",      // KiCad gerber viewer
    "x2",            // Altium Designer (X2.exe)
    "dxp",           // Altium/Protel DXP, older installs
    // ── Game engines — middle-drag pans the scene view ─────────────────────
    "unity",         // Unity Editor
    "unrealeditor",  // Unreal Engine 5
    "ue4editor",     // Unreal Engine 4
    "godot",         // Godot (see "Known gaps" — the shipped exe is versioned)
    // ── Slicers, mesh and scientific viewers — middle-drag orbits ──────────
    "ultimaker-cura",
    "prusa-slicer",
    "bambustudio",
    "orcaslicer",
    "simplify3d",
    "meshmixer",
    "meshlab",
    "cloudcompare",
    "paraview",
    "comsol",
    "ansys",
    "ansyswbu",      // Ansys Workbench
    // ── 2D design canvases — middle-drag PANS the artboard ─────────────────
    //    Judged in, not asked for. Easiest group to prune; see the doc above.
    "photoshop",
    "illustrator",
    "indesign",
    "figma",
    "krita",
    "gimp",
    "inkscape",
    "affinity photo",
    "affinity designer",
    "affinity publisher",
    "clipstudiopaint",
    "aseprite",
    "paint.net",
    "qgis-bin",      // QGIS — middle-drag pans the map
    "arcgispro",     // Esri ArcGIS Pro — middle-drag pans the map
];

/// Is `foreground` (any form `normalize_stem` accepts) one of the built-in
/// middle-button exclusions? Pure, so the table is walkable in a test.
pub fn is_orbit_app(foreground: &str) -> bool {
    let fg = crate::hook::exclusions::normalize_stem(foreground);
    if fg.is_empty() {
        return false;
    }
    ORBIT_APPS.iter().any(|e| *e == fg)
}

/// The foreground app is on the table above. Read by the mouse callback as ONE
/// relaxed load; written ONLY by `st-exclusion-watcher` (see `publish`).
pub static ORBIT_ACTIVE: AtomicBool = AtomicBool::new(false);

/// `st-exclusion-watcher` has completed at least one probe, so `ORBIT_ACTIVE`
/// is a measurement rather than an initial value.
///
/// THE MIDDLE-BUTTON TRIGGER IS GATED ON THIS. See the header: a watcher that
/// never spawned would leave `ORBIT_ACTIVE` false for the session and swallow
/// the middle button inside every CAD program on the machine. With this gate,
/// the same failure simply means the middle button is never intercepted at all
/// — the app behaves as it did before this feature existed.
pub static WATCHER_ALIVE: AtomicBool = AtomicBool::new(false);

/// Publish the verdict for one foreground name. Called from the exclusion
/// watcher's tick, which has already paid for the `foreground_stem()` probe.
///
/// Logging here is legal (poller thread, not the callback) and is ON CHANGE
/// ONLY, exactly like `publish_excluded_apps`'s neighbour: an alt-tab must
/// never spam the log.
pub fn publish(foreground: &str) {
    let hit = is_orbit_app(foreground);
    WATCHER_ALIVE.store(true, Ordering::Relaxed);
    if ORBIT_ACTIVE.swap(hit, Ordering::Relaxed) != hit {
        if hit {
            log::info!(
                "orbit-apps: middle-button-ring-standing-down-for-a-3d-or-cad-program-spaceadom \
                 — {foreground} is foreground and is on the BUILT-IN middle-button exclusion \
                 list, so holding the middle button there orbits the model exactly as it always \
                 has. This is NOT the user's App exceptions list: Space, the ring, Space+letter \
                 and every special key keep working here. PROBLEM 263."
            );
        } else {
            log::info!(
                "orbit-apps: left the 3D/CAD program — the middle-button ring trigger is live \
                 again (PROBLEM 263)."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table is matched by EXACT stem through the one shared
    /// normalisation, in every form a foreground probe can produce.
    #[test]
    fn the_table_matches_every_form_of_a_listed_app() {
        assert!(is_orbit_app("C:\\Program Files\\SOLIDWORKS Corp\\SOLIDWORKS\\SLDWORKS.exe"));
        assert!(is_orbit_app("blender.exe"));
        assert!(is_orbit_app("Blender.EXE"));
        assert!(is_orbit_app("acad"));
        assert!(is_orbit_app("  \"D:/Autodesk/Fusion360.exe\"  "));
        assert!(is_orbit_app("Cinema 4D.exe"), "a stem with a space must still match");
    }

    /// NO SUBSTRING MATCHING. `edge` is on the table for Solid Edge; if the
    /// rule ever decayed into `contains`, Microsoft Edge — one of the two
    /// places this feature is most wanted — would silently lose it.
    #[test]
    fn matching_is_exact_and_never_a_substring() {
        assert!(is_orbit_app("edge.exe"), "Solid Edge is on the list");
        assert!(!is_orbit_app("msedge.exe"), "Microsoft Edge must NOT match");
        assert!(!is_orbit_app("edgewebview.exe"));
        assert!(!is_orbit_app("blender-launcher.exe"));
        assert!(!is_orbit_app("unityhub.exe"), "the Hub is not the Editor");
        assert!(!is_orbit_app("chrome.exe"));
        assert!(!is_orbit_app("explorer.exe"));
        assert!(!is_orbit_app("spaceadom.exe"));
    }

    /// An unreadable foreground name is "we do not know", never "matches".
    /// Failing the other way would stand the trigger down on every window the
    /// probe cannot query — which is most elevated ones.
    #[test]
    fn an_empty_foreground_name_never_matches() {
        assert!(!is_orbit_app(""));
        assert!(!is_orbit_app("   "));
    }

    /// The families the owner named must all be reachable. This is a spec
    /// test: if a rename drops one of these, the feature silently starts
    /// eating the orbit gesture in a program the owner listed by name.
    #[test]
    fn every_app_the_owner_named_is_on_the_table() {
        for stem in [
            "sldworks",     // SolidWorks
            "fusion360",    // Fusion 360
            "inventor",     // Inventor
            "acad",         // AutoCAD
            "cnext",        // CATIA
            "parametric",   // Creo
            "ugraf",        // NX
            "rhino",        // Rhino
            "sketchup",     // SketchUp
            "blender",      // Blender
            "maya",         // Maya
            "3dsmax",       // 3ds Max
            "revit",        // Revit
            "onshape",      // Onshape
            "freecad",      // FreeCAD
            "kicad",        // KiCad
            "x2",           // Altium
            "unity",        // Unity
            "unrealeditor", // Unreal
        ] {
            assert!(
                is_orbit_app(stem),
                "{stem} was named by the owner and must be on the built-in list"
            );
        }
    }

    /// No duplicates and nothing empty — a duplicate is harmless at runtime and
    /// is the fingerprint of two people adding the same app from two lists.
    #[test]
    fn the_table_is_clean() {
        let mut seen = std::collections::BTreeSet::new();
        for e in ORBIT_APPS {
            assert!(!e.is_empty(), "an empty entry would match nothing and hide a typo");
            assert_eq!(
                *e,
                crate::hook::exclusions::normalize_stem(e),
                "{e} is not already in normalised form, so it can never match"
            );
            assert!(seen.insert(*e), "{e} is on the table twice");
        }
    }

    /// The publish path is the ONLY writer, and it arms the watcher gate.
    /// Without that store the feature never runs at all (by design).
    #[test]
    fn publishing_arms_the_watcher_gate_and_tracks_the_foreground() {
        WATCHER_ALIVE.store(false, Ordering::SeqCst);
        ORBIT_ACTIVE.store(false, Ordering::SeqCst);
        publish("C:\\x\\SLDWORKS.exe");
        assert!(WATCHER_ALIVE.load(Ordering::SeqCst), "one probe arms the gate");
        assert!(ORBIT_ACTIVE.load(Ordering::SeqCst));
        publish("chrome.exe");
        assert!(!ORBIT_ACTIVE.load(Ordering::SeqCst));
        assert!(WATCHER_ALIVE.load(Ordering::SeqCst), "the gate stays armed");
    }
}
