/// site_icon.rs — PROBLEM 267: a LINK binding's icon, fetched ONCE at bind
/// time.
///
/// The middle button's icon ring must show a real icon for every tile, and a
/// link has no exe to ask the shell about — its icon is the site's favicon.
/// The rule the design's disclosure line states ("Site icons are fetched once,
/// when you bind the link") is the whole architecture of this file:
///
///   * The key editor calls `fetch_site_icon` the moment a URL is bound (and
///     again on the next edit of a key whose binding still has no icon — the
///     one retry). The result is stored IN THE BINDING (`KeyBinding::site_icon`)
///     as a complete `data:` URL.
///   * NOTHING fetches at ring time. `middle_ring::build_entries` reads the
///     stored value or draws a letter disc. A ring that waited on the network
///     would arrive after the hand had let go.
///
/// Two attempts, in order, each under `FETCH_TIMEOUT`: `https://<host>/favicon.ico`,
/// then the page's own `<link rel="icon" …>` (or `shortcut icon` /
/// `apple-touch-icon`) resolved against the page URL. The bytes are accepted
/// only if they SNIFF as an image (`sniff_image_mime`) — a 200 with an HTML
/// "not found" page is the most common way a favicon request succeeds and lies
/// — and only up to `MAX_ICON_BYTES`, because a config.json that carries a
/// 2 MB PNG for one key is a config.json every Space-hold has to clone.
///
/// The impure half is one `async fn` around `reqwest`; everything it decides
/// with is a pure function below, and those have the tests.
use base64::{engine::general_purpose::STANDARD, Engine};

/// Per request. The brief's number: "3 s timeout".
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
/// The whole bind-time fetch, both attempts included, may not outlive this.
const TOTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
/// Largest icon we will store in the config. 64 KB of PNG is a generous
/// favicon; a 256 KB cap leaves room for a fat multi-size .ico.
const MAX_ICON_BYTES: usize = 256 * 1024;
/// How much of the page HTML to read while looking for `<link rel=icon>`.
/// Icons are declared in `<head>`; half a megabyte is far past it.
const MAX_HTML_BYTES: usize = 512 * 1024;
const UA: &str = "Spaceadom (favicon fetch at bind time; +https://github.com/spaceadom)";

// ---------------------------------------------------------------------------
// Pure
// ---------------------------------------------------------------------------

/// `https://<host>/favicon.ico` for a page URL, or `None` when the URL has no
/// usable host. The scheme is kept (an `http://` site gets `http://`), the
/// port too — a dev server on `:3000` serves its favicon there, not on 443.
pub fn favicon_url(page: &str) -> Option<String> {
    let (scheme, rest) = page.trim().split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?.trim();
    // Strip userinfo; keep host[:port].
    let host = authority.rsplit('@').next()?.trim();
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host}/favicon.ico"))
}

/// The origin (`scheme://host[:port]`) of a page URL, for resolving a
/// root-relative icon href.
fn origin_of(page: &str) -> Option<String> {
    let f = favicon_url(page)?;
    f.strip_suffix("/favicon.ico").map(str::to_string)
}

/// Resolve an icon `href` against the page it came from. Handles the four
/// forms real pages use: absolute, protocol-relative (`//cdn…`),
/// root-relative (`/x.png`) and document-relative (`img/x.png`).
pub fn resolve_href(page: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() || href.starts_with("data:") {
        // A data: icon is already what we want — but we never store one that
        // did not pass the sniff, and a data: href is handed back as-is for
        // the caller to sniff after decoding. Keep this function URL-only.
        return None;
    }
    if href.contains("://") {
        return Some(href.to_string());
    }
    let origin = origin_of(page)?;
    if let Some(rest) = href.strip_prefix("//") {
        let scheme = origin.split("://").next()?;
        return Some(format!("{scheme}://{rest}"));
    }
    if let Some(rest) = href.strip_prefix('/') {
        return Some(format!("{origin}/{rest}"));
    }
    // Document-relative: the page's directory.
    let (_, after) = page.split_once("://")?;
    let path = after.split(['?', '#']).next().unwrap_or(after);
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "",
    };
    let dir = dir.split_once('/').map(|(_, p)| p).unwrap_or("");
    Some(format!("{origin}/{dir}{href}"))
}

/// Find the page's declared icon: the FIRST `<link>` whose `rel` contains
/// `icon` (`icon`, `shortcut icon`, `apple-touch-icon`), returning its `href`
/// resolved against `page`. A small hand-rolled scan rather than an HTML
/// parser: the tag is one element, attribute order is arbitrary, quotes are
/// either kind, and that is the whole grammar worth handling for an icon.
pub fn pick_icon_link(html: &str, page: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<link") {
        let start = from + i;
        let end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &html[start..end];
        from = end.max(start + 5);
        let rel = attr(tag, "rel").unwrap_or_default().to_ascii_lowercase();
        let is_icon = rel.split_whitespace().any(|w| w == "icon" || w == "apple-touch-icon");
        if !is_icon {
            continue;
        }
        if let Some(href) = attr(tag, "href") {
            if let Some(url) = resolve_href(page, &href) {
                return Some(url);
            }
        }
    }
    None
}

/// One attribute's value out of a tag, quoted either way or bare.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        // Must be a whole attribute name: preceded by whitespace, followed by
        // optional whitespace and `=`.
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        if !before_ok {
            continue;
        }
        let rest = lower[at + name.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let off = tag.len() - rest.len();
        let raw = &tag[off..];
        let value = if let Some(q) = raw.strip_prefix('"') {
            q.split('"').next().unwrap_or("")
        } else if let Some(q) = raw.strip_prefix('\'') {
            q.split('\'').next().unwrap_or("")
        } else {
            raw.split(|c: char| c.is_ascii_whitespace() || c == '>').next().unwrap_or("")
        };
        return Some(value.to_string());
    }
    None
}

/// The image type the BYTES say they are — never what the server said. A
/// favicon request that "succeeds" with an HTML 404 page is the most common
/// failure, and `Content-Type: image/x-icon` on such a body is not unheard of.
pub fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 4 {
        return None;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
    let head = head.trim_start();
    if head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg")) {
        return Some("image/svg+xml");
    }
    None
}

/// Bytes → the `data:` URL the binding stores, or `None` when they are not an
/// image or are too large. This is the ONLY function that decides what is
/// allowed into config.json.
pub fn to_data_url(bytes: &[u8]) -> Option<String> {
    if bytes.len() > MAX_ICON_BYTES {
        return None;
    }
    let mime = sniff_image_mime(bytes)?;
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

// ---------------------------------------------------------------------------
// Impure — one GET per attempt, all inside TOTAL_TIMEOUT
// ---------------------------------------------------------------------------

/// GET `url`, reading at most `cap` bytes. Errors are strings for the log.
async fn get_bytes(client: &reqwest::Client, url: &str, cap: usize) -> Result<Vec<u8>, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request failed ({e})"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("body read failed ({e})"))?;
    if bytes.len() > cap {
        return Err(format!("body too large ({} bytes, cap {cap})", bytes.len()));
    }
    Ok(bytes.to_vec())
}

async fn fetch_inner(url: String) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(UA)
        .build()
        .ok()?;
    // Attempt 1: /favicon.ico at the site root.
    if let Some(ico) = favicon_url(&url) {
        match get_bytes(&client, &ico, MAX_ICON_BYTES).await {
            Ok(bytes) => {
                if let Some(data) = to_data_url(&bytes) {
                    log::info!("site-icon: {ico} → {} bytes, stored (PROBLEM 267)", bytes.len());
                    return Some(data);
                }
                log::info!("site-icon: {ico} answered {} bytes that are not an image — trying the page's <link rel=icon>", bytes.len());
            }
            Err(e) => log::info!("site-icon: {ico} — {e}; trying the page's <link rel=icon>"),
        }
    }
    // Attempt 2: the page's own <link rel=icon>.
    let html = match get_bytes(&client, &url, MAX_HTML_BYTES).await {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => {
            log::info!("site-icon: {url} — page fetch failed ({e}); no icon this time, the next edit of the key retries");
            return None;
        }
    };
    let Some(link) = pick_icon_link(&html, &url) else {
        log::info!("site-icon: {url} declares no <link rel=icon>; no icon this time");
        return None;
    };
    match get_bytes(&client, &link, MAX_ICON_BYTES).await {
        Ok(bytes) => {
            let data = to_data_url(&bytes);
            if data.is_some() {
                log::info!("site-icon: {link} → {} bytes, stored (PROBLEM 267)", bytes.len());
            } else {
                log::info!("site-icon: {link} answered {} bytes that are not an image — no icon", bytes.len());
            }
            data
        }
        Err(e) => {
            log::info!("site-icon: {link} — {e}; no icon this time");
            None
        }
    }
}

/// Fetch a site's favicon for the key editor, at bind time. Returns the
/// `data:` URL to store in `KeyBinding::site_icon`, or `None` — never an
/// error, because an absent icon is the whole user-facing failure mode (the
/// ring draws a letter disc, and the next edit retries).
///
/// `async` and on the runtime, never the main thread: three seconds per
/// attempt is nothing to a background task and everything to a dashboard.
/// A PACKAGED copy still fetches — this is the user's own bound URL, not a
/// service of ours (unlike `release_notes`, which stays silent when packaged).
#[tauri::command]
pub async fn fetch_site_icon(url: String) -> Option<String> {
    let url = url.trim().to_string();
    if favicon_url(&url).is_none() {
        log::info!("site-icon: {url:?} is not an http(s) URL with a host — nothing to fetch");
        return None;
    }
    match tokio::time::timeout(TOTAL_TIMEOUT, fetch_inner(url.clone())).await {
        Ok(r) => r,
        Err(_) => {
            log::info!("site-icon: {url} — gave up after {TOTAL_TIMEOUT:?}; the next edit of the key retries");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favicon_url_keeps_scheme_host_and_port_and_refuses_the_rest() {
        assert_eq!(favicon_url("https://github.com/x/y?z").as_deref(), Some("https://github.com/favicon.ico"));
        assert_eq!(favicon_url("http://localhost:3000/app").as_deref(), Some("http://localhost:3000/favicon.ico"));
        assert_eq!(favicon_url("HTTPS://Example.COM").as_deref(), Some("https://Example.COM/favicon.ico"));
        assert_eq!(favicon_url("https://user:pw@host.tld/p").as_deref(), Some("https://host.tld/favicon.ico"));
        assert!(favicon_url("ftp://x/y").is_none());
        assert!(favicon_url("github.com").is_none(), "no scheme, no fetch — the editor normalises before binding");
        assert!(favicon_url("https:///nohost").is_none());
    }

    #[test]
    fn icon_link_is_found_in_any_attribute_order_and_resolved() {
        let page = "https://example.com/docs/index.html";
        let html = r#"<html><head>
            <link href="/static/fav.png" rel="icon" type="image/png">
            <link rel="stylesheet" href="x.css"></head></html>"#;
        assert_eq!(pick_icon_link(html, page).as_deref(), Some("https://example.com/static/fav.png"));
        let html2 = "<LINK REL='shortcut icon' HREF=img/i.ico>";
        assert_eq!(pick_icon_link(html2, page).as_deref(), Some("https://example.com/docs/img/i.ico"));
        let html3 = r#"<link rel="apple-touch-icon" href="//cdn.example.net/a.png">"#;
        assert_eq!(pick_icon_link(html3, page).as_deref(), Some("https://cdn.example.net/a.png"));
        let html4 = r#"<link rel="icon" href="https://other.tld/abs.svg">"#;
        assert_eq!(pick_icon_link(html4, page).as_deref(), Some("https://other.tld/abs.svg"));
        // A stylesheet whose href contains "icon" is not an icon link; `rel`
        // decides. And a page with no icon link yields None.
        assert!(pick_icon_link(r#"<link rel="stylesheet" href="/icons.css">"#, page).is_none());
        assert!(pick_icon_link("<p>no head</p>", page).is_none());
        // `preconnect` is not `icon`; the `rel` must contain the whole word.
        assert!(pick_icon_link(r#"<link rel="preconnect" href="https://x">"#, page).is_none());
    }

    #[test]
    fn the_sniff_trusts_bytes_not_headers() {
        assert_eq!(sniff_image_mime(&[0x89, b'P', b'N', b'G', 0, 0]), Some("image/png"));
        assert_eq!(sniff_image_mime(&[0, 0, 1, 0, 1, 0]), Some("image/x-icon"));
        assert_eq!(sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image_mime(b"GIF89a...."), Some("image/gif"));
        assert_eq!(sniff_image_mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_image_mime(b"  <svg xmlns='x'></svg>"), Some("image/svg+xml"));
        assert_eq!(sniff_image_mime(b"<?xml version='1.0'?><svg/>"), Some("image/svg+xml"));
        // The failure that lies most often: an HTML 404 served as an icon.
        assert!(sniff_image_mime(b"<!DOCTYPE html><html>Not found</html>").is_none());
        assert!(sniff_image_mime(b"").is_none());
        assert!(sniff_image_mime(&[1, 2]).is_none());
    }

    #[test]
    fn to_data_url_encodes_images_and_refuses_junk_and_giants() {
        let png = [0x89u8, b'P', b'N', b'G', 1, 2, 3, 4];
        let url = to_data_url(&png).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        assert_eq!(STANDARD.decode(url.split(',').nth(1).unwrap()).unwrap(), png);
        assert!(to_data_url(b"<html>").is_none());
        let giant = vec![0u8; MAX_ICON_BYTES + 1];
        assert!(to_data_url(&giant).is_none(), "a giant icon never reaches config.json");
    }

    #[test]
    fn href_resolution_handles_the_four_forms() {
        let page = "https://example.com/a/b/page.html?q=1#f";
        assert_eq!(resolve_href(page, "https://x.y/z.png").as_deref(), Some("https://x.y/z.png"));
        assert_eq!(resolve_href(page, "//cdn.x/z.png").as_deref(), Some("https://cdn.x/z.png"));
        assert_eq!(resolve_href(page, "/root.png").as_deref(), Some("https://example.com/root.png"));
        assert_eq!(resolve_href(page, "rel.png").as_deref(), Some("https://example.com/a/b/rel.png"));
        assert_eq!(resolve_href("https://example.com", "rel.png").as_deref(), Some("https://example.com/rel.png"));
        assert!(resolve_href(page, "").is_none());
        assert!(resolve_href(page, "data:image/png;base64,AAAA").is_none());
    }
}
