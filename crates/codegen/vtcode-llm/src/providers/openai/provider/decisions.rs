//! Direct OpenAI API-key Decisions transport. Never discovers another credential.

use super::super::backend_setup::OpenAIBackendKind;
use super::OpenAIProvider;
use crate::provider::{ChoiceDecisionRequest, ChoiceDecisionResponse, LLMError};

impl OpenAIProvider {
    pub(super) fn decisions_eligible(&self) -> bool {
        self.provider_key_override.is_none()
            && self.custom_provider_auth.is_none()
            && self.api_format_override.is_none()
            && self.openai_chatgpt_auth.is_none()
            && matches!(self.backend_setup.kind(), OpenAIBackendKind::ApiKey)
            && !self.api_key.trim().is_empty()
            && standard_decisions_base(&self.base_url)
    }

    pub(super) async fn decide_choice_request(
        &self,
        request: ChoiceDecisionRequest,
    ) -> Result<ChoiceDecisionResponse, LLMError> {
        if !self.decisions_eligible() {
            return Err(decisions_error("Decisions requires a direct OpenAI API-key session at the standard endpoint"));
        }
        send_choice(
            self.decisions_client
                .as_ref()
                .map_err(|_build_error| decisions_error("Decisions client creation failed"))?,
            &format!("{}/decisions", self.base_url.trim_end_matches('/')),
            &self.api_key,
            request,
        )
        .await
    }
}

fn standard_decisions_base(base_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(base_url) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("api.openai.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.path(), "/v1" | "/v1/")
        && url.query().is_none()
        && url.fragment().is_none()
}

fn decisions_error(message: &str) -> LLMError {
    LLMError::Provider { message: message.to_owned(), metadata: None }
}

async fn send_choice(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    request: ChoiceDecisionRequest,
) -> Result<ChoiceDecisionResponse, LLMError> {
    let payload = request.payload()?;
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|_transport_error| decisions_error("Decisions transport failed"))?;
    if !response.status().is_success() {
        // Provider bodies may echo the input. Keep only the HTTP status.
        tracing::debug!(status = response.status().as_u16(), "Decisions request failed");
        return Err(decisions_error(&format!("Decisions HTTP {}", response.status().as_u16())));
    }
    // A single choice answer is small. Bound success bodies as well as errors.
    const MAX_RESPONSE_BYTES: usize = 16 * 1024;
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_read_error| decisions_error("Decisions response read failed"))?
    {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
            return Err(decisions_error("Decisions response exceeds the size limit"));
        }
        body.extend_from_slice(&chunk);
    }
    let payload =
        serde_json::from_slice(&body).map_err(|_parse_error| decisions_error("Malformed Decisions response"))?;
    Ok(ChoiceDecisionResponse::from_payload(&payload, &request))
}

#[cfg(test)]
mod tests;
