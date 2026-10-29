use super::*;

#[derive(Clone)]
pub(super) struct ElicitationClientService {
    handler: LoggingClientHandler,
}

impl ElicitationClientService {
    pub(super) fn new(handler: LoggingClientHandler) -> Self {
        Self { handler }
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        context: RequestContext<RoleClient>,
    ) -> Result<crate::McpElicitationResponse, rmcp::ErrorData> {
        let request = restore_context_meta(request, context.meta);
        self.handler.process_elicitation_request(request).await
    }
}

impl Service<RoleClient> for ElicitationClientService {
    async fn handle_request(
        &self,
        request: ServerRequest,
        context: RequestContext<RoleClient>,
    ) -> Result<ClientResult, rmcp::ErrorData> {
        match request {
            ServerRequest::ElicitRequest(request) => {
                let response = self.create_elicitation(request.params, context).await?;
                Ok(ClientResult::CustomResult(elicitation_response_result(response)?))
            }
            request => {
                <LoggingClientHandler as Service<RoleClient>>::handle_request(&self.handler, request, context).await
            }
        }
    }

    async fn handle_notification(
        &self,
        notification: ServerNotification,
        context: NotificationContext<RoleClient>,
    ) -> Result<(), rmcp::ErrorData> {
        <LoggingClientHandler as Service<RoleClient>>::handle_notification(&self.handler, notification, context).await
    }

    fn get_info(&self) -> rmcp::model::ClientConfig {
        <LoggingClientHandler as Service<RoleClient>>::get_info(&self.handler)
    }
}

#[derive(Clone)]
pub(super) struct LoggingClientHandler {
    provider: String,
    initialize_params: InitializeRequestParams,
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    list_changed_state: Arc<ListChangedState>,
}

impl LoggingClientHandler {
    pub(super) fn new(
        provider_name: String,
        params: InitializeRequestParams,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
        list_changed_state: Arc<ListChangedState>,
    ) -> Self {
        Self {
            provider: provider_name,
            initialize_params: params,
            elicitation_handler,
            list_changed_state,
        }
    }

    pub(super) fn provider_name(&self) -> &str {
        &self.provider
    }

    async fn process_elicitation_request(
        &self,
        request: ElicitRequestParams,
    ) -> Result<crate::McpElicitationResponse, rmcp::ErrorData> {
        let provider = self.provider.clone();

        let default_response = crate::McpElicitationResponse {
            action: ElicitationAction::Decline,
            content: None,
            meta: None,
        };

        if let Some(handler) = &self.elicitation_handler {
            let (message, schema_value, request_meta) = match &request {
                ElicitRequestParams::FormElicitationParams { meta, message, requested_schema } => {
                    let schema_value = match serde_json::to_value(requested_schema) {
                        Ok(value) => value,
                        Err(err) => {
                            warn!(
                                provider = provider.as_str(),
                                error = %err,
                                "Failed to serialize MCP elicitation schema; using null placeholder"
                            );
                            Value::Null
                        }
                    };
                    (message.clone(), schema_value, serialize_elicitation_meta(provider.as_str(), meta.as_ref()))
                }
                ElicitRequestParams::UrlElicitationParams { meta, message, url, .. } => {
                    let schema_value = json!({
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "const": url
                            }
                        }
                    });
                    (message.clone(), schema_value, serialize_elicitation_meta(provider.as_str(), meta.as_ref()))
                }
                _ => {
                    warn!(
                        provider = provider.as_str(),
                        "Unknown elicitation request type; using default empty response"
                    );
                    ("".to_owned(), Value::Null, serialize_elicitation_meta(provider.as_str(), None))
                }
            };

            let validator = build_elicitation_validator(provider.as_str(), &schema_value);
            let payload = crate::McpElicitationRequest {
                message: message.clone(),
                requested_schema: schema_value.clone(),
                meta: request_meta,
            };

            match handler.handle_elicitation(&provider, payload).await {
                Ok(response) => {
                    validate_elicitation_payload(
                        provider.as_str(),
                        validator.as_ref(),
                        &response.action,
                        response.content.as_ref(),
                    )?;
                    info!(
                        provider = provider.as_str(),
                        message = message.as_str(),
                        action = ?response.action,
                        "MCP provider elicitation handled"
                    );
                    return Ok(response);
                }
                Err(err) => {
                    warn!(
                        provider = provider.as_str(),
                        message = message.as_str(),
                        error = %err,
                        "Failed to process MCP elicitation; declining"
                    );
                }
            }
        } else {
            let message_str = match &request {
                ElicitRequestParams::FormElicitationParams { message, .. } => message.as_str(),
                ElicitRequestParams::UrlElicitationParams { message, .. } => message.as_str(),
                _ => "unknown",
            };
            info!(
                provider = provider.as_str(),
                message = message_str,
                "MCP provider requested elicitation but no handler configured; declining"
            );
        }

        Ok(default_response)
    }

    #[allow(
        deprecated,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    fn handle_logging(&self, params: LoggingMessageNotificationParam) {
        let logger = params.logger.unwrap_or_default();
        let summary = params
            .data
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| params.data.to_string());

        match params.level {
            LoggingLevel::Debug => debug!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider log"
            ),
            LoggingLevel::Info | LoggingLevel::Notice => info!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider log"
            ),
            LoggingLevel::Warning => warn!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider warning"
            ),
            LoggingLevel::Error | LoggingLevel::Critical | LoggingLevel::Alert | LoggingLevel::Emergency => error!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider error"
            ),
        }
    }
}

impl ClientHandler for LoggingClientHandler {
    fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        context: RequestContext<RoleClient>,
    ) -> impl Future<Output = Result<rmcp::model::ElicitResult, rmcp::ErrorData>> + Send + '_ {
        let request = restore_context_meta(request, context.meta);
        async move {
            self.process_elicitation_request(request).await.map(|response| {
                let meta = response.meta.and_then(|value| {
                    value.as_object().cloned().map(MetaObject).or_else(|| {
                        warn!(
                            provider = self.provider.as_str(),
                            "Elicitation response meta is not an object; dropping _meta"
                        );
                        None
                    })
                });
                {
                    let mut result = rmcp::model::ElicitResult::new(response.action);
                    result.content = response.content;
                    result.meta = meta;
                    result
                }
            })
        }
    }

    #[allow(
        deprecated,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> impl Future<Output = Result<ListRootsResult, rmcp::ErrorData>> + Send + '_ {
        let provider = self.provider.clone();
        async move {
            let mut roots = Vec::new();
            match std::env::current_dir() {
                Ok(dir) => {
                    if let Some(uri) = directory_to_file_uri(&dir) {
                        roots.push(Root::new(uri).with_name("workspace"));
                    } else {
                        warn!(
                            provider = provider.as_str(),
                            path = %dir.display(),
                            "Failed to convert workspace directory to file URI for MCP roots"
                        );
                    }
                }
                Err(err) => {
                    warn!(
                        provider = provider.as_str(),
                        error = %err,
                        "Failed to resolve current directory for MCP roots"
                    );
                }
            }

            Ok(ListRootsResult::new(roots))
        }
    }

    fn on_cancelled(
        &self,
        params: CancelledNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        debug!(
            provider = self.provider.as_str(),
            request_id = ?params.request_id,
            reason = ?params.reason,
            "MCP provider cancelled request"
        );
        async move {}
    }

    fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            progress_token = ?params.progress_token,
            progress = params.progress,
            total = ?params.total,
            message = ?params.message,
            "MCP provider progress update"
        );
        async move {}
    }

    #[allow(
        deprecated,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    fn on_logging_message(
        &self,
        params: LoggingMessageNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        self.handle_logging(params);
        async move {}
    }

    fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        info!(provider = self.provider.as_str(), uri = params.uri.as_str(), "MCP resource updated");
        async move {}
    }

    fn on_resource_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        self.list_changed_state.mark_resources_changed();
        info!(provider = self.provider.as_str(), "MCP provider reported resource list change");
        async move {}
    }

    fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) -> impl Future<Output = ()> + Send + '_ {
        self.list_changed_state.mark_tools_changed();
        info!(provider = self.provider.as_str(), "MCP provider reported tool list change");
        async move {}
    }

    fn on_prompt_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        self.list_changed_state.mark_prompts_changed();
        info!(provider = self.provider.as_str(), "MCP provider reported prompt list change");
        async move {}
    }

    fn get_info(&self) -> rmcp::model::ClientConfig {
        convert_to_rmcp(self.initialize_params.clone()).unwrap_or_else(|error| {
            warn!(
                provider = self.provider.as_str(),
                error = %error,
                "Failed to convert MCP initialize params; using fallback client info"
            );
            rmcp::model::ClientConfig::new(Default::default(), crate::utils::build_client_implementation())
        })
    }
}

pub(super) fn restore_context_meta(
    mut request: ElicitRequestParams,
    mut context_meta: RequestMetaObject,
) -> ElicitRequestParams {
    drop(context_meta.remove(MCP_PROGRESS_TOKEN_META_KEY));
    if context_meta.is_empty() {
        return request;
    }

    match &mut request {
        ElicitRequestParams::FormElicitationParams { meta, .. }
        | ElicitRequestParams::UrlElicitationParams { meta, .. } => {
            meta.get_or_insert_with(RequestMetaObject::new).extend(context_meta);
        }
        _ => {}
    }

    request
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateElicitationResultWithMeta {
    action: ElicitationAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    meta: Option<Value>,
}

pub(super) fn elicitation_response_result(
    response: crate::McpElicitationResponse,
) -> Result<CustomResult, rmcp::ErrorData> {
    let result = CreateElicitationResultWithMeta {
        action: response.action,
        content: response.content,
        meta: response.meta,
    };

    serde_json::to_value(result)
        .map(CustomResult)
        .map_err(|err| rmcp::ErrorData::internal_error(err.to_string(), None))
}

fn serialize_elicitation_meta(provider: &str, meta: Option<&RequestMetaObject>) -> Option<Value> {
    meta.and_then(|meta| match serde_json::to_value(meta) {
        Ok(value) => Some(value),
        Err(err) => {
            warn!(
                provider = provider,
                error = %err,
                "Failed to serialize MCP elicitation metadata; dropping _meta"
            );
            None
        }
    })
}

pub(crate) fn build_elicitation_validator(provider: &str, schema: &Value) -> Option<Validator> {
    if schema.is_null() {
        return None;
    }

    match Validator::new(schema) {
        Ok(validator) => Some(validator),
        Err(err) => {
            warn!(
                provider = provider,
                error = %err,
                "Failed to build JSON schema validator for MCP elicitation; skipping validation"
            );
            None
        }
    }
}

pub(crate) fn validate_elicitation_payload(
    provider: &str,
    validator: Option<&Validator>,
    action: &ElicitationAction,
    content: Option<&Value>,
) -> Result<(), rmcp::ErrorData> {
    if !matches!(action, ElicitationAction::Accept) {
        return Ok(());
    }

    let Some(validator) = validator else {
        return Ok(());
    };

    let Some(payload) = content else {
        warn!(provider = provider, "MCP elicitation accept action missing response content");
        return Err(rmcp::ErrorData::invalid_params("Elicitation response missing content for accept action", None));
    };

    if !validator.is_valid(payload) {
        let messages: Vec<String> = validator.iter_errors(payload).map(|err| err.to_string()).collect();
        warn!(
            provider = provider,
            errors = ?messages,
            "MCP elicitation response failed schema validation"
        );
        return Err(rmcp::ErrorData::invalid_params(
            "Elicitation response failed schema validation",
            Some(json!({ "errors": messages })),
        ));
    }

    Ok(())
}

pub(crate) fn directory_to_file_uri(path: &Path) -> Option<String> {
    Url::from_directory_path(path).ok().map(|url| url.to_string())
}
