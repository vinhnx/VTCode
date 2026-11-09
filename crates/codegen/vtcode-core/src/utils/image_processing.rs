//! Image processing utilities
//!
//! Re-exports from `vtcode-commons::image` plus URL-based image fetching.

use anyhow::{Context, Result};
use url::Url;
pub use vtcode_commons::image::*;

use crate::tools::web_fetch::{is_blocked_special_use_tld, is_private_host};

/// Maximum redirects followed for a remote image. Each hop is re-validated
/// against the SSRF guards: a public URL that redirects to a private host
/// must stop rather than leak internal bytes into the model context.
const MAX_IMAGE_REDIRECT_HOPS: usize = 5;

/// Validate a remote image URL before any network happens: HTTP(S) scheme, a
/// real host, and no private/special-use destination. Mirrors the `web_fetch`
/// destination guards so `@<url>` image fetches cannot reach hosts that page
/// fetches cannot.
fn validate_image_url(url: &str) -> Result<Url> {
    let parsed = Url::parse(url).with_context(|| format!("Invalid URL: {url}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("Invalid URL: {url}");
    }
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty())
        .with_context(|| format!("Invalid URL (no host): {url}"))?;
    if is_private_host(host) || is_blocked_special_use_tld(host) {
        anyhow::bail!("Refusing to fetch image from private/local host '{host}'");
    }
    Ok(parsed)
}

/// Pure redirect-hop check behind the reqwest policy below, factored out so the
/// hop cap and per-hop SSRF re-validation are unit-testable without a server.
fn image_redirect_hop_allowed(next: &Url, previous_hops: usize) -> bool {
    if previous_hops >= MAX_IMAGE_REDIRECT_HOPS {
        return false;
    }
    if !matches!(next.scheme(), "http" | "https") {
        return false;
    }
    next.host_str()
        .is_some_and(|host| !is_private_host(host) && !is_blocked_special_use_tld(host))
}

/// Reads an image from a URL and converts it to base64 format
pub async fn read_image_from_url(url: &str) -> Result<ImageData> {
    let parsed = validate_image_url(url)?;

    let client = vtcode_commons::http::apply_connect_timeout(
        reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).redirect(
            reqwest::redirect::Policy::custom(|attempt| {
                if image_redirect_hop_allowed(attempt.url(), attempt.previous().len()) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }),
        ),
    )
    .build()
    .context("Failed to create HTTP client")?;

    let response = client
        .get(parsed)
        .send()
        .await
        .with_context(|| format!("Failed to fetch image from URL: {url}"))?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("HTTP error when fetching image: {} (status: {})", url, response.status()));
    }

    let content_length = response.content_length().unwrap_or(0);
    if content_length > 20 * 1024 * 1024 {
        return Err(anyhow::anyhow!("Image from URL too large: {content_length} bytes (max 20MB)"));
    }

    let declared_mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(detect_mime_type_from_content_type);

    // A declared but unsupported image type (SVG/BMP/TIFF) must fail here
    // instead of falling through to magic-byte sniffing, which would
    // mislabel SVG markup as PNG and produce a provider 400 downstream.
    if let Some(content_type) = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        && content_type.to_lowercase().starts_with("image/")
        && declared_mime_type
            .as_deref()
            .is_none_or(|mime| !is_supported_image_mime_type(mime))
    {
        return Err(anyhow::anyhow!("Unsupported image content type for LLM vision: {content_type}"));
    }

    let mime_type = declared_mime_type;

    let file_contents = response
        .bytes()
        .await
        .with_context(|| format!("Failed to read response body from: {url}"))?
        .to_vec();

    // Reject markup/error pages that slipped past the header check: SVG and
    // HTML payloads are text, not raster images, and providers refuse them.
    let leading = file_contents.iter().take_while(|byte| byte.is_ascii_whitespace()).count();
    if file_contents.len() > leading && file_contents[leading] == b'<' {
        return Err(anyhow::anyhow!("URL did not return raster image data: {url}"));
    }

    let mime_type = mime_type.unwrap_or_else(|| detect_mime_type_from_data(&file_contents));
    if !is_supported_image_mime_type(&mime_type) {
        return Err(anyhow::anyhow!("Unsupported image content type for LLM vision: {mime_type}"));
    }
    let base64_data = encode_to_base64(&file_contents);

    Ok(ImageData {
        base64_data,
        mime_type,
        file_path: url.to_string(),
        size: file_contents.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_hosts_are_rejected() {
        for url in [
            "http://127.0.0.1/x.png",
            "http://127.0.0.2:8080/x.png",
            "https://10.1.2.3/x.png",
            "http://192.168.0.1/x.png",
            "http://172.16.5.4/x.png",
            "http://169.254.169.254/latest/meta-data/",
            "http://localhost/x.png",
            "http://localhost.localdomain/x.png",
            "http://[::1]/x.png",
            "http://[::ffff:127.0.0.1]/x.png",
            "http://user:pass@127.0.0.1/x.png",
        ] {
            assert!(validate_image_url(url).is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn special_use_tlds_are_rejected() {
        for url in [
            "http://printer.local/x.png",
            "http://printer.local./x.png",
            "http://wiki.internal/x.png",
            "http://app.localhost/x.png",
            "http://example.test/x.png",
            "http://example.invalid/x.png",
            "http://nas.home.arpa/x.png",
        ] {
            assert!(validate_image_url(url).is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn public_urls_are_accepted() {
        // Asymmetric partners to the rejection lists: public IPs, public DNS,
        // ports, and userinfo on a public host all validate.
        for url in [
            "https://example.com/x.png",
            "http://93.184.216.34/x.png",
            "https://example.com:8443/a/b.png?size=large#frag",
            "http://[2606:4700:4700::1111]/x.png",
            "https://user@example.com/x.png",
        ] {
            assert!(validate_image_url(url).is_ok(), "{url} must validate");
        }
    }

    #[test]
    fn non_http_schemes_and_malformed_urls_are_rejected() {
        for url in ["ftp://example.com/x.png", "file:///etc/passwd", "not a url", "http://"] {
            assert!(validate_image_url(url).is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn redirect_hops_recheck_the_destination_and_cap() {
        let public = Url::parse("https://cdn.example.com/x.png").unwrap();
        let private = Url::parse("http://169.254.169.254/x.png").unwrap();
        let downgrade = Url::parse("ftp://example.com/x.png").unwrap();
        assert!(image_redirect_hop_allowed(&public, 0));
        assert!(image_redirect_hop_allowed(&public, MAX_IMAGE_REDIRECT_HOPS - 1));
        assert!(!image_redirect_hop_allowed(&public, MAX_IMAGE_REDIRECT_HOPS));
        assert!(!image_redirect_hop_allowed(&private, 0));
        assert!(!image_redirect_hop_allowed(&downgrade, 0));
    }
}
