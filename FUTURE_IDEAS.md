# SpaceToggle OS — Future Ideas & Roadmap

This document serves as a repository for future concepts, architectural ideas, and enhancements that have been proposed but are not currently implemented in the main application.

## 1. "Typing Calibration" Utility (Smart Rollover)
*   **Concept:** The `rollover_ms` configuration shouldn't just be a static slider. 
*   **Implementation Idea:** Implement a "Typing Calibration" utility in the settings menu. The user types a sample sentence at their natural speed. The Rust backend measures their average KeyDown-to-KeyUp speed for the Spacebar and dynamically calculates the optimal `rollover_ms` (usually 1.2x their average keystroke duration) to guarantee a zero-bug typing experience customized to their fingers.

## 2. "App-State Awareness" (Dynamic Contextual Actions)
*   **Concept:** Context-dependent shortcuts based on the currently active window. 
*   **Implementation Idea:** Allow the active profile to override bindings dynamically. For example, in a "Designer" profile:
    *   If **Photoshop** is active: `Space + B` selects the Brush tool.
    *   If **Figma** is active: `Space + B` triggers a design element helper.
    *   If **Default**: `Space + B` opens the Browser.
*(Note: Currently deferred because Profile Switching via Space+RAlt handles most contextual workflow switching needs).*
