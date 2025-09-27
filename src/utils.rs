/// Utilities for testing and client helpers.
///
/// Currently exposes `ws_url`, a tiny helper that turns a base http(s) URL into
/// a ws(s) URL and appends a WebSocket path. This is handy in tests and
/// examples when spinning up an ephemeral HTTP server and then connecting via
/// WebSocket.
///
/// The function is intentionally simple and assumes `path` starts with `/`.
///
/// # Examples
///
/// Convert http -> ws and append path:
///
/// ```
/// use stalk_api::utils::ws_url;
/// assert_eq!(ws_url("http://my-http-site.com", "/ws"), "ws://my-http-site.com/ws");
/// ```
///
/// Handle trailing slash on the base URL without producing a double slash:
///
/// ```
/// use stalk_api::utils::ws_url;
/// assert_eq!(ws_url("http://my-http-site.com/", "/ws"), "ws://my-http-site.com/ws");
/// ```
///
/// HTTPS becomes WSS:
///
/// ```
/// use stalk_api::utils::ws_url;
/// assert_eq!(ws_url("https://example.org", "/ws/coords"), "wss://example.org/ws/coords");
/// ```
pub fn ws_url(http_url: &str, path: &str) -> String {
    let base = http_url.trim_end_matches('/');
    // Only rewrite the scheme prefix, leave the rest untouched.
    let rewritten = if base.starts_with("https://") {
        base.replacen("https", "wss", 1)
    } else if base.starts_with("http://") {
        base.replacen("http", "ws", 1)
    } else {
        base.to_string()
    };
    format!("{}{}", rewritten, path)
}
