//! Curated network allowlist for VT Code's agent egress.
//!
//! The allowlist is loaded from a TOML file shipped with the crate (see
//! `data/network_allowlist.toml`) and organized by category. Categories
//! include AI provider endpoints, web & specialized search, web-crawl
//! helpers (Jina, Defuddle, Firecrawl, etc.), MCP servers, package
//! registries, code-hosting platforms, OAuth/identity, dev-infrastructure,
//! and OS-update mirrors.
//!
//! The TOML is the source of truth; the Rust types in this module are
//! derived from the on-disk shape so we can also expose the per-category
//! lists (e.g., for diagnostics or for the agent prompt to describe
//! "where you can fetch from"). The flat `all_allow_domains()` view
//! collapses every category into one deduped `Vec<String>` and is what
//! the `WebFetchConfig` defaults consume.
//!
//! The allowlist also supports a `verify = true` flag per entry. Entries
//! with that flag are surfaced via [`NetworkAllowlist::unverified_entries`]
//! so a startup hook can warn the operator before they're used.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Embedded copy of the curated allowlist TOML.
///
/// Including the file at compile time means the allowlist is always
/// available — even in WASM / sandbox environments where reading a
/// runtime file path would be a problem.
pub const DEFAULT_ALLOWLIST_TOML: &str = include_str!("../data/network_allowlist.toml");

/// Top-level allowlist document.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct NetworkAllowlist {
    /// File-level metadata (`[meta]` block).
    #[serde(default)]
    pub meta: Option<AllowlistMeta>,

    /// LLM inference endpoints (cloud-hosted).
    #[serde(default)]
    pub ai_providers: AiProviderCategories,

    /// Search APIs (web + specialized/vertical).
    #[serde(default)]
    pub search: SearchCategories,

    /// Full-page fetch & content extraction services.
    #[serde(default, rename = "web_crawl")]
    pub web_crawl: Vec<AllowlistEntry>,

    /// MCP tool server endpoints.
    #[serde(default, rename = "mcp_servers")]
    pub mcp_servers: Vec<AllowlistEntry>,

    /// Language package manager registries.
    #[serde(default)]
    pub package_registries: Vec<AllowlistEntry>,

    /// Git platforms & raw content hosts.
    #[serde(default)]
    pub code_hosting: Vec<AllowlistEntry>,

    /// OAuth & identity provider endpoints.
    #[serde(default)]
    pub auth: Vec<AllowlistEntry>,

    /// Cloud infra, databases, observability.
    #[serde(default, rename = "dev_infra")]
    pub dev_infra: Vec<AllowlistEntry>,

    /// System / OS package mirrors.
    #[serde(default, rename = "os_updates")]
    pub os_updates: Vec<AllowlistEntry>,
}

/// Allowlist file-level metadata.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AllowlistMeta {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub last_updated: Option<String>,
    #[serde(default)]
    pub maintainer: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// AI provider entries split into cloud vs. local/self-hosted.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AiProviderCategories {
    #[serde(default)]
    pub cloud: Vec<AllowlistEntry>,
    #[serde(default)]
    pub local: Vec<LocalAiProviderEntry>,
}

/// Search entries split into generic web vs. specialized/vertical.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SearchCategories {
    #[serde(default)]
    pub web: Vec<AllowlistEntry>,
    #[serde(default)]
    pub specialized: Vec<AllowlistEntry>,
}

/// A single allowlist row.
///
/// `domain` is the primary host key. Optional `path` narrows the exemption
/// to URLs that begin with the given path (used for `github.com/login/oauth`).
/// `verify = true` flags entries that should be re-confirmed before the
/// agent relies on them.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct AllowlistEntry {
    /// Human-readable label.
    #[serde(default)]
    pub name: Option<String>,
    /// Apex domain (e.g. `github.com`). May be a wildcard like `*.auth0.com`.
    #[serde(default)]
    pub domain: Option<String>,
    /// Optional path prefix that the URL must start with.
    #[serde(default)]
    pub path: Option<String>,
    /// Network protocol; defaults to `https` when missing.
    #[serde(default)]
    pub protocol: Option<String>,
    /// Free-form notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// When `true`, surface the entry via `unverified_entries` so callers
    /// can warn or block until the operator confirms.
    #[serde(default)]
    pub verify: bool,
}

/// Local/self-hosted AI provider — identified by `host` + `port` rather
/// than a public domain. The allowlist tracks these for diagnostics; the
/// agent sandbox already permits `localhost` traffic via separate config.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct LocalAiProviderEntry {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl NetworkAllowlist {
    /// Parse the embedded TOML. Returns a default empty allowlist if the
    /// parse fails so the agent can still start in degraded mode (a
    /// warning is logged by the caller).
    pub fn load_default() -> Self {
        toml::from_str(DEFAULT_ALLOWLIST_TOML).unwrap_or_default()
    }

    /// Flat list of every domain string in the allowlist, including
    /// wildcards. Local/self-hosted entries are excluded because they
    /// have no public domain to match against.
    ///
    /// Order is preserved per category so the resulting list is
    /// deterministic for tests and config dumps. Use a `BTreeSet` when
    /// callers want stable dedup without ordering.
    pub fn all_allow_domains(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for entry in self
            .ai_providers
            .cloud
            .iter()
            .chain(self.search.web.iter())
            .chain(self.search.specialized.iter())
            .chain(self.web_crawl.iter())
            .chain(self.mcp_servers.iter())
            .chain(self.package_registries.iter())
            .chain(self.code_hosting.iter())
            .chain(self.auth.iter())
            .chain(self.dev_infra.iter())
            .chain(self.os_updates.iter())
        {
            if let Some(domain) = entry.domain.as_deref() {
                let trimmed = domain.trim();
                if !trimmed.is_empty() && !out.iter().any(|d| d == trimmed) {
                    out.push(trimmed.to_string());
                }
            }
        }
        out
    }

    /// Deduplicated set form of `all_allow_domains()`.
    pub fn all_allow_domains_set(&self) -> BTreeSet<String> {
        self.all_allow_domains().into_iter().collect()
    }

    /// Domains from the categories that are appropriate for the
    /// `web_fetch` tool's default allowlist.
    ///
    /// Excludes:
    /// - `ai_providers.cloud` (LLM inference endpoints; require auth and
    ///   return raw JSON, not web content)
    /// - `auth` (OAuth/identity endpoints; never useful as a fetch target)
    /// - `dev_infra` (backend services, observability endpoints; auth-
    ///   required and not designed for public reading)
    /// - `os_updates` (system package mirrors; not useful for an agent)
    /// - `defuddle.md` from `web_crawl` (defuddle is a relay, not a fetch
    ///   target; routing web_fetch to it would bypass the SSRF checks)
    /// - `ai_providers.local` (no public domain)
    ///
    /// Includes the categories where the agent might want to read public
    /// web content: search engines, specialized knowledge bases,
    /// package registries, code-hosting platforms, MCP servers that
    /// expose public web content, and web-crawl relays other than
    /// defuddle.
    pub fn web_fetch_relevant_domains(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |entry: &AllowlistEntry| {
            if let Some(domain) = entry.domain.as_deref() {
                let trimmed = domain.trim();
                if !trimmed.is_empty()
                    && !out.iter().any(|d: &String| d == trimmed)
                    // defuddle.md is a relay, not a fetch target.
                    && trimmed != "defuddle.md"
                {
                    out.push(trimmed.to_string());
                }
            }
        };
        for entry in &self.search.web {
            push(entry);
        }
        for entry in &self.search.specialized {
            push(entry);
        }
        for entry in &self.web_crawl {
            push(entry);
        }
        for entry in &self.mcp_servers {
            push(entry);
        }
        for entry in &self.package_registries {
            push(entry);
        }
        for entry in &self.code_hosting {
            push(entry);
        }
        out
    }

    /// Entries that the allowlist itself flags as `verify = true`. The
    /// agent can refuse to use these (or surface a warning) until the
    /// operator confirms.
    pub fn unverified_entries(&self) -> Vec<&AllowlistEntry> {
        self.iter_entries().filter(|e| e.verify).collect()
    }

    /// All entries across every category, in source-file order. Local
    /// AI providers are excluded (no domain).
    pub fn iter_entries(&self) -> impl Iterator<Item = &AllowlistEntry> {
        self.ai_providers
            .cloud
            .iter()
            .chain(self.search.web.iter())
            .chain(self.search.specialized.iter())
            .chain(self.web_crawl.iter())
            .chain(self.mcp_servers.iter())
            .chain(self.package_registries.iter())
            .chain(self.code_hosting.iter())
            .chain(self.auth.iter())
            .chain(self.dev_infra.iter())
            .chain(self.os_updates.iter())
    }

    /// Total number of allowlist entries across all categories (excluding
    /// local AI providers).
    pub fn entry_count(&self) -> usize {
        self.iter_entries().count()
    }

    /// Pretty-print the allowlist as a one-line per category summary
    /// (e.g. `"ai_providers.cloud: 27, search.web: 11, …"`). Useful in
    /// startup logs so operators can confirm what shipped.
    pub fn category_summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.ai_providers.cloud.is_empty() {
            parts.push(format!("ai_providers.cloud: {}", self.ai_providers.cloud.len()));
        }
        if !self.ai_providers.local.is_empty() {
            parts.push(format!("ai_providers.local: {}", self.ai_providers.local.len()));
        }
        if !self.search.web.is_empty() {
            parts.push(format!("search.web: {}", self.search.web.len()));
        }
        if !self.search.specialized.is_empty() {
            parts.push(format!("search.specialized: {}", self.search.specialized.len()));
        }
        if !self.web_crawl.is_empty() {
            parts.push(format!("web_crawl: {}", self.web_crawl.len()));
        }
        if !self.mcp_servers.is_empty() {
            parts.push(format!("mcp_servers: {}", self.mcp_servers.len()));
        }
        if !self.package_registries.is_empty() {
            parts.push(format!("package_registries: {}", self.package_registries.len()));
        }
        if !self.code_hosting.is_empty() {
            parts.push(format!("code_hosting: {}", self.code_hosting.len()));
        }
        if !self.auth.is_empty() {
            parts.push(format!("auth: {}", self.auth.len()));
        }
        if !self.dev_infra.is_empty() {
            parts.push(format!("dev_infra: {}", self.dev_infra.len()));
        }
        if !self.os_updates.is_empty() {
            parts.push(format!("os_updates: {}", self.os_updates.len()));
        }
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_default_parses_embedded_toml() {
        let list = NetworkAllowlist::load_default();
        // The shipped TOML has 103 entries; verify the broad shape
        // before checking individual hosts.
        assert!(list.entry_count() > 50, "expected many entries, got {}", list.entry_count());
        assert!(list.entry_count() <= 200, "allowlist grew unexpectedly");
    }

    #[test]
    fn load_default_includes_common_dev_hosts() {
        let list = NetworkAllowlist::load_default();
        let domains = list.all_allow_domains_set();
        for host in [
            "github.com",
            "api.github.com",
            "crates.io",
            "registry.npmjs.org",
            "pypi.org",
            "defuddle.md",
            "r.jina.ai",
            "api.tavily.com",
            "api.anthropic.com",
        ] {
            assert!(domains.contains(host), "default allowlist should include {host}; missing");
        }
    }

    #[test]
    fn load_default_preserves_wildcards() {
        let list = NetworkAllowlist::load_default();
        let domains = list.all_allow_domains_set();
        for wildcard in ["*.auth0.com", "*.workers.dev", "*.vercel.app"] {
            assert!(domains.contains(wildcard), "default allowlist should include wildcard {wildcard}");
        }
    }

    #[test]
    fn load_default_flags_unverified_entries() {
        let list = NetworkAllowlist::load_default();
        let unverified: Vec<&str> = list.unverified_entries().iter().filter_map(|e| e.name.as_deref()).collect();
        assert!(
            unverified.iter().any(|n| n.contains("MiMo")),
            "expected MiMo to be flagged verify=true; got {unverified:?}"
        );
    }

    #[test]
    fn category_summary_lists_populated_categories() {
        let list = NetworkAllowlist::load_default();
        let summary = list.category_summary();
        assert!(summary.contains("ai_providers.cloud"));
        assert!(summary.contains("search.web"));
        assert!(summary.contains("code_hosting"));
    }
}
