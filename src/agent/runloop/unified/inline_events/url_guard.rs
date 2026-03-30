use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::Url;
use vtcode_tui::app::{InlineListItem, InlineListSelection};

pub(crate) const URL_GUARD_APPROVE_ACTION: &str = "url_guard:approve";
pub(crate) const URL_GUARD_DENY_ACTION: &str = "url_guard:deny";

const TRUSTED_HOSTS: &[&str] = &[
    "auth.openai.com",
    "platform.openai.com",
    "api.openai.com",
    "deepwiki.com",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UrlGuardPrompt {
    url: String,
    host_label: String,
    insecure_transport: bool,
    local_or_private: bool,
    trusted_host: bool,
}

impl UrlGuardPrompt {
    pub(crate) fn parse(url: String) -> Option<Self> {
        let parsed = Url::parse(&url).ok()?;
        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            return None;
        }

        let host = parsed.host_str()?.to_ascii_lowercase();
        let port = parsed
            .port()
            .map(|value| format!(":{value}"))
            .unwrap_or_default();
        let host_label = format!("{host}{port}");

        Some(Self {
            url,
            host_label,
            insecure_transport: scheme == "http",
            local_or_private: is_local_or_private_host(&host),
            trusted_host: is_trusted_host(&host),
        })
    }

    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    pub(crate) fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            "This link may be unsafe. VT Code requires approval before opening external URLs."
                .to_string(),
        ];

        if self.insecure_transport {
            lines.push(
                "Plain HTTP is insecure. Traffic can be intercepted or modified before it reaches your browser."
                    .to_string(),
            );
        } else if self.local_or_private {
            lines.push(
                "This destination is a local or private address. Opening it may interact with services on this machine or network."
                    .to_string(),
            );
        } else if self.trusted_host {
            lines.push(
                "Destination matches VT Code's small built-in trusted host list, but approval is still required."
                    .to_string(),
            );
        } else {
            lines.push(
                "Destination is not in VT Code's small built-in trusted host list. Review it carefully before proceeding."
                    .to_string(),
            );
        }

        if self.insecure_transport && self.local_or_private {
            lines.push(
                "This URL is both plain HTTP and local/private. Only continue if you expected this exact destination."
                    .to_string(),
            );
        }

        lines.push(format!("Host: {}", self.host_label));
        lines.push(format!("URL: {}", self.url));
        lines.push("Choose Open to continue or Cancel to stay in VT Code.".to_string());

        lines
    }

    pub(crate) fn items(&self) -> Vec<InlineListItem> {
        let badge = if self.insecure_transport {
            Some("HTTP".to_string())
        } else if self.local_or_private {
            Some("Local".to_string())
        } else if self.trusted_host {
            Some("Known".to_string())
        } else {
            Some("HTTPS".to_string())
        };

        vec![
            InlineListItem {
                title: "Cancel".to_string(),
                subtitle: Some("Do not open this link.".to_string()),
                badge: Some("Default".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    URL_GUARD_DENY_ACTION.to_string(),
                )),
                search_value: None,
            },
            InlineListItem {
                title: "Open in browser".to_string(),
                subtitle: Some(
                    "Launch the exact URL in your default browser after approval.".to_string(),
                ),
                badge,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    URL_GUARD_APPROVE_ACTION.to_string(),
                )),
                search_value: None,
            },
        ]
    }

    pub(crate) fn default_selection(&self) -> InlineListSelection {
        InlineListSelection::ConfigAction(URL_GUARD_DENY_ACTION.to_string())
    }
}

fn is_trusted_host(host: &str) -> bool {
    TRUSTED_HOSTS
        .iter()
        .any(|trusted| host_matches_domain(host, trusted))
}

fn host_matches_domain(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn is_local_or_private_host(host: &str) -> bool {
    if matches!(host, "localhost" | "0.0.0.0")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return true;
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(addr) => is_local_or_private_ipv4(addr),
            IpAddr::V6(addr) => is_local_or_private_ipv6(addr),
        };
    }

    false
}

fn is_local_or_private_ipv4(addr: Ipv4Addr) -> bool {
    addr.is_private()
        || addr.is_loopback()
        || addr.is_link_local()
        || addr.is_broadcast()
        || addr.is_documentation()
        || addr.is_unspecified()
}

fn is_local_or_private_ipv6(addr: Ipv6Addr) -> bool {
    addr.is_loopback()
        || addr.is_unspecified()
        || addr.is_unique_local()
        || addr.is_unicast_link_local()
}

#[cfg(test)]
mod tests {
    use super::UrlGuardPrompt;

    #[test]
    fn parse_http_url_marks_insecure_transport() {
        let prompt =
            UrlGuardPrompt::parse("http://example.com/docs".to_string()).expect("http prompt");

        let lines = prompt.lines();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Plain HTTP is insecure"))
        );
    }

    #[test]
    fn parse_https_known_host_marks_trusted() {
        let prompt = UrlGuardPrompt::parse("https://auth.openai.com/oauth/authorize".to_string())
            .expect("trusted host");

        let lines = prompt.lines();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("built-in trusted host list"))
        );
    }

    #[test]
    fn parse_localhost_url_marks_local_private() {
        let prompt = UrlGuardPrompt::parse("https://localhost:1455/auth/callback".to_string())
            .expect("localhost prompt");

        let lines = prompt.lines();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("local or private address"))
        );
    }
}
