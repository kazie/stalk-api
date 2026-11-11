use std::collections::HashSet;

/// Utilities for testing and client helpers.
///
/// Exposes `ws_url` for URL construction and `parse_api_tokens` for splitting
/// comma-separated API keys.
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

/// Parses comma-separated tokens, trimming whitespace and filtering out empty entries.
///
/// # Examples
///
/// Parsing single or multiple tokens:
///
/// ```
/// use stalk_api::utils::parse_api_tokens;
/// let tokens = parse_api_tokens("token1, token2 , token3 ");
/// assert!(tokens.contains("token1"));
/// assert!(tokens.contains("token2"));
/// assert!(tokens.contains("token3"));
/// assert_eq!(tokens.len(), 3);
/// ```
///
/// Handling whitespace and empty segments:
///
/// ```
/// use stalk_api::utils::parse_api_tokens;
/// let tokens = parse_api_tokens(" , token1 ,, token2 , ");
/// assert_eq!(tokens.len(), 2);
/// assert!(tokens.contains("token1"));
/// assert!(tokens.contains("token2"));
/// ```
pub fn parse_api_tokens(raw: &str) -> HashSet<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_api_tokens_single() {
        let tokens = parse_api_tokens("secret-key");
        assert_eq!(tokens.len(), 1);
        assert!(tokens.contains("secret-key"));
    }

    #[test]
    fn test_parse_api_tokens_multiple_with_whitespace() {
        let tokens = parse_api_tokens(" key_a ,  key_b , key_c ");
        assert_eq!(tokens.len(), 3);
        assert!(tokens.contains("key_a"));
        assert!(tokens.contains("key_b"));
        assert!(tokens.contains("key_c"));
    }

    #[test]
    fn test_parse_api_tokens_empty_and_commas() {
        let tokens = parse_api_tokens(",,,  ,  ,");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_parse_api_tokens_duplicates() {
        let tokens = parse_api_tokens("key1, key1, key2, key1");
        assert_eq!(tokens.len(), 2);
        assert!(tokens.contains("key1"));
        assert!(tokens.contains("key2"));
    }
}
