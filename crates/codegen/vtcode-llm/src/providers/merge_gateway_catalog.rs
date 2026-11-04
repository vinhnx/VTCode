//! Merge Gateway authenticated model catalog discovery.
//!
//! This module fetches the native `/v1/models` catalog, paginates cursor-based
//! results, and normalizes vendor-specific capability data into vtcode-core
//! friendly snapshot structures.

use anyhow::{Context, Result, bail, ensure};
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, IF_NONE_MATCH},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::Duration;
use vtcode_commons::tool_types::CompactStr;
use vtcode_config::TimeoutsConfig;

use super::merge_gateway_contract::{
    MergeAvailabilityStatus, MergeInputModality, MergeModelCatalogResponse, MergeModelRecord, MergeModelsListQuery,
    MergeServiceTier, MergeVendorModelInfo,
};

const CATALOG_PAGE_LIMIT: u32 = 500;
const MAX_CATALOG_PAGES: usize = 1_000;
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 120;

/// Normalized catalog availability for vtcode-core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeCatalogAvailability {
    Available,
    Deprecated,
    Unknown,
}

impl From<MergeAvailabilityStatus> for MergeCatalogAvailability {
    fn from(value: MergeAvailabilityStatus) -> Self {
        match value {
            MergeAvailabilityStatus::Available => Self::Available,
            MergeAvailabilityStatus::Deprecated => Self::Deprecated,
            MergeAvailabilityStatus::Unknown => Self::Unknown,
        }
    }
}

/// Normalized service tiers for the catalog snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeCatalogServiceTier {
    Standard,
    Flex,
    Priority,
    Unknown,
}

impl From<MergeServiceTier> for MergeCatalogServiceTier {
    fn from(value: MergeServiceTier) -> Self {
        match value {
            MergeServiceTier::Standard => Self::Standard,
            MergeServiceTier::Flex => Self::Flex,
            MergeServiceTier::Priority => Self::Priority,
            MergeServiceTier::Unknown => Self::Unknown,
        }
    }
}

/// Optional Merge catalog filters preserved across pagination.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeCatalogFilters {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub vendor: Option<String>,
}

/// Normalized Merge catalog model metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeCatalogModel {
    pub model: String,
    pub provider: String,
    pub display_name: Option<String>,
    pub availability: MergeCatalogAvailability,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub supports_tool_use: bool,
    pub supports_streaming: bool,
    pub supports_vision: bool,
    pub supports_structured_output: bool,
    pub service_tiers: Vec<MergeCatalogServiceTier>,
    pub supports_reasoning: bool,
    pub reasoning_disable_supported: bool,
    pub reasoning_controls: Vec<String>,
}

/// Full normalized snapshot of the Merge catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeCatalogSnapshot {
    pub models: Vec<MergeCatalogModel>,
    pub etag: Option<String>,
}

/// Authenticated Merge Gateway `/v1/models` discovery client.
#[derive(Clone)]
pub struct MergeGatewayCatalogClient {
    api_key: String,
    catalog_base_url: String,
    http_client: Client,
}

impl MergeGatewayCatalogClient {
    /// Creates a client using an injected HTTP client.
    pub fn try_with_client(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        http_client: Client,
    ) -> Result<Self> {
        Self::try_from_parts(api_key.into(), base_url.into(), http_client)
    }

    /// Creates a client using a timeout-aware HTTP client derived from `TimeoutsConfig`.
    pub fn try_with_timeouts(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        timeouts: Option<TimeoutsConfig>,
    ) -> Result<Self> {
        let http_client = build_http_client(timeouts.as_ref())?;
        Self::try_from_parts(api_key.into(), base_url.into(), http_client)
    }

    /// Fetches the catalog snapshot. Returns `Ok(None)` when Merge replies `304 Not Modified`
    /// and a non-empty `etag` validator was supplied.
    pub async fn fetch_snapshot(
        &self,
        filters: &MergeCatalogFilters,
        etag: Option<&str>,
    ) -> Result<Option<MergeCatalogSnapshot>> {
        let etag = normalize_optional_value(etag);
        let mut cursor: Option<String> = None;
        let mut seen_cursors: HashSet<String> = HashSet::new();
        let mut models = Vec::new();
        let mut snapshot_etag: Option<String> = None;
        let mut page_index: usize = 1;

        loop {
            ensure!(
                page_index <= MAX_CATALOG_PAGES,
                "Merge Gateway catalog exceeded the maximum of {MAX_CATALOG_PAGES} pages"
            );
            let query = build_wire_query(filters, cursor.as_deref());
            let mut request = self
                .http_client
                .get(self.models_endpoint_url())
                .bearer_auth(&self.api_key)
                .header(ACCEPT, "application/json")
                .query(&query);

            if page_index == 1 {
                if let Some(etag) = etag.as_deref() {
                    request = request.header(IF_NONE_MATCH, etag);
                }
            }

            let response = request.send().await.with_context(|| {
                format!(
                    "Merge Gateway catalog request failed on page {page_index} ({})",
                    describe_filters(filters, cursor.as_deref())
                )
            })?;

            if response.status() == StatusCode::NOT_MODIFIED {
                if page_index == 1 && etag.is_some() {
                    return Ok(None);
                }

                bail!(
                    "Merge Gateway catalog returned 304 Not Modified on page {page_index} without a usable If-None-Match validator"
                );
            }

            if !response.status().is_success() {
                bail!(
                    "Merge Gateway catalog request failed on page {page_index} with HTTP {} ({})",
                    response.status(),
                    describe_filters(filters, cursor.as_deref())
                );
            }

            if snapshot_etag.is_none() {
                snapshot_etag = header_value_to_string(response.headers().get(reqwest::header::ETAG));
            }

            let raw: Value = response
                .json()
                .await
                .with_context(|| format!("Merge Gateway catalog response on page {page_index} was not valid JSON"))?;
            validate_catalog_envelope(&raw, page_index)?;
            let page: MergeModelCatalogResponse = serde_json::from_value(raw)
                .with_context(|| format!("Merge Gateway catalog response on page {page_index} was malformed"))?;

            models.extend(page.data.into_iter().map(normalize_catalog_model));

            if page.has_more {
                let next_cursor = page
                    .next_cursor
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Merge Gateway catalog page {page_index} set has_more=true without a non-empty next_cursor"
                        )
                    })?;

                if !seen_cursors.insert(next_cursor.clone()) {
                    bail!("Merge Gateway catalog repeated pagination cursor `{next_cursor}` on page {page_index}");
                }

                cursor = Some(next_cursor);
                page_index += 1;
                continue;
            }

            return Ok(Some(MergeCatalogSnapshot { models, etag: snapshot_etag }));
        }
    }

    fn try_from_parts(api_key: String, base_url: String, http_client: Client) -> Result<Self> {
        let api_key = api_key.trim().to_owned();
        ensure!(!api_key.is_empty(), "Merge Gateway API key cannot be empty");

        let catalog_base_url = normalize_catalog_base_url(&base_url)?;
        Ok(Self { api_key, catalog_base_url, http_client })
    }

    fn models_endpoint_url(&self) -> String {
        format!("{}/models", self.catalog_base_url)
    }
}

fn build_http_client(timeouts: Option<&TimeoutsConfig>) -> Result<Client> {
    let request_timeout = timeouts
        .and_then(|config| config.ceiling_duration(config.default_ceiling_seconds))
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS));
    // Catalog pagination runs many sequential requests; honoring the user's
    // connect budget here is what makes a slow link usable instead of flaky.
    let connect_timeout = timeouts
        .and_then(|config| config.ceiling_duration(config.http_connect_seconds))
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS));

    Client::builder()
        .timeout(request_timeout)
        .connect_timeout(connect_timeout)
        .build()
        .context("failed to build Merge Gateway catalog HTTP client")
}

fn build_wire_query(filters: &MergeCatalogFilters, cursor: Option<&str>) -> MergeModelsListQuery {
    MergeModelsListQuery {
        model: normalize_optional_value(filters.model.as_deref()),
        provider: normalize_optional_value(filters.provider.as_deref()),
        vendor: normalize_optional_value(filters.vendor.as_deref()),
        cursor: normalize_optional_value(cursor),
        limit: Some(CATALOG_PAGE_LIMIT),
    }
}

fn normalize_optional_value(value: Option<&str>) -> Option<CompactStr> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(CompactStr::from)
}

fn normalize_catalog_base_url(base_url: &str) -> Result<String> {
    let mut normalized = base_url.trim().trim_end_matches('/').to_owned();
    ensure!(!normalized.is_empty(), "Merge Gateway base URL cannot be empty");

    loop {
        let mut changed = false;
        for suffix in ["/chat/completions", "/responses", "/openai", "/models"] {
            if let Some(stripped) = normalized.strip_suffix(suffix) {
                normalized = stripped.trim_end_matches('/').to_owned();
                changed = true;
                break;
            }
        }

        if !changed {
            break;
        }
    }

    if !normalized.ends_with("/v1") && !normalized.contains("/v1/") {
        normalized.push_str("/v1");
    }

    Ok(normalized)
}

fn validate_catalog_envelope(raw: &Value, page_index: usize) -> Result<()> {
    let object = raw.as_object().and_then(|map| map.get("object")).and_then(Value::as_str);
    ensure!(
        object == Some("list"),
        "Merge Gateway catalog envelope on page {page_index} must set object=\"list\""
    );
    Ok(())
}

fn header_value_to_string(value: Option<&reqwest::header::HeaderValue>) -> Option<String> {
    value
        .and_then(|header| header.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn describe_filters(filters: &MergeCatalogFilters, cursor: Option<&str>) -> String {
    format!(
        "cursor={cursor:?}, model={:?}, provider={:?}, vendor={:?}",
        filters.model.as_deref(),
        filters.provider.as_deref(),
        filters.vendor.as_deref(),
    )
}

fn normalize_catalog_model(record: MergeModelRecord) -> MergeCatalogModel {
    let context_window = aggregate_min_context_window(&record.vendors);
    let max_output_tokens = aggregate_min_max_output_tokens(&record.vendors);
    let supports_tool_use = all_vendors_support(&record.vendors, |vendor| vendor.capabilities.supports_tool_calling);
    let supports_streaming = all_vendors_support(&record.vendors, |vendor| vendor.capabilities.streaming);
    let supports_vision = all_vendors_support(&record.vendors, |vendor| {
        vendor
            .capabilities
            .input
            .iter()
            .any(|modality| matches!(modality, MergeInputModality::Image))
    });
    let supports_structured_output =
        all_vendors_support(&record.vendors, |vendor| vendor.capabilities.supports_structured_outputs);
    let service_tiers = aggregate_service_tiers(&record.vendors);
    let supports_reasoning = all_vendors_support(&record.vendors, |vendor| vendor.capabilities.supports_reasoning());
    let reasoning_disable_supported = !record.vendors.is_empty()
        && record
            .vendors
            .values()
            .all(|vendor| vendor.capabilities.reasoning_disable_supported());
    let reasoning_controls = aggregate_reasoning_controls(&record.vendors);

    MergeCatalogModel {
        model: record.model.to_string(),
        provider: record.provider.to_string(),
        display_name: record.display_name.map(|value| value.to_string()),
        availability: record.availability_status.into(),
        context_window,
        max_output_tokens,
        supports_tool_use,
        supports_streaming,
        supports_vision,
        supports_structured_output,
        service_tiers,
        supports_reasoning,
        reasoning_disable_supported,
        reasoning_controls,
    }
}

fn aggregate_min_context_window(vendors: &BTreeMap<CompactStr, MergeVendorModelInfo>) -> Option<u32> {
    vendors.values().filter_map(|vendor| vendor.context_window).min()
}

fn aggregate_min_max_output_tokens(vendors: &BTreeMap<CompactStr, MergeVendorModelInfo>) -> Option<u32> {
    vendors.values().filter_map(|vendor| vendor.max_output_tokens).min()
}

fn all_vendors_support(
    vendors: &BTreeMap<CompactStr, MergeVendorModelInfo>,
    predicate: impl Fn(&MergeVendorModelInfo) -> bool,
) -> bool {
    !vendors.is_empty() && vendors.values().all(predicate)
}

fn aggregate_service_tiers(vendors: &BTreeMap<CompactStr, MergeVendorModelInfo>) -> Vec<MergeCatalogServiceTier> {
    let mut intersection: Option<BTreeSet<MergeCatalogServiceTier>> = None;

    for vendor in vendors.values() {
        let tiers: BTreeSet<MergeCatalogServiceTier> = vendor
            .service_tiers
            .iter()
            .copied()
            .map(MergeCatalogServiceTier::from)
            .collect();

        intersection = Some(match intersection {
            None => tiers,
            Some(current) => current.intersection(&tiers).copied().collect(),
        });
    }

    intersection.map(|tiers| tiers.into_iter().collect()).unwrap_or_default()
}

fn aggregate_reasoning_controls(vendors: &BTreeMap<CompactStr, MergeVendorModelInfo>) -> Vec<String> {
    if !all_vendors_support(vendors, |vendor| vendor.capabilities.supports_reasoning()) {
        return Vec::new();
    }

    let mut controls = BTreeSet::new();
    for vendor in vendors.values() {
        controls.extend(
            vendor
                .capabilities
                .reasoning_controls()
                .into_iter()
                .map(|control| control.to_string()),
        );
    }
    controls.into_iter().collect()
}

#[cfg(test)]
mod tests;
