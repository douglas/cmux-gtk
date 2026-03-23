use crate::settings;

pub(super) fn normalize_url(input: &str, engine: settings::SearchEngine) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "about:blank".to_string();
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("file://")
    {
        return trimmed.to_string();
    }
    if trimmed.contains('.') && !trimmed.contains(' ') {
        return format!("https://{trimmed}");
    }
    engine.search_url(trimmed)
}

/// Extract the host portion from a URL (e.g. "http://example.com:8080/path" -> "example.com").
pub(super) fn extract_host(url: &str) -> String {
    let after_scheme = url.find("://").map(|i| &url[i + 3..]).unwrap_or(url);
    // Strip userinfo@ if present
    let after_user = after_scheme
        .find('@')
        .map(|i| &after_scheme[i + 1..])
        .unwrap_or(after_scheme);
    // Take up to the first / or : (port)
    let end = after_user
        .find(&['/', ':', '?', '#'][..])
        .unwrap_or(after_user.len());
    after_user[..end].to_lowercase()
}

/// Generate an HTML interstitial warning page for insecure HTTP navigation.
pub(super) fn insecure_http_interstitial(url: &str) -> String {
    let escaped = url
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">
<style>
  body {{ font-family: system-ui, sans-serif; padding: 40px; text-align: center;
         color: #333; background: #fff; }}
  @media (prefers-color-scheme: dark) {{
    body {{ color: #ddd; background: #1e1e2e; }}
    .url {{ color: #f38ba8; }}
    button {{ background: #45475a; color: #cdd6f4; border-color: #585b70; }}
    button:hover {{ background: #585b70; }}
  }}
  h2 {{ margin-bottom: 8px; }}
  .icon {{ font-size: 48px; margin-bottom: 16px; }}
  .url {{ word-break: break-all; color: #d32; font-family: monospace; font-size: 14px; }}
  .actions {{ margin-top: 24px; }}
  button {{ padding: 8px 20px; margin: 0 8px; border-radius: 6px; cursor: pointer;
            font-size: 14px; border: 1px solid #ccc; background: #f5f5f5; }}
  button:hover {{ background: #e0e0e0; }}
  button.proceed {{ background: #e74c3c; color: white; border-color: #c0392b; }}
  button.proceed:hover {{ background: #c0392b; }}
</style></head><body>
<div class="icon">&#9888;&#65039;</div>
<h2>Insecure Connection</h2>
<p>This page is being served over an unencrypted HTTP connection:</p>
<p class="url">{escaped}</p>
<p>Your data may be visible to others on the network.</p>
<div class="actions">
  <button onclick="history.back()">Go Back</button>
  <button class="proceed" onclick="location.href='{escaped}'">Proceed Anyway</button>
</div>
</body></html>"#
    )
}
