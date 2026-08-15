
/// Search the filesystem for Brave or Chrome at their standard install paths.
/// Returns the absolute path to the first found executable, or `None`.
pub fn find_browser() -> Option<String> {
    let candidates = browser_candidates();
    candidates
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
}

/// All standard install paths for Brave and Chrome, in priority order.
fn browser_candidates() -> Vec<String> {
    let pf = std::env::var("ProgramFiles").unwrap_or_default();
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();

    vec![
        // Brave — preferred
        format!("{pf}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
        format!("{local}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
        // Chrome
        format!("{pf}\\Google\\Chrome\\Application\\chrome.exe"),
        format!("{local}\\Google\\Chrome\\Application\\chrome.exe"),
    ]
}

/// Validate that a user-supplied browser path actually exists and is an executable.
pub fn validate_browser_path(path: &str) -> bool {
    let p = std::path::Path::new(path);
    p.exists()
        && p.extension()
            .map(|e| e.to_string_lossy().to_lowercase() == "exe")
            .unwrap_or(false)
}
