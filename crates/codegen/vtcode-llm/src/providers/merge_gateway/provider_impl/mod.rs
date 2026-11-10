//! `MergeGatewayProvider` behavior: native payload building, streaming, parsing.

use super::*;

mod parsing;
mod streaming;

impl MergeGatewayProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::merge_gateway::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        let timeouts = TimeoutsConfig::default();
        let http_client = HttpClientFactory::for_llm(&timeouts);
        Self::from_runtime_parts(api_key, model, http_client, urls::MERGE_GATEWAY_NATIVE_API_BASE.to_string())
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: HttpClient,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self::from_runtime_parts(api_key, model, http_client, base_url)
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key = <MergeGatewaySpec as OpenAiCompatSpec>::resolve_api_key(api_key);
        let model = resolve_model(model, models::merge_gateway::DEFAULT_MODEL);
        let base_url =
            override_base_url(urls::MERGE_GATEWAY_NATIVE_API_BASE, base_url, Some(env_vars::MERGE_GATEWAY_BASE_URL));
        let timeouts = timeouts.unwrap_or_default();
        let http_client = HttpClientFactory::for_llm(&timeouts);

        let legacy_core = if is_legacy_openai_base_url(&base_url) {
            Some(OpenAiCompatCore::from_config(
                Some(api_key.clone()),
                Some(model.clone()),
                Some(base_url.clone()),
                prompt_cache,
                Some(timeouts),
                model_behavior.clone(),
            ))
        } else {
            None
        };

        Self {
            native: NativeMergeGatewayCore {
                api_key,
                http_client,
                base_url,
                model,
                model_behavior,
            },
            legacy_core,
            no_tool_vendor_cache: Mutex::new(HashSet::new()),
        }
    }

    fn from_runtime_parts(api_key: String, model: String, http_client: HttpClient, base_url: String) -> Self {
        let legacy_core = if is_legacy_openai_base_url(&base_url) {
            Some(OpenAiCompatCore::from_parts(api_key.clone(), model.clone(), http_client.clone(), base_url.clone()))
        } else {
            None
        };

        Self {
            native: NativeMergeGatewayCore {
                api_key,
                http_client,
                base_url,
                model,
                model_behavior: None,
            },
            legacy_core,
            no_tool_vendor_cache: Mutex::new(HashSet::new()),
        }
    }

    fn prepare_native_request(&self, request: &mut LLMRequest) {
        if request.model.trim().is_empty() {
            request.model = self.native.model.clone();
        }
    }

    fn tool_vendor_known_missing(&self, model: &str) -> bool {
        self.no_tool_vendor_cache
            .lock()
            .map(|guard| guard.contains(model))
            .unwrap_or(false)
    }

    pub(crate) fn mark_tool_vendor_missing(&self, model: &str) {
        if let Ok(mut guard) = self.no_tool_vendor_cache.lock() {
            guard.insert(model.to_owned());
        }
    }

    fn no_tool_vendor_cached_error(model: &str) -> LLMError {
        provider_error(format!(
            "Model '{model}' has no vendor serving tools on Merge Gateway (cached from an earlier capability_unavailable rejection). Hint: use default_routing or another model until the route gains a tool-capable vendor."
        ))
    }

    fn responses_url(&self) -> String {
        format!("{}/responses", self.native.base_url.trim_end_matches('/'))
    }

    fn build_native_tools(&self, tools: &[ToolDefinition], model: &str) -> Option<Vec<Value>> {
        let gemini_compatible = model.starts_with("google/gemini-");
        let serialized: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                tool.function.as_ref().map(|func| {
                    let parameters = if gemini_compatible {
                        sanitize_function_parameters(func.parameters.clone())
                    } else {
                        func.parameters.clone()
                    };
                    json!({
                        "type": "function",
                        "name": func.name,
                        "description": func.description,
                        "parameters": parameters,
                    })
                })
            })
            .collect();

        if serialized.is_empty() { None } else { Some(serialized) }
    }

    fn native_tool_use_value(&self, call: &ToolCall) -> Result<Value, LLMError> {
        call.validate()
            .map_err(|err| provider_error(format!("Invalid Merge Gateway tool call: {err}")))?;
        let name = call
            .tool_name()
            .ok_or_else(|| provider_error("Merge Gateway tool call is missing a function name"))?;
        let input = call
            .execution_arguments()
            .map_err(|err| provider_error(format!("Failed to serialize Merge Gateway tool call arguments: {err}")))?;
        Ok(json!({
            "type": "tool_use",
            "id": call.id,
            "name": name,
            "input": input,
        }))
    }

    fn native_message_content_value(&self, message: &Message, model: &str) -> Result<Value, LLMError> {
        let content = serialize_message_content_openai_for_model(message, model);
        if message.tool_calls.as_ref().is_none_or(Vec::is_empty) {
            return Ok(content);
        }

        let mut parts = match content {
            Value::String(text) => {
                if text.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![json!({"type": "text", "text": text})]
                }
            }
            Value::Array(parts) => parts,
            other => vec![other],
        };

        if let Some(tool_calls) = &message.tool_calls {
            for call in tool_calls {
                parts.push(self.native_tool_use_value(call)?);
            }
        }

        Ok(Value::Array(parts))
    }

    fn native_input_item_for_message(&self, message: &Message, model: &str) -> Result<Value, LLMError> {
        if message.role.is_tool_response() {
            let tool_call_id = message
                .tool_call_id
                .clone()
                .ok_or_else(|| provider_error("Merge Gateway tool result messages must include a tool_call_id"))?;
            return Ok(json!({
                "type": "tool_result",
                "tool_use_id": tool_call_id,
                "content": function_output_value_from_message_content(&message.content),
            }));
        }

        Ok(json!({
            "type": "message",
            "role": message.role.as_generic_str(),
            "content": self.native_message_content_value(message, model)?,
        }))
    }

    fn native_stop_sequences(request: &LLMRequest) -> Option<Vec<Value>> {
        let stop: Vec<Value> = request
            .stop_sequences
            .as_ref()
            .into_iter()
            .flatten()
            .filter_map(|stop| {
                let trimmed = stop.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(Value::String(trimmed.to_string()))
                }
            })
            .collect();

        if stop.is_empty() { None } else { Some(stop) }
    }

    pub(crate) fn build_native_payload(&self, request: &LLMRequest, stream: bool) -> Result<Value, LLMError> {
        let mut input = Vec::new();

        if let Some(system) = &request.system_prompt {
            let trimmed = system.trim();
            if !trimmed.is_empty() {
                input.push(json!({
                    "type": "message",
                    "role": "system",
                    "content": trimmed,
                }));
            }
        }

        for message in request.messages.iter() {
            input.push(self.native_input_item_for_message(message, &request.model)?);
        }

        let mut payload = Map::new();
        payload.insert("model".to_owned(), Value::String(request.model.clone()));
        payload.insert("input".to_owned(), Value::Array(input));

        // Bedrock-backed routes reject tool_choice=none, so omit callable
        // definitions when synthesis disables tools. Preserve historical input.
        let tools_disabled = matches!(request.tool_choice, Some(ToolChoice::None));
        if !tools_disabled
            && let Some(tools) = request
                .tools
                .as_ref()
                .and_then(|tools| self.build_native_tools(tools, &request.model))
        {
            payload.insert("tools".to_owned(), Value::Array(tools));
        }

        if let Some(max_tokens) = request.max_tokens {
            payload.insert("max_tokens".to_owned(), json!(max_tokens));
        }
        if let Some(temperature) = request.temperature {
            payload.insert("temperature".to_owned(), json!(crate::providers::common::sampling_param_f64(temperature)));
        }
        if let Some(top_p) = request.top_p {
            payload.insert("top_p".to_owned(), json!(crate::providers::common::sampling_param_f64(top_p)));
        }
        if let Some(stop) = Self::native_stop_sequences(request) {
            payload.insert("stop".to_owned(), Value::Array(stop));
        }
        if !tools_disabled && let Some(choice) = &request.tool_choice {
            payload.insert("tool_choice".to_owned(), choice.to_provider_format("merge-gateway"));
        }
        if let Some(output_format) = &request.output_format {
            payload.insert("response_format".to_owned(), output_format.clone());
        }
        if let Some(service_tier) = request.service_tier.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            match map_openai_service_tier_for_merge(service_tier) {
                Some(mapped) => {
                    payload.insert("service_tier".to_owned(), Value::String(mapped.to_owned()));
                }
                None => {
                    tracing::warn!(
                        model = %request.model,
                        service_tier = %service_tier,
                        "Merge Gateway has no equivalent for service_tier; omitting it and using gateway default routing"
                    );
                }
            }
        }
        if let Some(cache_key) = request
            .prompt_cache_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            payload.insert("prompt_cache_key".to_owned(), Value::String(cache_key.to_owned()));
        }
        if let Some(session_id) = merge_session_identity(request) {
            payload.insert("session_id".to_owned(), Value::String(session_id));
        }
        if stream {
            payload.insert("stream".to_owned(), Value::Bool(true));
        }

        if let Some(effort) = request.reasoning_effort
            && is_active_reasoning_effort(effort)
            && let Some(control) = merge_reasoning_control_for_model(&request.model)
        {
            match control {
                MergeReasoningControl::ReasoningEffort => {
                    payload.insert("reasoning_effort".to_owned(), Value::String(effort.as_str().to_owned()));
                }
                MergeReasoningControl::ThinkingBudget => {
                    if let Some(thinking) = merge_thinking_payload(effort, request.max_tokens) {
                        payload.insert("thinking".to_owned(), thinking);
                    }
                }
            }
        }

        Ok(Value::Object(payload))
    }

    async fn generate_native(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.generate_native_with_streaming_retry(request, true).await
    }

    async fn generate_native_with_streaming_retry(
        &self,
        mut request: LLMRequest,
        allow_streaming_retry: bool,
    ) -> Result<LLMResponse, LLMError> {
        self.prepare_native_request(&mut request);
        LLMProvider::validate_request(self, &request)?;
        // Fail fast only when this request would put tools on the wire and the
        // route has no tool vendor. Recovery (ToolChoice::None) can still
        // synthesize without tools.
        if self.tool_vendor_known_missing(&request.model) && native_request_sends_tools(&request) {
            return Err(Self::no_tool_vendor_cached_error(&request.model));
        }
        let payload = self.build_native_payload(&request, false)?;
        let session_id = merge_session_identity(&request);
        let mut http = self
            .native
            .http_client
            .post(self.responses_url())
            .bearer_auth(&self.native.api_key);
        if let Some(session_id) = session_id.as_deref() {
            http = http.header("X-Session-Id", session_id);
        }
        let response = http
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("Merge Gateway", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = crate::providers::common::read_provider_error_body(response).await;
            // The route accepts the tier vocabulary but is not priced for this
            // tier: retry once without it and serve standard. Terminates:
            // re-entry has no tier so this branch cannot refire.
            if is_merge_tier_pricing_rejection(status, &body) && request.service_tier.is_some() {
                tracing::warn!(
                    model = %request.model,
                    status = %status,
                    "Merge Gateway route is not priced for service_tier; retrying once without it"
                );
                request.service_tier = None;
                return Box::pin(self.generate_native_with_streaming_retry(request, allow_streaming_retry)).await;
            }
            // Some vendors behind the gateway (e.g. `zai/`) only serve
            // streaming requests: retry once via the streaming transport and
            // collect the terminal response. Terminates: the streaming path
            // never falls back to non-streaming when the retry flag is
            // cleared, so this branch cannot refire.
            if allow_streaming_retry && is_streaming_only_rejection(status, &body) {
                tracing::warn!(
                    model = %request.model,
                    status = %status,
                    "Merge Gateway route requires streaming; retrying once with streaming"
                );
                return Box::pin(self.collect_streamed_response(request)).await;
            }
            // Non-streaming tool requests fail here only when no vendor serves
            // tools at all: remember the verdict so later turns fail fast.
            // Streaming-capability misses never reach this point (the stream
            // path downgrades first), and tool-free requests bypass the cache.
            // A rejection naming `reasoning` blames the reasoning+tools
            // combination rather than tools alone, so it must not poison the
            // verdict: a tools-only retry may still route.
            let sent_reasoning = request.reasoning_effort.is_some_and(is_active_reasoning_effort);
            if is_capability_unavailable(status, &body)
                && native_request_sends_tools(&request)
                && !(sent_reasoning && is_reasoning_capability_rejection(&body))
            {
                self.mark_tool_vendor_missing(&request.model);
            }
            return Err(merge_request_error(status, &body));
        }

        let json: Value = response.json().await.map_err(|e| format_parse_error("Merge Gateway", &e))?;
        Self::parse_native_response_payload(json, request.model)
    }

    async fn collect_streamed_response(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut stream = self.stream_native_normalized_inner(request, false).await?;
        let mut completed = None;
        while let Some(event) = stream.next().await {
            match event? {
                NormalizedStreamEvent::Done { response } => {
                    completed = Some(*response);
                    break;
                }
                NormalizedStreamEvent::Usage { .. }
                | NormalizedStreamEvent::TextDelta { .. }
                | NormalizedStreamEvent::ReasoningDelta { .. }
                | NormalizedStreamEvent::ReasoningStage { .. }
                | NormalizedStreamEvent::ToolCallStart { .. }
                | NormalizedStreamEvent::ToolCallDelta { .. } => {}
            }
        }
        completed.ok_or_else(|| provider_error("Merge Gateway streamed fallback ended without a completed response"))
    }

    async fn stream_native_normalized(&self, request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        self.stream_native_normalized_inner(request, true).await
    }

    async fn stream_native_normalized_inner(
        &self,
        mut request: LLMRequest,
        allow_non_streaming_fallback: bool,
    ) -> Result<LLMNormalizedStream, LLMError> {
        self.prepare_native_request(&mut request);
        LLMProvider::validate_request(self, &request)?;
        if self.tool_vendor_known_missing(&request.model) && native_request_sends_tools(&request) {
            return Err(Self::no_tool_vendor_cached_error(&request.model));
        }
        request.stream = true;

        let payload = self.build_native_payload(&request, true)?;
        let session_id = merge_session_identity(&request);
        let mut http = self
            .native
            .http_client
            .post(self.responses_url())
            .bearer_auth(&self.native.api_key);
        if let Some(session_id) = session_id.as_deref() {
            http = http.header("X-Session-Id", session_id);
        }
        let response = http
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("Merge Gateway", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = crate::providers::common::read_provider_error_body(response).await;
            // A streaming request can fail capability routing while a plain
            // non-streaming request with the same tools routes fine (the
            // failing combo is `streaming_tools`, not `tools`). Retry once
            // without streaming; tool definitions are preserved. If the route
            // has no tool vendor at all the retry fails too and its error —
            // naming exactly what is missing — is surfaced. Streaming-only
            // routes (e.g. `zai/`) never take this path: non-streaming is
            // known to fail there, so surface the capability error instead of
            // ping-ponging between transports.
            if allow_non_streaming_fallback
                && is_capability_unavailable(status, &body)
                && !is_streaming_only_model(&request.model)
            {
                tracing::warn!(
                    model = %request.model,
                    status = %status,
                    "Merge Gateway rejected streaming capabilities; retrying once without streaming"
                );
                request.stream = false;
                let fallback = Box::pin(self.generate_native_with_streaming_retry(request, false)).await?;
                let completed = LLMStreamEvent::Completed { response: Box::new(fallback) };
                let stream = try_stream! {
                    for event in completed.into_normalized() {
                        yield event;
                    }
                };
                return Ok(Box::pin(stream));
            }
            // Same pricing fallback as the non-streaming path, preserving
            // streaming: re-entry has no tier so this branch cannot refire.
            if is_merge_tier_pricing_rejection(status, &body) && request.service_tier.is_some() {
                tracing::warn!(
                    model = %request.model,
                    status = %status,
                    "Merge Gateway route is not priced for service_tier; retrying once without it"
                );
                request.service_tier = None;
                return Box::pin(self.stream_native_normalized_inner(request, allow_non_streaming_fallback)).await;
            }
            return Err(merge_request_error(status, &body));
        }

        let model = request.model.clone();
        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut decoder = Utf8StreamDecoder::new();
            let mut buffer: Vec<u8> = Vec::new();
            let mut offset = 0usize;
            let mut state = MergeStreamState::new(model.clone());

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|e| format_network_error("Merge Gateway", &e))?;
                decoder.push_bytes(&chunk, &mut buffer);

                while let Some(raw_event) = next_sse_event(&buffer, &mut offset)
                    .map_err(|error| format_parse_error("Merge Gateway", &error))?
                {

                    let payload_text = match extract_data_payload(raw_event) {
                        Some(payload) => payload.into_owned(),
                        None => {
                            let trimmed = raw_event.trim();
                            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                                trimmed.to_string()
                            } else {
                                continue;
                            }
                        }
                    };

                    if payload_text.trim() == "[DONE]" {
                        state.done = true;
                        break;
                    }

                    let events = Self::handle_native_stream_payload(raw_event, &payload_text, &mut state)?;
                    for event in events {
                        yield event;
                    }

                    if state.done {
                        break;
                    }
                }

                if offset > 0 {
                    buffer.drain(..offset);
                    offset = 0;
                }

                if state.done {
                    break;
                }
            }

            if !state.done {
                Err(provider_error("Merge Gateway stream ended before a terminal response event"))?;
            }

            if let Some(response) = state.native_snapshot.take() {
                for event in state.apply_native_snapshot(response)? {
                    yield event;
                }
            }

            let response = state.finish()?;
            if let Some(usage) = response.usage.clone() {
                yield NormalizedStreamEvent::Usage { usage };
            }
            yield NormalizedStreamEvent::Done { response: Box::new(response) };
        };

        Ok(Box::pin(stream))
    }
}

/// Stable opaque session identity for Merge Gateway cache-aware routing.

#[async_trait]
impl LLMProvider for MergeGatewayProvider {
    fn name(&self) -> &str {
        "merge-gateway"
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if let Some(core) = &self.legacy_core {
            core.prepare(&mut request);
            return core.generate_prepared(request).await;
        }

        self.generate_native(request).await
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        if let Some(core) = &self.legacy_core {
            let mut request = request;
            core.prepare(&mut request);
            LLMProvider::validate_request(self, &request)?;
            request.stream = true;
            return core.stream_prepared(request).await;
        }

        let mut normalized = self.stream_native_normalized(request).await?;
        let stream = try_stream! {
            let mut completed = None;
            while let Some(event) = normalized.next().await {
                match event? {
                    NormalizedStreamEvent::TextDelta { delta } => yield LLMStreamEvent::Token { delta },
                    NormalizedStreamEvent::ReasoningDelta { delta, .. } => yield LLMStreamEvent::Reasoning { delta },
                    NormalizedStreamEvent::ReasoningStage { stage } => yield LLMStreamEvent::ReasoningStage { stage },
                    NormalizedStreamEvent::ToolCallStart { .. }
                    | NormalizedStreamEvent::ToolCallDelta { .. }
                    | NormalizedStreamEvent::Usage { .. } => {}
                    NormalizedStreamEvent::Done { response } => {
                        completed = Some(response);
                        break;
                    }
                }
            }

            if let Some(response) = completed {
                yield LLMStreamEvent::Completed { response };
            }
        };

        Ok(Box::pin(stream))
    }

    async fn stream_normalized(&self, request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        if self.legacy_core.is_some() {
            let mut legacy_stream = self.stream(request).await?;
            let stream = try_stream! {
                while let Some(event) = legacy_stream.next().await {
                    for normalized in event?.into_normalized() {
                        yield normalized;
                    }
                }
            };
            return Ok(Box::pin(stream));
        }

        self.stream_native_normalized(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        models::merge_gateway::SUPPORTED_MODELS
            .iter()
            .map(|model| (*model).to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        validate_request_common(request, "Merge Gateway", "merge-gateway", None)
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, model: &str) -> bool {
        // The native `/responses` surface services non-streaming generation on
        // most routes, and the harness's stream-timeout retry falls back to
        // non-streaming only when this capability is advertised. Streaming-only
        // routes (e.g. `zai/`) reject non-streaming with `streaming_only`, so
        // they must not advertise the fallback: `collect_single_response` then
        // uses the streaming transport directly and the timeout retry cannot
        // re-hit the known-failing mode.
        !is_streaming_only_model(model)
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        false
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        self.native
            .model_behavior
            .as_ref()
            .and_then(|behavior| behavior.model_supports_reasoning)
            .unwrap_or_else(|| merge_reasoning_control_for_model(model).is_some())
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        self.native
            .model_behavior
            .as_ref()
            .and_then(|behavior| behavior.model_supports_reasoning_effort)
            .unwrap_or_else(|| merge_reasoning_control_for_model(model).is_some())
    }

    fn supports_vision(&self, model: &str) -> bool {
        matches!(
            model,
            models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5
                | models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5_5
                | models::merge_gateway::ANTHROPIC_CLAUDE_SONNET_5
                | models::merge_gateway::ANTHROPIC_CLAUDE_SONNET_5_5
                | models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_4_5_20251001
                | models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_5_5
                | models::merge_gateway::ANTHROPIC_CLAUDE_FABLE_5_1
                | models::merge_gateway::ANTHROPIC_CLAUDE_FABLE_5_2
                | models::merge_gateway::GOOGLE_GEMINI_3_6_FLASH
                | models::merge_gateway::GOOGLE_GEMINI_3_7_FLASH
                | models::merge_gateway::GOOGLE_GEMINI_3_8_FLASH
                | models::merge_gateway::GOOGLE_GEMINI_4_ARGON
                | models::merge_gateway::MOONSHOT_KIMI_K3
                | models::merge_gateway::MOONSHOT_KIMI_K3_1_PRO
                | models::merge_gateway::ZAI_GLM_5_3_FLASH
                | models::merge_gateway::OPENAI_GPT_5_5
                | models::merge_gateway::OPENAI_GPT_5_6_LUNA
                | models::merge_gateway::OPENAI_GPT_5_6_SOL
                | models::merge_gateway::OPENAI_GPT_5_6_TERRA
                | models::merge_gateway::OPENAI_GPT_6_ASTRA
                | models::merge_gateway::OPENAI_GPT_6_SOL
                | models::merge_gateway::OPENAI_GPT_6_1_SOL
                | models::merge_gateway::OPENAI_GPT_6_1_TERRA
                | models::merge_gateway::OPENAI_GPT_6_LUNA
                | models::merge_gateway::META_MUSE_SPARK_OPEN
        )
    }

    fn effective_context_size(&self, model: &str) -> usize {
        vtcode_config::models::model_catalog_entry(MergeGatewaySpec::KEY, model)
            .map(|entry| entry.context_window)
            .filter(|capacity| *capacity > 0)
            .unwrap_or(128_000)
    }
}

#[async_trait]
impl crate::client::LLMClient for MergeGatewayProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let request = crate::providers::common::make_default_request(prompt, &self.native.model);
        Ok(<Self as LLMProvider>::generate(self, request).await?)
    }

    fn model_id(&self) -> &str {
        &self.native.model
    }
}
