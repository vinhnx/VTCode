//! `MergeGatewayProvider` behavior: native payload building, streaming, parsing.

use super::*;

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

        // Some Merge routes terminate at Anthropic Bedrock, which rejects
        // `tool_choice: "none"`. Keep tool definitions on the wire even when
        // tools are disabled so the rendered prefix stays cache-stable across
        // recovery turns; only the choice field is omitted. If the route has
        // no tool vendor at all, omit definitions so synthesis can still run.
        let tools_disabled = matches!(request.tool_choice, Some(ToolChoice::None));
        let omit_tools_for_missing_vendor = tools_disabled && self.tool_vendor_known_missing(&request.model);
        if !omit_tools_for_missing_vendor
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

    async fn generate_native(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.prepare_native_request(&mut request);
        LLMProvider::validate_request(self, &request)?;
        // Fail fast only when this request would put tools on the wire and the
        // route has no tool vendor. Recovery (ToolChoice::None) can still
        // synthesize without tools.
        if self.tool_vendor_known_missing(&request.model)
            && native_request_sends_tools(&request, self.tool_vendor_known_missing(&request.model))
        {
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
                return Box::pin(self.generate_native(request)).await;
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
                && native_request_sends_tools(&request, self.tool_vendor_known_missing(&request.model))
                && !(sent_reasoning && is_reasoning_capability_rejection(&body))
            {
                self.mark_tool_vendor_missing(&request.model);
            }
            return Err(merge_request_error(status, &body));
        }

        let json: Value = response.json().await.map_err(|e| format_parse_error("Merge Gateway", &e))?;
        Self::parse_native_response_payload(json, request.model)
    }

    async fn stream_native_normalized(&self, mut request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        self.prepare_native_request(&mut request);
        LLMProvider::validate_request(self, &request)?;
        if self.tool_vendor_known_missing(&request.model)
            && native_request_sends_tools(&request, self.tool_vendor_known_missing(&request.model))
        {
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
            // naming exactly what is missing — is surfaced.
            if is_capability_unavailable(status, &body) {
                tracing::warn!(
                    model = %request.model,
                    status = %status,
                    "Merge Gateway rejected streaming capabilities; retrying once without streaming"
                );
                request.stream = false;
                let fallback = self.generate_native(request).await?;
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
                return Box::pin(self.stream_native_normalized(request)).await;
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

    fn handle_native_stream_payload(
        raw_event: &str,
        payload_text: &str,
        state: &mut MergeStreamState,
    ) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let payload: Value = serde_json::from_str(payload_text)
            .map_err(|e| provider_error(format!("Invalid Merge Gateway SSE payload: {e}")))?;
        let event_name = Self::merge_event_name(raw_event, &payload);
        let data = payload.get("data").cloned().unwrap_or(payload.clone());
        let mut events = Vec::new();

        let fallback_restart = data
            .get("fallback_restart")
            .or_else(|| payload.get("fallback_restart"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if fallback_restart {
            let model = data.get("model").or_else(|| payload.get("model")).and_then(Value::as_str);
            state.reset_for_fallback_restart(model);
            return Ok(events);
        }

        if event_name.is_empty() {
            if data.get("response").is_some()
                || data.get("output").is_some()
                || data.get("content").is_some()
                || data.get("text").is_some()
            {
                let response_value = data.get("response").cloned().unwrap_or_else(|| data.clone());
                let response = Self::parse_native_response_payload(response_value, state.model.clone())?;
                state.native_snapshot = None;
                events.extend(state.apply_native_snapshot(response)?);
                state.done = true;
            }
            return Ok(events);
        }

        match event_name.as_str() {
            // Native `/v1/responses` streams cumulative snapshots. The
            // OpenAI-compatible `/v1/openai/responses` surface uses the
            // delta-oriented events handled below, so keep both protocols
            // explicit at this boundary.
            "response.stream" | "response.done" => {
                let response_value = data.get("response").cloned().or_else(|| {
                    if data.get("output").is_some() {
                        Some(data.clone())
                    } else {
                        None
                    }
                });

                let response = match response_value {
                    Some(response_value) => Self::parse_native_response_payload(response_value, state.model.clone())?,
                    None if event_name == "response.done" => state
                        .native_snapshot
                        .take()
                        .ok_or_else(|| provider_error("Merge Gateway response.done event was missing the response"))?,
                    None => return Ok(events),
                };

                if event_name == "response.done" {
                    state.native_snapshot = None;
                    events.extend(state.apply_native_snapshot(response)?);
                    state.done = true;
                } else {
                    if let Some(previous) = state.native_snapshot.take() {
                        if Self::native_snapshots_are_cumulative(&previous, &response) {
                            if !state.native_snapshot_streaming {
                                events.extend(state.apply_native_snapshot(previous)?);
                                state.native_snapshot_streaming = true;
                            }
                            events.extend(state.apply_native_snapshot(response.clone())?);
                        } else if state.native_snapshot_streaming {
                            // A provider replacement cannot retract already-emitted
                            // deltas, but it must not leak the old snapshot into the
                            // terminal response or subsequent deltas.
                            state.reset_native_snapshot_accumulator();
                        }
                    }
                    state.remember_native_snapshot(response);
                }
            }
            "response.output_text.delta" | "response.output_text.done" => {
                if let Some(fragment) = Self::stream_text_fragment(&data) {
                    if let Some(delta) = state.apply_text_fragment(fragment, event_name.ends_with(".done")) {
                        if !delta.is_empty() {
                            events.push(NormalizedStreamEvent::TextDelta { delta });
                        }
                    }
                }
            }
            "response.output_item.added" => {
                if let Some(item) = Self::stream_output_item(&data) {
                    events.extend(state.record_tool_use_item(item, false)?);
                }
            }
            "response.output_item.done" => {
                if let Some(item) = Self::stream_output_item(&data) {
                    events.extend(state.record_tool_use_item(item, true)?);
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some((call_id, name, fragment)) = Self::stream_tool_arguments(&data) {
                    events.extend(state.record_tool_arguments(call_id, name, fragment, false)?);
                }
            }
            "response.function_call_arguments.done" => {
                if let Some((call_id, name, fragment)) = Self::stream_tool_arguments(&data) {
                    events.extend(state.record_tool_arguments(call_id, name, fragment, true)?);
                }
            }
            "response.usage" => {
                let usage_value = data.get("usage").unwrap_or(&data);
                state.usage = Self::parse_native_usage(Some(usage_value));
            }
            "response.completed" | "response.incomplete" => {
                if event_name == "response.incomplete" {
                    state.incomplete = true;
                }
                let response_value = data.get("response").cloned().or_else(|| {
                    if data.get("output").is_some() {
                        Some(data.clone())
                    } else {
                        None
                    }
                });

                if let Some(response_value) = response_value {
                    match Self::parse_native_response_payload(response_value, state.model.clone()) {
                        Ok(response) => {
                            state.final_response = Some(response);
                            state.done = true;
                        }
                        Err(err) if state.has_streamed_output() => {
                            state.request_id = Self::extract_request_id(&data);
                            state.done = true;
                            if matches!(event_name.as_str(), "response.completed") {
                                return Err(err);
                            }
                        }
                        Err(err) => return Err(err),
                    }
                } else if state.has_streamed_output() {
                    state.request_id = Self::extract_request_id(&data);
                    state.done = true;
                }
            }
            "response.failed" | "response.error" | "error" => {
                let message = data
                    .get("error")
                    .and_then(Value::as_object)
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .or_else(|| data.get("error").and_then(Value::as_str))
                    .or_else(|| data.get("message").and_then(Value::as_str))
                    .unwrap_or("Merge Gateway stream failed")
                    .to_string();
                return Err(provider_error(message));
            }
            _ => {}
        }

        if state.done && state.final_response.is_none() && !state.has_streamed_output() {
            state.final_response = Some(state.synthesize_response());
        }

        Ok(events)
    }

    fn merge_event_name(raw_event: &str, payload: &Value) -> String {
        for line in raw_event.lines() {
            if let Some(event) = line.strip_prefix("event:") {
                return event.trim().to_string();
            }
        }
        if let Some(event) = payload.get("event").and_then(Value::as_str) {
            return event.to_string();
        }
        if let Some(event) = payload.get("type").and_then(Value::as_str) {
            return event.to_string();
        }
        if let Some(event) = payload.get("object").and_then(Value::as_str)
            && (event.starts_with("response.") || event == "error")
        {
            return event.to_string();
        }
        String::new()
    }

    fn native_snapshots_are_cumulative(previous: &LLMResponse, current: &LLMResponse) -> bool {
        let content_is_cumulative = match (previous.content.as_deref(), current.content.as_deref()) {
            (Some(previous), Some(current)) => current.starts_with(previous),
            (Some(_), None) => false,
            (None, _) => true,
        };
        if !content_is_cumulative {
            return false;
        }

        let previous_calls = previous.tool_calls.as_deref().unwrap_or_default();
        let current_calls = current.tool_calls.as_deref().unwrap_or_default();
        previous_calls.iter().all(|previous_call| {
            let Some(current_call) = current_calls.iter().find(|call| call.id == previous_call.id) else {
                return false;
            };

            let previous_name = previous_call.tool_name().unwrap_or_default();
            let current_name = current_call.tool_name().unwrap_or_default();
            if previous_name != current_name {
                return false;
            }

            let previous_arguments = previous_call.raw_input().unwrap_or_default();
            previous_arguments.is_empty()
                || previous_arguments == "{}"
                || current_call.raw_input().unwrap_or_default().starts_with(previous_arguments)
        })
    }

    fn stream_text_fragment(data: &Value) -> Option<String> {
        data.get("delta")
            .or_else(|| data.get("output_text"))
            .or_else(|| data.get("text"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    }

    fn stream_output_item(data: &Value) -> Option<&Value> {
        data.get("output_item").or_else(|| data.get("item")).or_else(|| {
            if data.get("type").is_some() || data.get("content").is_some() {
                Some(data)
            } else {
                None
            }
        })
    }

    fn stream_tool_arguments(data: &Value) -> Option<(String, Option<String>, String)> {
        let call_id = data
            .get("call_id")
            .or_else(|| data.get("item_id"))
            .or_else(|| data.get("tool_use_id"))
            .or_else(|| data.get("tool_call_id"))
            .or_else(|| data.get("id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)?;
        let name = data.get("name").and_then(Value::as_str).map(ToOwned::to_owned).or_else(|| {
            Self::stream_output_item(data)
                .and_then(|item| item.get("name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        });
        let fragment = data
            .get("delta")
            .or_else(|| data.get("arguments"))
            .or_else(|| data.get("input"))
            .and_then(|value| Self::value_to_arguments(value).ok())
            .unwrap_or_default();
        Some((call_id, name, fragment))
    }

    fn value_to_arguments(value: &Value) -> Result<String, LLMError> {
        match value {
            Value::String(text) => Ok(text.clone()),
            _ => serde_json::to_string(value)
                .map_err(|e| provider_error(format!("Failed to serialize Merge Gateway tool arguments: {e}"))),
        }
    }

    fn parse_native_response_payload(json: Value, model: String) -> Result<LLMResponse, LLMError> {
        let mut content = String::new();
        let mut tool_calls = Vec::new();
        let mut finish_reason = FinishReason::Stop;

        if let Some(output) = json.get("output").and_then(Value::as_array) {
            for item in output {
                Self::parse_native_output_item(item, &mut content, &mut tool_calls, &mut finish_reason)?;
            }
        } else if json.get("content").is_some() || json.get("text").is_some() || json.get("tool_use").is_some() {
            Self::parse_native_output_item(&json, &mut content, &mut tool_calls, &mut finish_reason)?;
        } else {
            return Err(provider_error("Invalid response from Merge Gateway: missing output"));
        }

        if matches!(finish_reason, FinishReason::Stop) && !tool_calls.is_empty() {
            finish_reason = FinishReason::ToolCalls;
        }

        Ok(LLMResponse {
            content: if content.is_empty() { None } else { Some(content) },
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            model: json
                .get("model")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(model),
            usage: Self::parse_native_usage(json.get("usage")),
            finish_reason,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: Self::extract_request_id(&json),
            organization_id: None,
            compaction: None,
        })
    }

    fn parse_native_output_item(
        item: &Value,
        content: &mut String,
        tool_calls: &mut Vec<ToolCall>,
        finish_reason: &mut FinishReason,
    ) -> Result<(), LLMError> {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        match item_type {
            "message" | "" => {
                if let Some(role) = item.get("role").and_then(Value::as_str)
                    && role != "assistant"
                {
                    return Ok(());
                }

                if let Some(reason) = item.get("finish_reason").and_then(Value::as_str) {
                    *finish_reason = Self::map_finish_reason(reason);
                }

                if let Some(content_value) = item.get("content") {
                    Self::parse_native_content_value(content_value, content, tool_calls, finish_reason)?;
                } else {
                    Self::parse_native_content_value(item, content, tool_calls, finish_reason)?;
                }
            }
            "tool_use" | "function_call" => {
                if let Some(tool_call) = Self::tool_call_from_value(item)? {
                    tool_calls.push(tool_call);
                    if matches!(finish_reason, FinishReason::Stop) {
                        *finish_reason = FinishReason::ToolCalls;
                    }
                }
            }
            "refusal" => {
                if let Some(text) = item.get("refusal").and_then(Value::as_str) {
                    content.push_str(text);
                    *finish_reason = FinishReason::Refusal;
                }
            }
            _ => {
                Self::parse_native_content_value(item, content, tool_calls, finish_reason)?;
            }
        }
        Ok(())
    }

    fn parse_native_content_value(
        value: &Value,
        content: &mut String,
        tool_calls: &mut Vec<ToolCall>,
        finish_reason: &mut FinishReason,
    ) -> Result<(), LLMError> {
        match value {
            Value::String(text) => content.push_str(text),
            Value::Array(parts) => {
                for part in parts {
                    Self::parse_native_content_part(part, content, tool_calls, finish_reason)?;
                }
            }
            Value::Object(_) => {
                Self::parse_native_content_part(value, content, tool_calls, finish_reason)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_native_content_part(
        part: &Value,
        content: &mut String,
        tool_calls: &mut Vec<ToolCall>,
        finish_reason: &mut FinishReason,
    ) -> Result<(), LLMError> {
        let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
        match part_type {
            "text" => {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    content.push_str(text);
                } else if let Some(text) = part.get("output_text").and_then(Value::as_str) {
                    content.push_str(text);
                }
            }
            "tool_use" | "function_call" => {
                if let Some(tool_call) = Self::tool_call_from_value(part)? {
                    tool_calls.push(tool_call);
                    *finish_reason = FinishReason::ToolCalls;
                }
            }
            "refusal" => {
                if let Some(text) = part.get("refusal").and_then(Value::as_str) {
                    content.push_str(text);
                    *finish_reason = FinishReason::Refusal;
                }
            }
            _ => {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    content.push_str(text);
                } else if let Some(text) = part.get("output_text").and_then(Value::as_str) {
                    content.push_str(text);
                } else if let Some(text) = part.as_str() {
                    content.push_str(text);
                }

                if let Some(inner) = part.get("content") {
                    Self::parse_native_content_value(inner, content, tool_calls, finish_reason)?;
                }

                if let Some(tool_call) = Self::tool_call_from_value(part)? {
                    tool_calls.push(tool_call);
                    *finish_reason = FinishReason::ToolCalls;
                }
            }
        }
        Ok(())
    }

    fn tool_call_from_value(value: &Value) -> Result<Option<ToolCall>, LLMError> {
        let id = value
            .get("id")
            .or_else(|| value.get("call_id"))
            .or_else(|| value.get("tool_use_id"))
            .or_else(|| value.get("tool_call_id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(generate_tool_call_id);

        let name = value
            .get("name")
            .or_else(|| value.get("function").and_then(|func| func.get("name")))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            return Ok(None);
        }

        let arguments = match value.get("input").or_else(|| value.get("arguments")) {
            Some(Value::Null) | None => "{}".to_string(),
            Some(input) => Self::value_to_arguments(input)?,
        };

        Ok(Some(ToolCall::function(id, name, arguments)))
    }

    fn map_finish_reason(reason: &str) -> FinishReason {
        match reason.to_ascii_lowercase().as_str() {
            "stop" => FinishReason::Stop,
            "length" | "max_tokens" => FinishReason::Length,
            "tool_use" | "tool_calls" => FinishReason::ToolCalls,
            "content_filter" => FinishReason::ContentFilter,
            "refusal" => FinishReason::Refusal,
            "error" => FinishReason::Error("Merge Gateway reported an error".to_string()),
            _ => FinishReason::Stop,
        }
    }

    pub(crate) fn parse_native_usage(value: Option<&Value>) -> Option<Usage> {
        let usage = value?;
        let prompt_tokens = usage_u32_from_keys(usage, RESPONSES_PROMPT_TOKEN_KEYS);
        let completion_tokens = usage_u32_from_keys(usage, RESPONSES_COMPLETION_TOKEN_KEYS);
        let total_tokens = usage
            .get("total_tokens")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or_else(|| prompt_tokens.saturating_add(completion_tokens));

        // Surface gateway cache signals instead of hard-coding None. The
        // shared Responses helpers cover OpenAI-style
        // `input_tokens_details.cached_tokens` /
        // `prompt_tokens_details.cached_tokens` / `prompt_cache_hit_tokens` /
        // `cached_tokens` shapes; the explicit fallbacks cover gateway
        // variants (`cache_read_tokens`, OpenRouter-style
        // `prompt_cache_read_tokens`, Anthropic-style
        // `cache_read_input_tokens`).
        let openai_cached_prompt_tokens = parse_cached_prompt_tokens_from_usage(usage, true);
        let cache_creation_tokens = parse_cache_write_tokens_from_usage(usage, true)
            .or_else(|| {
                usage
                    .get("prompt_cache_write_tokens")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
            })
            .or_else(|| {
                usage
                    .get("cache_creation_input_tokens")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
            });
        let cache_read_tokens = usage
            .get("cache_read_tokens")
            .or_else(|| usage.get("prompt_cache_read_tokens"))
            .or_else(|| usage.get("cache_read_input_tokens"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .or(openai_cached_prompt_tokens);
        // Merge native Responses often reports only Anthropic-style cache-read
        // fields. Surface that count in `cached_prompt_tokens` so trajectory
        // and status metrics are not misleading zeros when the gateway cached.
        // Cost accounting already prefers `cache_read_tokens` via
        // `Usage::billable_totals`; this mapping is observability only.
        let cached_prompt_tokens = openai_cached_prompt_tokens.or(cache_read_tokens);

        Some(Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
            cached_prompt_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            iterations: None,
        })
    }

    fn extract_request_id(value: &Value) -> Option<String> {
        value
            .get("id")
            .or_else(|| value.get("request_id"))
            .or_else(|| value.get("provider_request_id"))
            .or_else(|| value.get("routing").and_then(|routing| routing.get("request_id")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
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

    fn supports_non_streaming(&self, _model: &str) -> bool {
        // The native `/responses` surface services non-streaming generation on
        // every route, and the harness's stream-timeout retry falls back to
        // non-streaming only when this capability is advertised. Pin it here so
        // the fallback cannot silently disappear with a trait-default change.
        true
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
                | models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_5_5
                | models::merge_gateway::ANTHROPIC_CLAUDE_FABLE_5_1
                | models::merge_gateway::GOOGLE_GEMINI_3_6_FLASH
                | models::merge_gateway::GOOGLE_GEMINI_3_7_FLASH
                | models::merge_gateway::GOOGLE_GEMINI_3_8_FLASH
                | models::merge_gateway::MOONSHOT_KIMI_K3
                | models::merge_gateway::ZAI_GLM_5_3_FLASH
                | models::merge_gateway::OPENAI_GPT_5_5
                | models::merge_gateway::OPENAI_GPT_5_6_LUNA
                | models::merge_gateway::OPENAI_GPT_5_6_SOL
                | models::merge_gateway::OPENAI_GPT_5_6_TERRA
                | models::merge_gateway::OPENAI_GPT_6_ASTRA
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
